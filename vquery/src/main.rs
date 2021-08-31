mod config;
mod database;
mod query;
mod run;
mod runwalker;
mod sample;

use crate::config::OutputType;
use crate::database::Database;
use crate::sample::Sample;
use rayon::prelude::*;
use std::collections::HashMap;
use std::error::Error;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use structopt::StructOpt;

#[macro_use]
extern crate log;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn dump_csv(samples: Vec<(String, Sample)>, sep: &str) {
    for s in samples.into_iter() {
        let fastqs = s.1.files.join(" ");
        println!(
            "{}",
            vec![
                s.0,
                s.1.dna_nr,
                s.1.primer_set,
                s.1.project,
                s.1.name,
                fastqs
            ]
            .join(sep)
        );
    }
}

fn extract_from_zip(path: &Path, sample: &Sample) -> Result<()> {
    let zipfile = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(zipfile)?;
    for f in &sample.files {
        let mut fastq = zip.by_name(f)?;

        let target = PathBuf::from(fastq.name());
        let mut targetfile = std::fs::File::create(target.file_name().unwrap())?;
        std::io::copy(&mut fastq, &mut targetfile)?;
    }
    Ok(())
}

fn extract_from_dir(path: &Path, sample: &Sample) -> Result<()> {
    for f in &sample.files {
        let mut src = path.to_path_buf();
        src.push(f);
        let target = PathBuf::from(f)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        std::fs::copy(&src, &target)?;
    }
    Ok(())
}

fn dump_fastq(db: &mut Database, samples: Vec<(String, Sample)>) {
    // Make a list of paths that correspond to the runs
    let mut runs: Vec<String> = samples.iter().map(|(run, _)| run.to_owned()).collect();
    runs.sort();
    runs.dedup();

    info!(
        "Extracting fastq files from {} samples over {} runs",
        samples.len(),
        runs.len()
    );

    let mut runpaths: HashMap<String, PathBuf> = HashMap::new();
    for run in runs.into_iter() {
        let run = db.get_run(&run, false).unwrap();
        runpaths.insert(run.name, run.path);
    }

    samples.into_par_iter().for_each(|(runname, sample)| {
        let runpath = runpaths.get(&runname).unwrap();
        if let Some(ext) = runpath.extension() {
            if ext.to_ascii_lowercase() == "zip" {
                extract_from_zip(runpath, &sample).unwrap_or_else(|e| {
                    error!("Cannot extract from zip file {}: {}", runpath.display(), e)
                });
            } else {
                warn!(
                    "Run path {} has weird extension. Don't know what to do, skipping.",
                    runname
                );
            }
        } else {
            extract_from_dir(runpath, &sample)
                .unwrap_or_else(|e| error!("Cannot copy from run folder: {}", e));
        }
    });
    info!("Done");
}

fn main() -> Result<()> {
    let config = config::Opt::from_args();

    // set up logging
    env_logger::init();

    // set up global thread pool
    rayon::ThreadPoolBuilder::new()
        .num_threads(config.threads)
        .build_global()?;

    match config.cmd {
        config::Command::Query {
            query,
            format,
            filter,
        } => {
            let mut db = Database::new(&config.connstr, false)?;
            let mut queries: Vec<String> = Vec::new();

            if &query == "-" {
                for line in std::io::stdin().lock().lines() {
                    queries.push(line?);
                }
                info!("Performing {} queries...", queries.len());
            } else {
                queries.push(query);
            }

            let mut candidates: Vec<(String, Sample)> = Vec::new();
            for q in queries {
                let mut these_candidates = db.find_samples(&q, &filter)?;
                candidates.append(&mut these_candidates);
            }
            info!("{} candidates returned.", candidates.len());
            match format {
                OutputType::CSV => dump_csv(candidates, ","),
                OutputType::TSV => dump_csv(candidates, "\t"),
                OutputType::Fastq => dump_fastq(&mut db, candidates),
            }
        }
        config::Command::Update { force, rundir } => {
            let mut db = Database::new(&config.connstr, force)?;
            db.update(&rundir)?;
        }
        config::Command::Initialize => {
            Database::new(&config.connstr, true)?;
        }
    }

    Ok(())
}
