//! This module contains tools to build sample sheets from lists of samples,
//! and to export sample sheets to ARResT-compatible formats.

use std::{collections::HashMap, convert::TryInto, fs::File, io::Write, path::{Path, PathBuf}};
use std::error::Error;

use crate::{models, vaultdb::MatchStatus};

use calamine::{Reader, Xlsx, open_workbook};
use diesel::{PgConnection, QueryDsl, RunQueryDsl, ExpressionMethods};
use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};


/// A catch-all error type
type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// A sample sheet containing a list of samples
#[derive(Debug)]
pub struct SampleSheet {
    /// The entries of the sample sheet
    pub entries: Vec<SampleSheetEntry>,
}

/// An entry of a SampleSheet
#[derive(Debug,Default)]
pub struct SampleSheetEntry {
    /// Sample data accoring to the database
    pub model: models::Sample,

    /// Columns usually imported from an external sample sheet.
    /// These entries can overlap with basic data. During export,
    /// the `override` settings control which one to use.
    pub extra_cols: HashMap<String, String>
}

/// Convert DNA numbers to XX-XXXXX format, will be filled up with zeros if necessary.
/// 
/// If a DNA number is in a supported format, it will be normalized to a two-digit year
/// enconding, a dash sign `-` and a five-digit number. A supported input format
/// * may or may not start with a `D-` prefix
/// * must contain a number, dash, number sequence
/// 
/// If `dnanr` is not in a supported format, `None` is returned.
/// 
/// # Example
/// ```
/// assert_eq!(Some("01-12345"), normalize_dna_nr("01-12345"))
/// assert_eq!(Some("01-00123"), normalize_dna_nr("01-345"))
/// assert_eq!(Some("01-00123"), normalize_dna_nr("D-1-345"))
/// assert_eq!(None, normalize_dna_nr("asdfjklö"))
/// ```
pub(crate) fn normalize_dna_nr(dnanr: &str) -> Option<String> {
    
    let dnanr = dnanr.strip_prefix("D-").unwrap_or(dnanr);
    let parts: Vec<&str> = dnanr.split('-').collect();
    if parts.len() != 2 {
        return None;
    }
    Some(format!(
        "{:02}-{:05}",
        parts[0].parse::<u32>().unwrap(),
        parts[1].parse::<u32>().unwrap()
    ))
}

impl SampleSheetEntry {

