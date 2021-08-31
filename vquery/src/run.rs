use crate::runwalker;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

use crate::sample::Sample;
use lazy_static::lazy_static;
use regex::Regex;
use std::io::BufReader;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Debug)]
pub struct Run {
    pub date: time::Date,
    pub name: String,
    pub path: PathBuf,
    pub samples: Vec<Sample>,
    pub investigator: String,
    pub assay: String,
    pub description: String,
    pub chemistry: String,
}

fn is_fastq(s: &str) -> bool {
    s.ends_with(".fastq.gz")
        && !s.contains("Data")
        && !s.contains("Undetermined")
        && !s.contains("Archiv_")
}

fn parse_from_fastq(samples: &mut Vec<Sample>, fastq: &str) {
    lazy_static! {
        static ref RE_NAME: Regex = Regex::new(r"(?P<name>.*?)_S\d+_.*\.fastq\.gz$").unwrap();
    }

    // try to make a name for the fastq
    let mut p: PathBuf = PathBuf::from(fastq);
    let file_name = p.file_name().unwrap().to_string_lossy().to_string();
    p.pop();
    let dir_name = p.file_name().unwrap().to_string_lossy().to_string();
    let project = if dir_name.starts_with("data_") {
        String::from("")
    } else {
        dir_name
    };

    let mut s = if let Some(captures) = RE_NAME.captures(&file_name) {
        let mut s = Sample::default();
        s.name = captures.name("name").unwrap().as_str().to_string();
        parse_samplename(&mut s);
        s.project = project;
        s
    } else {
        let mut s = Sample::default();
        s.name = String::from("Unknown");
        s.project = project;
        s
    };

    // merge if there is already such a sample
    let mut found = false;
    for sample in samples.iter_mut() {
        if sample.name == s.name
            && sample.dna_nr == s.dna_nr
            && sample.primer_set == s.primer_set
            && sample.project == s.project
        {
            sample.files.push(fastq.to_string());
            found = true;
            break;
        }
    }

    if !found {
        s.files.push(fastq.to_string());
        samples.push(s);
    }
}

fn parse_samplename(s: &mut Sample) {
    lazy_static! {
        static ref RE_DNA: Regex = Regex::new(r"(?:D-)?(?P<dnanr>\d\d-\d{3,})").unwrap();
        static ref RE_PRIMER: Regex =
            Regex::new(r"_(?i)(?P<primer>IGH.*?|IGK.*?|FR.*?|Fr.*?|DJ|TRD.*?|TRB.*?|TRG.*?)(_|$)")
                .unwrap();
    }
    let oldname = s.name.clone().replace(" ", "_");
    if let Some(captures) = RE_DNA.captures(&oldname) {
        s.dna_nr = captures.name("dnanr").unwrap().as_str().to_string();
    }

    if let Some(captures) = RE_PRIMER.captures(&oldname) {
        s.primer_set = captures.name("primer").unwrap().as_str().to_string();
    } else {
        //debug!("No capture for {}", oldname);
    }
}

fn match_fastq(sample: &Sample, fastq: &str) -> bool {
    if sample.dna_nr.len() > 0 {
        if sample.primer_set.len() > 0 {
            return fastq.contains(&sample.dna_nr) && fastq.contains(&sample.primer_set);
        } else {
            return fastq.contains(&sample.dna_nr);
        }
    } else {
        return fastq.contains(&sample.name);
    }
}

