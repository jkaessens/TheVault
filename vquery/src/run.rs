use crate::runwalker;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

use crate::sample::Sample;
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

fn is_fastq(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.ends_with(".fastq.gz"))
        .unwrap_or(false)
}

impl Run {
    /// Parses the run's SampleSheet.csv for auxiliary run information
    fn parse_samplesheet<R: Read>(&mut self, r: R, fastqs: Vec<String>) -> Result<()> {
        let b = BufReader::new(r);
        let mut data_mode = false;

        for line in b.lines() {
            if let Ok(linebuf) = line {
                let parts: Vec<&str> = linebuf.split(",").collect();

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
                    } else if parts[0] == "[Data]" {
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
                    match parts.len() {
                        10 => {
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
                                "Expected 10 or 6 columns. Got {} columns: {:?}",
                                parts.len(),
                                parts
                            );
                            return Err(Box::from(
                                "Expected 10 or 6 columns in [Data] section of sample sheet",
                            ));
                        }
                    }

                    s.files = fastqs
                        .iter()
                        .filter(|fastq| fastq.contains(&s.name))
                        .map(|s| s.clone())
                        .collect();

                    self.samples.push(s);
                }
            }
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
        let mut fastqs: Vec<String> = Vec::new();
        let walker = walkdir::WalkDir::new(&path).into_iter();
        for entry in walker.filter_entry(|e| is_fastq(e)) {
            fastqs.push(entry?.path().to_string_lossy().to_string());
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
            .filter(|name| name.ends_with(".fastq.gz"))
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
