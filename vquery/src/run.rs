use std::io::prelude::*;
use zip::ZipArchive;
use std::path::{PathBuf, Path};
use std::error::Error;
use std::fs::File;
use crate::runwalker;

use sxd_xpath::evaluate_xpath;
use std::io::BufReader;
use crate::sample::Sample;

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

/// Collects sample information from a demultiplexing report.
///
/// Returns a `Vec<Sample>` where every `Sample` has the following fields populated:
/// - `name`
/// - `dna_nr` (if available)
/// - `project`
///
/// The remaining fields (currently only `files`) are default-initialized.
///
/// # Arguments
///
/// * `r` - a stream implementing the `Read` trait where the demultiplexing XML report will be read from
///
/// # Examples
///
/// ```
/// let samples = collect_samples(File::open("data_mm0/Stats/DemultiplexingStats.xml")?);
/// ```
/*
fn collect_samples<R: Read>(mut r: R) -> Vec<Sample> {
    let mut samples: Vec<Sample> = Vec::new();

    let mut content = "".to_owned();
    r.read_to_string(&mut content).expect("Failed to read DemultiplexingStats.xml");
    let xml = sxd_document::parser::parse(&content).expect("Failed to parse xml");
    let xpath_projects = "/Stats/Flowcell/Project/@name";
    let document = xml.as_document();

    let p = evaluate_xpath(&document, xpath_projects).expect("XPath failed");
    if let sxd_xpath::Value::Nodeset(ns) = p {
        for project in ns.into_iter() {
            let attr = project.attribute().unwrap();
            let project_name = attr.value();
            let s = evaluate_xpath(&document, &format!("/Stats/Flowcell/Project[@name='{}']/Sample/@name", project_name)).unwrap();
            if let sxd_xpath::Value::Nodeset(ns_s) = s {
                for sample in ns_s.into_iter() {
                    let attr = sample.attribute().unwrap();

                    if let Some(mut s) = parse_sample_name(attr.value()) {
                        s.project = project_name.to_owned();
                        samples.push(s);
                    }
                }
            }
        }
    }

    samples
}
*/


impl Run {
    /// Parses the run's SampleSheet.csv for auxiliary run information
    fn parse_samplesheet<R: Read>(&mut self, r: R) -> Result<()> {
        let mut b = BufReader::new(r);
        let mut data_mode = false;

        //let mut linebuf = String::new();
        for line in b.lines() {// let Ok(bytes) = b.read_line(&mut linebuf) {
            if let Ok(linebuf) = line {
                let parts: Vec<&str> = linebuf.split(",").collect();

                if !data_mode  {
                    if parts.len() >= 2 {
                        match parts[0] {
                            "Investigator Name" => { self.investigator = parts[1].to_owned().to_string() }
                            "Assay" => { self.assay = parts[1].to_owned().to_string() }
                            "Description" => { self.description = parts[1].to_owned().to_string() }
                            "Chemistry" => { self.chemistry = parts[1].to_owned().to_string() }
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
                            eprintln!("got {} columns: {:?}", parts.len(), parts);
                            return Err(Box::from("Expected 10 or 6 columns in [Data] section of sample sheet"));
                        }
                    }
                    self.samples.push(s);
                }
            }
        }
        Ok(())
    }

    /// Constructor delegation, will pick up run infos from a directory
    fn from_dir(path: &Path) -> Result<Self> {
        /*
        let mut demux_info = path.to_owned();
        demux_info.push("data_mm0");
        demux_info.push("Stats");
        demux_info.push("DemultiplexingStats.xml");



        let f = File::open(demux_info);

        if let Ok(demux) = f {
            samples = collect_samples(demux);
        } else {
            eprintln!("{}: No demultiplexing stats found, skipping sample discovery!", run_name);
        }
        */
        let run_name = path.components().last().unwrap().as_os_str().to_string_lossy();

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

        let mut ss = path.to_owned();
        ss.push("SampleSheet.csv");
        let f = File::open(ss);
        if let Ok(mut ssheet) = f {
            r.parse_samplesheet(&mut ssheet)?;
        } else {
            eprintln!("{}: No SampleSheet.csv found, skipping!", run_name);
        }
        Ok(r)
    }

    /// Constructor delegation, will pick up run infos from a Zip file
    fn from_zip(path: &Path) -> Result<Self> {
        let mut z = ZipArchive::new(File::open(path)?)?;
        let run_name = path.file_stem().unwrap().to_string_lossy();

        /*

        if let Ok(statsxml) = z.by_name(&format!("{}/data_mm0/Stats/DemultiplexingStats.xml", run_name)) {
            samples = collect_samples(statsxml);
        } else {
            eprintln!("{}: No demultiplexing stats found, skipping sample discovery!", run_name);
        }


         */

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

        if let Ok(mut ssheet) = z.by_name(&format!("{}/SampleSheet.csv", run_name)) {
            r.parse_samplesheet(&mut ssheet)?;
        } else {
            eprintln!("{}: No SampleSheet.csv found, skipping!", run_name);
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
    fn run_dir() ->  Result<()> {
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