/// Assign fastq files to samples
/// Strategy:
/// sort sample names by length (longest first), so we get the best matches
/// before one of the shorter prefixes could match, and then remove the matched
/// fastqs from the fastq file list
fn assign_fastqs(mut samples: &mut Vec<Sample>, mut fastqs: Vec<String>) -> usize {
    samples.sort_unstable_by_key(|s| s.name.len());
    samples.reverse();

    // match FASTQs by what we got from the sample sheet
    for s in samples.iter_mut() {
        // find fastqs that match the sample name and/or DNA number
        let myfastqs: Vec<(usize, String)> = fastqs
            .iter()
            .enumerate()
            .filter(|(_, f)| {
                let b = match_fastq(&s, &f);
                //debug!("{} -> {}: {}", &s.name, &f, b);
                b
            })
            .map(|(i, f)| (i, f.to_string()))
            .collect();

        // reset fastq in list to not shift indices around, and add to sample
        for (idx, file) in myfastqs.into_iter() {
            fastqs[idx].clear();
            if file.len() == 0 {
                error!("Trying to assign empty filename to sample {:?}", s);
            }
            s.files.push(file);
        }
    }

    // Create new samples, if necessary, based on what we can parse from the remaining
    // FASTQ filenames
    fastqs
        .into_iter()
        .filter(|f| !f.is_empty())
        .for_each(|f| parse_from_fastq(&mut samples, &f));

    return 0;
}
impl Run {
    /// Parses the run's SampleSheet.csv for auxiliary run information
    fn parse_samplesheet<R: Read>(&mut self, r: R, fastqs: Vec<String>) -> Result<()> {
        let b = BufReader::new(r);
        let mut data_mode = false;

        for line in b.lines() {
            if let Ok(linebuf) = line {
                let mut parts: Vec<&str> = linebuf.split(",").collect();

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
                    if parts[0] == "[Data]" {
                        if parts.len() > 10 {
                            warn!(
                                "{}: More than 10 colums, will ignore everything after column 10",
                                self.name
                            );
                        }
                        data_mode = true;
                    }
                } else {
                    if parts[0].to_ascii_lowercase() == "sample_id" {
                        continue;
                    }
                    if parts.len() < 2 {
                        continue;
                    }
                    let mut s: Sample = Sample::default();
                    if parts.len() > 10 {
                        parts.resize(10, "");
                    }
                    match parts.len() {
                        10 => {
                            s.project = parts[8].to_string();
                            s.name = parts[0].to_string();

                            // if it parses as unsigned number and it's positive, it might be a
                            // LIMS id.
                            if let Ok(id) = parts[9].parse::<i64>() {
                                if id > 0 {
                                    s.lims_id = id;
                                }
                            }
                        }
                        9 => {
                            s.project = parts[8].to_string();
                            s.name = parts[0].to_string();
                        }
                        7 => {
                            s.project = parts[5].to_string();
                            s.name = parts[0].to_string();
                        }
                        6 => {
                            s.project = parts[4].to_string();
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
                    if s.name.len() > 0 {
                        self.samples.push(s);
                    }
                }
            }
        }

        let orig_num = fastqs.len();
        let num = assign_fastqs(&mut self.samples, fastqs);
        if num > 0 {
            warn!(
                "{}: {} of {} fastqs were not assigned to samples",
                self.name, num, orig_num
            );
        }

        if self.samples.len() == 0 {
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

        let run_date = runwalker::parse_date(&run_name);

        // make fastq file list
        let walker = walkdir::WalkDir::new(&path).follow_links(true).into_iter();
        let fastqs: Vec<String> = walker
            .into_iter()
            .map(|e| {
                if let Ok(e) = e {
                    if e.depth() > 1 {
                        let s = e.path().display().to_string();
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
        if fastqs.len() == 0 {
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
            r.parse_samplesheet(&mut ssheet, fastqs)?;
        } else {
            warn!("{}: No SampleSheet.csv found, skipping!", run_name);
        }

        Ok(r)
    }

    /// Constructor delegation, will pick up run infos from a Zip file
    fn from_zip(path: &Path) -> Result<Self> {
        let mut z = ZipArchive::new(File::open(path)?)?;
        let run_name = path.file_stem().unwrap().to_string_lossy();

        let run_date = runwalker::parse_date(&run_name);

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
            r.parse_samplesheet(&mut ssheet, fastqs)?;
        } else {
            warn!("{}: No SampleSheet.csv found, skipping!", run_name);
        }

        Ok(r)
    }

    /// Create a `Run` instance from a given path.
    ///
    /// The path might either be a sequencing run directory or a zip file containing one.
    pub fn from_path(path: &Path) -> Result<Self> {
        if path.is_dir() {
            Self::from_dir(path)
        } else {
            Self::from_zip(path)
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
