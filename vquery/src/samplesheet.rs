use std::{collections::HashMap, path::{Path, PathBuf}};

use calamine::{Reader, Xlsx, open_workbook};
use diesel::{PgConnection, QueryDsl, RunQueryDsl, ExpressionMethods};
use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};
use std::error::Error;
use crate::{models, vaultdb::MatchStatus};


type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug)]
pub struct SampleSheet {
    pub entries: Vec<SampleSheetEntry>,
}

#[derive(Debug,Default)]
pub struct SampleSheetEntry {
    pub model: models::Sample,
    pub extra_cols: HashMap<String, String>
}


impl SampleSheetEntry {
    fn new() -> Self {
        SampleSheetEntry {
            model: models::Sample::default(), 
            extra_cols: HashMap::new()
        }
    }
    pub fn from_model(s: &models::Sample) -> SampleSheetEntry {
        SampleSheetEntry {
            model: s.clone(),
            extra_cols: HashMap::new()
        }
    }

    pub fn run_path(&self, db: &PgConnection) -> Result<PathBuf> {
        use crate::schema::run;
        let p: String = run::table.select(run::path).filter(run::name.eq(&self.model.run)).get_result(db)?;
        Ok(PathBuf::from(p))
    }

    pub fn fastq_paths(&self, db: &PgConnection) -> Result<Vec<String>> {
        use crate::schema::fastq;
        Ok(fastq::table.select(fastq::filename).filter(fastq::sample_id.eq(self.model.id)).load(db)?)
    }

    // generate a short but unique string representation of the run
    // to keep samples with same characteristics in different runs apart
    fn get_unique_run_id(&self) -> String {
        let parts: Vec<&str> = self.model.run.split("-").collect();
        format!("{}-{}", parts[0], parts[parts.len()-1])
    }

    pub fn from_row(row: &[&str], header: &[&str]) -> SampleSheetEntry {
        let mut sse = SampleSheetEntry::default();
        for (idx, col) in header.into_iter().enumerate() {
            match *col {
                "Sample" => { sse.model.name = row[idx].to_string() },
                "DNA nr" => { sse.model.dna_nr = row[idx].to_string() },
                "LIMS ID" => { sse.model.lims_id = match row[idx].to_string().parse::<i64>() {
                    Ok(0) => None,
                    Ok(s) => Some(s),
                    Err(_) => None,
                }},
                "project" => { sse.model.project = row[idx].to_string() },
                "primer_set" => { sse.model.primer_set = match row[idx] {
                    "" => None,
                    s => Some(s.to_string()),
                }},
                "run" => { sse.model.run = row[idx].to_string() },
                "cells" => { 
                    sse.model.cells = row[idx].to_string().parse::<i32>().ok()
                },
                key@_ => {
                    sse.extra_cols.insert(key.to_string(), row[idx].to_string());
                }
            }
        }
        unimplemented!()
    }
}

impl From<models::Sample> for SampleSheetEntry {
    fn from(s: models::Sample) -> Self {
        SampleSheetEntry {
            model: s,
            extra_cols: HashMap::new()
        }
    }
}

fn extract_from_zip(path: &Path, fastqs: &[String],  targetdir: &Path, sample_prefix: Option<String>) -> Result<()> {
    let zipfile = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(zipfile)?;
    let prefix = sample_prefix.unwrap_or(String::from(""));

    for f in fastqs {
        let mut fastq = zip.by_name(f)?;

        let target = PathBuf::from(fastq.name());
        let mut local_path = PathBuf::from(targetdir);
        
        local_path.push(prefix.clone() + &target.file_name().unwrap().to_string_lossy().to_string());
        
        let mut targetfile = std::fs::File::create(local_path)?;
        std::io::copy(&mut fastq, &mut targetfile)?;
    }
    Ok(())
}

