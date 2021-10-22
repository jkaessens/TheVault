
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use chrono::Datelike;
use zip::ZipArchive;

use crate::models;
use crate::models::NewSample;
use crate::samplesheet::normalize_dna_nr;
use lazy_static::lazy_static;
use regex::Regex;
use std::io::BufReader;

use walkdir::WalkDir;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug)]
pub struct Run {
    pub date: chrono::NaiveDate,
    pub name: String,
    pub path: PathBuf,
    pub samples: Vec<(models::NewSample, Vec<String>)>,
    pub investigator: String,
    pub assay: String,
    pub description: String,
    pub chemistry: String,
}


/// Parses a date string from a run name, that typically starts with "YYMMDD"
fn parse_date(source: &str) -> Result<chrono::NaiveDate> {
    if source.len() < 6 {
        return Err(Box::from("Date string too short"));
    }
    let year = source[0..2].parse::<i32>()? + 2000;
    let month = source[2..4].parse::<u32>()?;
    let day = source[4..6].parse::<u32>()?;
    Ok(chrono::NaiveDate::from_ymd(year, month, day))
}


fn is_fastq(s: &str) -> bool {
    s.ends_with(".fastq.gz")
        && !s.contains("Data")
        && !s.contains("Undetermined")
        && !s.contains("Archiv_")
}

fn parse_from_fastq(samples: &mut Vec<(NewSample, Vec<String>)>, fastq: &str, run_name: &str) {
    lazy_static! {
        static ref RE_NAME: Regex = Regex::new(r"(?P<name>.*?)_S\d+_.*\.fastq\.gz$").unwrap();
    }

    // try to make a name for the fastq
    let mut p: PathBuf = PathBuf::from(fastq);
    let file_name = p.file_name().unwrap().to_string_lossy().to_string();
    p.pop();
    let dir_name = p.file_name().unwrap().to_string_lossy().to_string();
    let project = dir_name.starts_with("data_").then(|| dir_name);

    let s = if let Some(captures) = RE_NAME.captures(&file_name) {
        let mut s = NewSample {
            name: captures.name("name").unwrap().as_str().to_string(),
            project,
            .. Default::default()
        };
        parse_samplename(&mut s);
        s
    } else {
        NewSample {
            name: String::from("Unknown"),
            run: run_name.to_string(),
            project,
            .. Default::default()
        }
    };

    // merge if there is already such a sample
    let mut found = false;
    for (sample, files) in samples.iter_mut() {
        if sample.name == s.name
            && sample.dna_nr == s.dna_nr
            && sample.primer_set == s.primer_set
            && sample.project == s.project
        {
            files.push(fastq.to_string());
            found = true;
            break;
        }
    }

    if !found {
        samples.push((s, vec![fastq.to_string()]));
    }
}

fn parse_samplename(s: &mut models::NewSample) {
    lazy_static! {
        static ref RE_DNA: Regex = Regex::new(r"(?:D-)?(?P<dnanr>\d\d-\d{3,})").unwrap();
        static ref RE_PRIMER: Regex =
            Regex::new(r"_(?i)(?P<primer>IGH.*?|IGK.*?|FR.*?|Fr.*?|DJ|TRD.*?|TRB.*?|TRG.*?)(_|$)")
                .unwrap();
    }
    let oldname = s.name.clone().replace(" ", "_");
    if let Some(captures) = RE_DNA.captures(&oldname) {
        s.dna_nr = normalize_dna_nr(captures.name("dnanr").unwrap().as_str());
    }

    if let Some(captures) = RE_PRIMER.captures(&oldname) {
        s.primer_set = Some(captures.name("primer").unwrap().as_str().to_string());
    } else {
        //debug!("No capture for {}", oldname);
    }
}

fn match_fastq(sample: &NewSample, fastq: &str) -> bool {
    if let Some(dna_nr) = sample.dna_nr {
        if let Some(primer_set) = sample.primer_set.as_ref() {
            fastq.contains(&dna_nr) && fastq.contains(primer_set)
        } else {
            fastq.contains(&dna_nr)
        }
    } else {
        fastq.contains(&sample.name)
    }
}