    pub fn _run_path(&self, db: &PgConnection) -> Result<PathBuf> {
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
        let underscore_parts: Vec<&str> = self.model.run.split('_').collect();
        let dash_parts: Vec<&str> = self.model.run.split('-').collect();
        format!("{}-{}", underscore_parts[0], dash_parts[dash_parts.len()-1])
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

impl From<Vec<models::Sample>> for SampleSheet {
    fn from(ss: Vec<models::Sample>) -> Self {
        SampleSheet {
            entries: ss.into_iter().map(|s| s.into()).collect()
        }
    }
}

fn extract_from_zip(path: &Path, fastqs: &[String],  targetdir: &Path, sample_prefix: Option<String>) -> Result<()> {
    let zipfile = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(zipfile)?;
    let prefix = sample_prefix.unwrap_or_else(|| String::from(""));

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
    let prefix = sample_prefix.unwrap_or_else(|| String::from(""));

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

    pub fn from_xlsx(xlsx: &str, db: &PgConnection) -> Result<Self> {
        // open Excel workbook
        let mut ss: Xlsx<_> = open_workbook(xlsx)?;
        let sheetname = ss.sheet_names()[0].clone();
        let sheet = ss.worksheet_range(&sheetname).unwrap()?;

        let header_row: Vec<String> = sheet.rows().next().unwrap().iter().map(|d| d.to_string()).collect();
        let col_dna_nr = header_row.iter().position(|c| *c == "DNA nr");
        let col_lims_id = header_row.iter().position(|c| *c == "LIMS ID");
        let col_sample = header_row.iter().position(|c| *c == "Sample");
        let col_primer_set = header_row.iter().position(|c| *c == "primer set");
        let col_run = header_row.iter().position(|c| *c == "run").ok_or_else(|| Box::<dyn Error>::from("Could not find required column 'run'"))?;

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
        runs.sort_unstable();
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
            let prefix = if runs.len() > 1 { Some( format!("{}-", entry.get_unique_run_id()) ) } else { None };

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


    pub fn write_csv<T: AsRef<str> + PartialEq> (&self, separator: &str, overrides: &[T], outfile: &Path) -> Result<()> {
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
        if !all_sans_basic.is_empty() {
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
                        "DNA nr" => { csv += &e.model.dna_nr.as_ref().unwrap_or(&String::from("")); },
                        "primer set" => { csv += e.model.primer_set.as_ref().unwrap_or(&String::from("")); },
                        "project" => { csv += &e.model.project.as_ref().unwrap_or(&String::from("")); },
                        "LIMS ID" => { csv += &e.model.lims_id.map(|i| i.to_string()).unwrap_or_else(|| String::from("")); },
                        "cells" => { 
                            if let Some(cells) = e.model.cells.as_ref() {
                                csv += &cells.to_string()
                            } else if let Some(cells) = e.extra_cols.get(*col) {
                                csv += cells
                            }
                            
                        },
                        s=> { error!("Unknown header: {}", s); panic!("Matching unknown basic header?!") },
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
        
        File::create(outfile)?.write_all(csv.as_bytes())?;
        Ok(())
    }

    pub fn write_xlsx<T: AsRef<str> + PartialEq> (&self, overrides: &[T], outfile: &Path) -> Result<()> {

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

        // set up an empty file
        let workbook = xlsxwriter::Workbook::new(outfile.to_str().unwrap());
        let mut sheet = workbook.add_worksheet(None)?;
        
        // write header
        for (col, title) in basic_header.iter().chain(all_sans_basic.iter()).enumerate() {
            sheet.write_string(0, col.clamp(0, u16::MAX.into()) as u16, title, None)?;
        }
        let has_multiple_runs = self.has_multiple_runs();

        for (row, e) in self.entries.iter().enumerate() {
            let row: u32 = (row + 1).try_into().unwrap();
            // write basic data points
            for (col_idx, colname) in basic_header.iter().enumerate() {
                let col_idx: u16 = col_idx.try_into().unwrap();
                
                let val = if overrides.iter().any(|x| &x.as_ref() == colname) {
                    e.extra_cols.get(*colname).unwrap_or(&String::from("")).to_string()
                } else {
                    match *colname {
                        "Sample" => { 
                            if has_multiple_runs {
                                format!("{}-{}", e.get_unique_run_id(), e.model.name)
                            } else {
                                e.model.name.to_string()
                            }
                        },
                        "run" => { e.model.run.to_string() },
                        "DNA nr" => { e.model.dna_nr.as_ref().map(|s| s.clone()).unwrap_or(String::from("")) },
                        "primer set" => { e.model.primer_set.as_ref().unwrap_or(&String::from("")).to_string() },
                        "project" => { e.model.project.as_ref().map(|s| s.clone()).unwrap_or(String::from("")) },
                        "LIMS ID" => { e.model.lims_id.map(|i| i.to_string()).unwrap_or_else(|| String::from("")) },
                        "cells" => { 
                            if let Some(cells) = e.model.cells.as_ref() {
                                cells.to_string()
                            } else if let Some(cells) = e.extra_cols.get(*colname) {
                                cells.to_string()
                            } else {
                                String::from("")
                            }
                            
                        },
                        s=> { error!("Unknown header: {}", s); panic!("Matching unknown basic header?!") },
                    }
                };

                sheet.write_string(row, col_idx, &val, None)?;
            }


            // write non-basic columns (extra cols from sample sheet)
            for (col_idx, col) in all_sans_basic.iter().enumerate() {
                let col_idx: u16 = (basic_header.len() + col_idx).try_into().unwrap();
                sheet.write_string(row, col_idx, e.extra_cols.get(*col).unwrap_or(&String::from("")), None)?;
            }
        }
        
        Ok(())
    }
}