fn extract_from_dir(path: &Path, fastqs: &[String], targetdir: &Path, sample_prefix: Option<String>) -> Result<()> {
    let prefix = sample_prefix.unwrap_or(String::from(""));

    for f in fastqs {
        let mut src = path.to_path_buf();
        src.push(f);
        
        let mut target = PathBuf::from(targetdir);
        target.push(prefix.clone() + &PathBuf::from(f).file_name().unwrap().to_string_lossy().to_string());

        std::fs::copy(&src, &target)?;
    }
    Ok(())
}

impl SampleSheet {
    pub fn new() -> Self {
        SampleSheet {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, s: models::Sample) {
        self.entries.push(s.into());
    }

    pub fn from_models(ss: &[&models::Sample]) -> Self {
        SampleSheet {
            entries: ss.iter().map(|s| SampleSheetEntry::from_model(s)).collect(),
        }
    }

    pub fn from_xlsx(xlsx: &str, db: &mut PgConnection) -> Result<Self> {
        // open Excel workbook
        let mut ss: Xlsx<_> = open_workbook(xlsx)?;
        let sheetname = ss.sheet_names()[0].clone();
        let sheet = ss.worksheet_range(&sheetname).unwrap()?;

        let header_row: Vec<String> = sheet.rows().next().unwrap().into_iter().map(|d| d.to_string()).collect();
        let col_dna_nr = header_row.iter().position(|c| *c == "DNA nr");
        let col_lims_id = header_row.iter().position(|c| *c == "LIMS ID");
        let col_sample = header_row.iter().position(|c| *c == "Sample");
        let col_primer_set = header_row.iter().position(|c| *c == "primer set");
        let col_run = header_row.iter().position(|c| *c == "run").ok_or(Box::<dyn Error>::from("Could not find required column 'run'"))?;

        let mut result = SampleSheet::new();
        for (row_idx, row) in sheet.rows().skip(1).enumerate() {
            let run = row[col_run].to_string();
            let name = col_sample.map(|col| row[col].to_string());
            let primer_set = col_primer_set.map(|col| row[col].to_string());
            let lims_id = col_lims_id.map(|col| row[col].to_string().parse::<i64>().ok()).flatten();
            let dna_nr = col_dna_nr.map(|col| row[col].to_string());            

            let mut entry: SampleSheetEntry = match crate::vaultdb::match_samples(db, lims_id, dna_nr, primer_set, name, run)? {
                MatchStatus::None(reason) => { warn!("Cannot find match for sample in row {}. Skipping. Reason: {}", row_idx+2, reason); continue }
                MatchStatus::One(sample) => sample.into(),
                MatchStatus::Multiple(v) => { warn!("Found {} matches for sample in row {}. Skipping.", row_idx+2, v.len()); continue }
            };

            // put all sample sheet columns as extra columns. During export, the user may select which one to use.
            // Defaults to what the DB already knows
            entry.extra_cols = header_row.iter().cloned().zip(row).map(|(header,data)| (header, data.to_string())).collect();
            
            result.entries.push(entry);
        }

        Ok(result)
    }

    pub fn has_multiple_runs(&self) -> bool {
        self.entries.iter().map(|e| (e.model.run.clone(), true)).collect::<HashMap<String,bool>>().into_keys().count() > 1
    }

    pub fn extract_fastqs(&self, db: &PgConnection, targetpath: &Path) -> Result<()> {
        // Make a list of paths that correspond to the runs so we can aggregate the ZIP extractions by ZIP file/run path
        let mut runs: Vec<&str> = self.entries.iter().map( |e| e.model.run.as_ref()).collect();
        runs.sort();
        runs.dedup();

        // Discover actual run path for runs
        let runpaths: HashMap<String,String> = { 
            use crate::schema::run;
            run::table
                .select((run::name, run::path))
                .filter(run::name.eq_any(&runs))
                .load(db)
                .expect("Could not get run")
        }.into_iter().collect();

        // Collect run paths before we go into parallel extraction
        let files: Vec<Vec<String>> = self.entries.iter().map(|e| e.fastq_paths(db)).collect::<Result<_>>()?;
 
        // Extract FASTQs from runs sample-wise in parallel, adding a sample prefix on-the-fly
        self.entries.par_iter().enumerate().for_each(|(idx, entry)| {
            let runpath = PathBuf::from(runpaths.get(&entry.model.run).unwrap());
            let fastqs = &files[idx];
            let prefix = if runs.len() > 1 { Some( entry.get_unique_run_id()) } else { None };

            if let Some(ext) = runpath.extension() {
                if ext.to_ascii_lowercase() == "zip" {
                    extract_from_zip(&runpath, fastqs.as_ref(), targetpath, prefix).unwrap_or_else(|e| {
                        error!("Cannot extract from zip file {}: {}", runpath.display(), e)
                    });
                } else {
                    warn!(
                        "Run path {} has weird extension. Don't know what to do, skipping.",
                        entry.model.run
                    );
                }
            } else {
                extract_from_dir(&runpath, fastqs.as_ref(), targetpath, prefix)
                    .unwrap_or_else(|e| error!("Cannot copy from run folder: {}", e));
            }
        });
        Ok(())
    }


    pub fn write_csv<T: AsRef<str> + PartialEq> (&self, separator: &str, overrides: &[T]) -> String {
        let basic_header = vec!["Sample", "run", "DNA nr", "primer set", "project", "LIMS ID", "cells"];
        
        // extra_cols hashmap is not necessarily fully populated for every sample, so check all
        let mut all_headers: Vec<String> = self.entries
                .iter()
                .map::<Vec<String>,_>(|e| e.extra_cols.keys().cloned().collect())
                .flatten()
                .collect();
        all_headers.sort_unstable();
        all_headers.dedup();

        //...to not have duplicates in the header lines where extra_cols and the basic headers would overlap
        let all_sans_basic: Vec<&str> = all_headers.iter().filter(|&h| !basic_header.contains(&(**h).as_ref())).map(|s| s.as_ref()).collect();

        // write header
        let mut csv = basic_header.join(separator);
        if all_sans_basic.len() > 0 {
            csv += separator;
            csv += &all_sans_basic.join(separator);
        }
        csv += "\n";

        let has_multiple_runs = self.has_multiple_runs();

        for e in &self.entries {
            // write basic data points
            for (col_idx, col) in basic_header.iter().enumerate() {
                let last = col_idx+1 == basic_header.len();
                if overrides.iter().any(|x| &x.as_ref() == col) {
                    csv += e.extra_cols.get(*col).unwrap_or(&String::from(""));
                } else {
                    match *col {
                        "Sample" => { 
                            if has_multiple_runs {
                                csv += &format!("{}-{}", e.get_unique_run_id(), e.model.name);
                            } else {
                                csv += &e.model.name; 
                            }
                        },
                        "run" => { csv += &e.model.run; },
                        "DNA nr" => { csv += &e.model.dna_nr; },
                        "primer set" => { csv += e.model.primer_set.as_ref().unwrap_or(&String::from("")); },
                        "project" => { csv += &e.model.project; },
                        "LIMS ID" => { csv += &e.model.lims_id.map(|i| i.to_string()).unwrap_or(String::from("")); },
                        "cells" => { 
                            if let Some(cells) = e.model.cells.as_ref() {
                                csv += &cells.to_string()
                            } else if let Some(cells) = e.extra_cols.get(*col) {
                                csv += cells
                            }
                            
                        },
                        s@_ => { error!("Unknown header: {}", s); panic!("Matching unknown basic header?!") },
                    }
                };
                if !last {
                    csv += separator;
                }
            }

            if !all_sans_basic.is_empty() {
                csv += separator;
            }

            // write non-basic columns (extra cols from sample sheet)
            for (col_idx, col) in all_sans_basic.iter().enumerate() {
                csv += e.extra_cols.get(*col).unwrap_or(&String::from(""));
                if col_idx+1 < all_sans_basic.len() {
                    csv += separator;
                }
            }

            csv += "\n";
        }
        
        csv
    }
}