/// Assign fastq files to samples
/// Strategy:
/// sort sample names by length (longest first), so we get the best matches
/// before one of the shorter prefixes could match, and then remove the matched
/// fastqs from the fastq file list
fn assign_fastqs(mut samples: &mut Vec<(NewSample, Vec<String>)>, mut fastqs: Vec<String>, run_name: &str) -> usize {
    samples.sort_unstable_by_key(|(s,_)| s.name.len());
    samples.reverse();

    // match FASTQs by what we got from the sample sheet
    for (s, files) in samples.iter_mut() {
        // find fastqs that match the sample name and/or DNA number
        #[allow(clippy::needless_collect)] // silence a false positive warning
        let myfastqs: Vec<(usize, String)> = fastqs
            .iter()
            .enumerate()
            .filter(|(_, f)| match_fastq(s, f))
            .map(|(i, f)| (i, f.to_string()))
            .collect();

        // reset fastq in list to not shift indices around, and add to sample
        for (idx, file) in myfastqs.into_iter() {
            fastqs[idx].clear();
            if file.is_empty() {
                error!("Trying to assign empty filename to sample {:?}", s);
            }
            files.push(file);
        }
    }

    // Create new samples, if necessary, based on what we can parse from the remaining
    // FASTQ filenames
    if !fastqs.is_empty() {
        debug!("Recovering {} samples from unmatched FASTQs", fastqs.len());
    }
    fastqs
        .into_iter()
        .filter(|f| !f.is_empty())
        .for_each(|f| parse_from_fastq(&mut samples, &f, run_name));

    0
}

impl Run {
    /// Tries to discover a spikeINBC.(txt|csv) file in a path constructed
    /// from the base directory, the run date and parts of the run name
    fn find_cellsheet(&mut self, basedir: &Path) -> Option<PathBuf> {
        // build path based on date
        let mut cellsheet_dir = PathBuf::from(basedir);
        let year = self.date.year();
        let month = self.date.month();
        cellsheet_dir.push(year.to_string());

        // No spikeINBC before 2016. During 2016, some weirdly formatted files
        // can be found but not parsed. From 2017 onwards, paths appear to be
        // stable
        cellsheet_dir.push(match month {
            1 => "01_Januar",
            2 => "02_Feburar",
            3 => "03_März",
            4 => "04_April",
            5 => "05_Mai",
            6 => "06_Juni",
            7 => "07_Juli",
            8 => "08_August",
            9 => "09_September",
            10 => "10_Oktober",
            11 => "11_November",
            12 => "12_Dezember",
            _ => panic!("Bad month number"),
        });
        
        // Now the run folder. It starts with the run date in YYYYMMDD format (instead of YYMMDD in the run name).
        // Then, the M number follows and then some more cruft, probably investigator names. Give WalkDir a sensible
        // prefix and let it find the proper one.
        let run_prefix = String::from("20") + &self.name.split_inclusive("_").collect::<Vec<&str>>()[0..2].concat();
        // Only keep the latest cellsheet if multiple can be found
        let mut latest_cellsheet = Option::<PathBuf>::None;
        let mut latest_date: i32 = 0;
        for entry in WalkDir::new(cellsheet_dir).max_depth(3)
                .into_iter()
                .filter_entry(|e| {
                    match e.depth() {
                        // root node
                        0 => true,
                        // run folder
                        1 => e.file_name().to_string_lossy().starts_with(&run_prefix),
                        // some folder starting with "Start_" and an unguessable suffix
                        2 => e.file_name().to_string_lossy().starts_with("Start_"),
                        // finally the spikeINBC file. 2017-2018 are usually .txt, since 2018 csv.
                        // Content is the same, it's only the extension.
                        3 => { let n = e.file_name().to_string_lossy(); n.ends_with("spikeINBC.txt") || n.ends_with("spikeINBC.csv") }
                        // shouldn't come here due to WalkDir max_depth setting
                        _ => panic!("Went too deep into cell sheet discovery"),
                    }
                })
                // ignore errors, just keep on looking
                .filter_map(|e| e.ok()) {
                    
            // only interested in spikeINBC files on level 3
            if entry.depth() == 3 {
                let file_name = entry.file_name().to_string_lossy();
                let parts = file_name.split('_').collect::<Vec<&str>>();
                if parts.len() == 1 && latest_date == 0{
                    latest_cellsheet = Some(entry.into_path());
                } else if parts.len() == 2 {
                    let this_date = parts[0].parse::<i32>().unwrap_or(0);
                    if this_date >= latest_date {
                        latest_cellsheet = Some(entry.into_path());
                        latest_date = this_date;
                    }
                }
            }
        }
        latest_cellsheet
    }

    /// Parses a given cellsheet and returns the number of samples that could be matched
    /// against the current run.
    fn parse_cellsheet(&mut self, csheet: &Path) -> Result<usize> {
        // 1 cell: 6.5 picogram DNA
        // 100 nanogram DNA = ca. 15384.6 cells
        // 1 nanogram DNA = ca. 153.846 cells
        static CELLS_PER_NG: f32 = 153.846;

        let mut samplecount = 0;

        let csheetf = File::open(csheet)?;
        for line in std::io::BufReader::new(csheetf).lines() {
            let line = match line {
                Err(e) =>{
                    // usually windows-1251 <-> UTF8 encoding issues
                    error!("Cannot parse {}: {}", csheet.display(), e);
                    continue;
                }
                Ok(line) => line
            };
            
            // expect 4 columns
            let parts = line.split(',').collect::<Vec<&str>>();
            if !line.is_empty() && parts.len() != 4 {
                debug!("{} cellsheet {} has {} columns!?", &self.name, csheet.display(), parts.len());
                return Err(Box::from("Malformed cellsheet"));
            }

            // skip header
            if parts[0] == "sample_ID" {
                continue;
            }

            // match cell sheet sample names against known samples
            // usually chokes on whitespaces, umlauts, missing hyphens in last names, etc
            // TODO: additionally try matching by dna_nr+primer_set
            let mut candidates: Vec<&mut models::NewSample> = self.samples.iter_mut().filter(|(s,_)| s.name == parts[0]).map(|(a,_)| a).collect();
            if candidates.len() != 1 {
                debug!("{} cell sheet {} entry {} matches {} known samples", self.name, csheet.display(), parts[0], candidates.len());
            } else {
                candidates[0].cells = parts[1]
                    .parse::<f32>()
                    .map(|f| (f * CELLS_PER_NG).round() as i32)
                    .ok();
                samplecount += 1;
            }
        }
        Ok(samplecount)
    }

    /// Parses the run's SampleSheet.csv for auxiliary run information
    fn parse_samplesheet<R: Read>(&mut self, r: R, fastqs: Vec<String>, run_name: &str) -> Result<()> {
        let b = BufReader::new(r);
        let mut data_mode = false;

        for linebuf in b.lines().flatten() {

            let mut parts: Vec<&str> = linebuf.split(',').collect();

            // part of samplesheet in windows INI style
            if !data_mode {
                if parts.len() >= 2 {
                    match parts[0] {
                        "Investigator Name" => {
                            self.investigator = parts[1].to_owned().to_string()
                        }
                        "Assay" => self.assay = parts[1].to_owned().to_string(),
                        "Description" => self.description = parts[1].to_owned().to_string(),
                        "Chemistry" => self.chemistry = parts[1].to_owned().to_string(),
                        _ => {}
                    }
                }
                // advance to second part, the CSV-like actual sample sheet
                if parts[0] == "[Data]" {
                    if parts.len() > 10 {
                        warn!(
                            "{}: More than 10 colums, will ignore everything after column 10",
                            self.name
                        );
                    }
                    data_mode = true;
                }
            // CSV-style part
            } else {
                if parts[0].to_ascii_lowercase() == "sample_id" {
                    continue;
                }
                if parts.len() < 2 {
                    continue;
                }
                let mut s: models::NewSample = Default::default();
                if parts.len() > 10 {
                    parts.resize(10, "");
                }
                match parts.len() {
                    10 => {
                        s.project = (!parts[8].is_empty()).then(|| parts[8].to_string());
                        s.name = parts[0].to_string();

                        // if it parses as unsigned number and it's positive, it might be a
                        // LIMS id.
                        if let Ok(id) = parts[9].parse::<i64>() {
                            if id > 0 {
                                s.lims_id = Some(id);
                            } else {
                                s.lims_id = None;
                            }
                        }
                    }
                    9 => {
                        s.project = (!parts[8].is_empty()).then(|| parts[8].to_string());
                        s.name = parts[0].to_string();
                    }
                    7 => {
                        s.project = (!parts[5].is_empty()).then(|| parts[5].to_string());
                        s.name = parts[0].to_string();
                    }
                    6 => {
                        s.project = (!parts[4].is_empty()).then(|| parts[4].to_string());
                        s.name = parts[0].to_string();
                    }
                    _ => {
                        error!(
                            "{}: Expected 10, 9, 7 or 6 columns. Got {} columns: {:?}",
                            self.name,
                            parts.len(),
                            parts
                        );
                        return Err(Box::from(
                            "Expected 10 or 6 columns in [Data] section of sample sheet",
                        ));
                    }
                }

                parse_samplename(&mut s);
                s.run = run_name.to_string();
                if !s.name.is_empty() {
                    self.samples.push( (s, Vec::new()) );
                }
            }
        }
        

        let orig_num = fastqs.len();
        let num = assign_fastqs(&mut self.samples, fastqs, run_name);
        if num > 0 {
            warn!(
                "{}: {} of {} fastqs were not assigned to samples",
                self.name, num, orig_num
            );
        }

        if self.samples.is_empty() {
            warn!("{}: Sample sheet for resulted in 0 samples", self.name);
        }
        Ok(())
    }

    /// Constructor delegation, will pick up run infos from a directory
    fn from_dir(path: &Path) -> Result<Self> {
        let run_name = path
            .components()
            .last()
            .unwrap()
            .as_os_str()
            .to_string_lossy();

        let run_date = parse_date(&run_name);

        // make fastq file list
        let walker = walkdir::WalkDir::new(&path).follow_links(true).into_iter();
        let fastqs: Vec<String> = walker
            .into_iter()
            .map(|e| {
                if let Ok(e) = e {
                    if e.depth() > 1 {
                        let s = e.path().display().to_string();
                        // cut off the root directory. We only want fastq paths relative to the run root
                        s[path.display().to_string().len() + 1..].to_string()
                    } else {
                        String::from("")
                    }
                } else {
                    String::from("")
                }
            })
            .filter(|e| is_fastq(e))
            .collect();
        if fastqs.is_empty() {
            error!("No fastqs for {}?", run_name);
        }

        let mut r = Run {
            date: run_date?,
            name: run_name.to_owned().to_string(),
            path: PathBuf::from(path),
            samples: Vec::new(),
            assay: String::from(""),
            chemistry: String::from(""),
            description: String::from(""),
            investigator: String::from(""),
        };

        let mut ss = path.to_owned();
        ss.push("SampleSheet.csv");
        let f = File::open(ss);
        if let Ok(mut ssheet) = f {
            r.parse_samplesheet(&mut ssheet, fastqs, &run_name)?;
        } else {
            warn!("{}: No SampleSheet.csv found, skipping!", run_name);
        }

        Ok(r)
    }

    /// Constructor delegation, will pick up run infos from a Zip file
    fn from_zip(path: &Path) -> Result<Self> {
        let mut z = ZipArchive::new(File::open(path)?)?;
        let run_name = path.file_stem().unwrap().to_string_lossy();

        let run_date = parse_date(&run_name);

        let mut r = Run {
            date: run_date?,
            name: run_name.to_owned().to_string(),
            path: PathBuf::from(path),
            samples: Vec::new(),
            assay: String::from(""),
            chemistry: String::from(""),
            description: String::from(""),
            investigator: String::from(""),
        };

        let fastqs: Vec<String> = z
            .file_names()
            .filter(|name| is_fastq(name))
            .map(|n| n.to_string())
            .collect();

        if let Ok(mut ssheet) = z.by_name(&format!("{}/SampleSheet.csv", run_name)) {
            r.parse_samplesheet(&mut ssheet, fastqs, &run_name)?;
        } else {
            warn!("{}: No SampleSheet.csv found, skipping!", run_name);
        }

        Ok(r)
    }

    /// Create a `Run` instance from a given path.
    ///
    /// The path might either be a sequencing run directory or a zip file containing one.
    pub fn from_path(rundir: &Path, cellsheetdir: &Path) -> Result<Self> {
        let run = if rundir.is_dir() {
            Self::from_dir(rundir)
        } else {
            Self::from_zip(rundir)
        };

        
        run.map(|mut r| {
            if let Some(csheet) = r.find_cellsheet(cellsheetdir) {
                if let Err(e) = r.parse_cellsheet(&csheet) {
                    warn!("{}: Found a cell sheet but could not parse it: {}", r.name, e);
                } else {
                    debug!("{}: Cell sheet imported", r.name);
                }
            } else {
                debug!("{}: No cell sheet found", r.name);
            }
            r
        })
    }

    pub fn to_schema_run(&self) -> models::Run {
        models::Run {
             assay: self.assay.clone(),
            chemistry: self.chemistry.clone(),
            date: self.date,
            description: if self.description.is_empty() { None } else { Some(self.description.clone()) },
            investigator: self.investigator.clone(),
            name: self.name.clone(),
            path: self.path.to_str().expect("Could not convert path to string").to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn run_dir() -> Result<()> {
        let r = Run::from_dir(Path::new("../test/210802_M70821_0114_000000000-DCWMD"))?;
        println!("Run: {:?}", r);
        Ok(())
    }

    #[test]
    fn run_zip() -> Result<()> {
        let r = Run::from_zip(Path::new("../test/210209_M70821_0070_000000000-DBPJW.zip"))?;
        println!("Run: {:?}", r);
        Ok(())
    }
}
