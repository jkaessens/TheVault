mod config;
mod database;
mod run;
mod runwalker;
mod sample;

use crate::database::Database;
use crate::sample::Sample;
use rayon::prelude::*;
use std::collections::HashMap;
use std::error::Error;
use std::io::prelude::*;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use std::fs::File;

#[macro_use]
extern crate log;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn extract_from_zip(path: &Path, sample: &Sample, targetdir: &Path) -> Result<()> {
    let zipfile = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(zipfile)?;
    for f in &sample.files {
        let mut fastq = zip.by_name(f)?;

        let target = PathBuf::from(fastq.name());
        let mut local_path = PathBuf::from(targetdir);
        local_path.push(target.file_name().unwrap());
        /*
        if local_path.exists() {
            warn!(
                "Instead of overwriting {}, I'm storing {} as {}.",
                local_path.display(),
                local_path.display(),
                new_path.display()
            )
        }
        */
        let mut targetfile = std::fs::File::create(local_path)?;
        std::io::copy(&mut fastq, &mut targetfile)?;
    }
    Ok(())
}

fn extract_from_dir(path: &Path, sample: &Sample, targetdir: &Path) -> Result<()> {
    for f in &sample.files {
        let mut src = path.to_path_buf();
        src.push(f);
        
        let mut target = PathBuf::from(targetdir);
        target.push(PathBuf::from(f).file_name().unwrap().to_string_lossy().to_string());

        std::fs::copy(&src, &target)?;
    }
    Ok(())
}

fn dump_samplesheet(samples: &Vec<(String, Sample)>, targetfile: &Path) -> Result<()> {
    let mut ssheet = File::create(targetfile)?;
    
    write!(ssheet, "sample\trun\tcells\tprimer set\tproject\tLIMS ID\tDNA nr\n")?;
    for (r, s) in samples {
        write!(ssheet, "{}\t{}\t{}\t{}\t{}\t{}\t{}\n", s.name, r, s.cells, s.primer_set, s.project, s.lims_id, s.dna_nr)?;
    }

    Ok(())
}

fn dump_fastq(db: &mut Database, samples: &Vec<(String, Sample)>, targetdir: &Path) {

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
        let runpath = runpaths.get(&*runname).unwrap();
        if let Some(ext) = runpath.extension() {
            if ext.to_ascii_lowercase() == "zip" {
                extract_from_zip(runpath, &sample, targetdir).unwrap_or_else(|e| {
                    error!("Cannot extract from zip file {}: {}", runpath.display(), e)
                });
            } else {
                warn!(
                    "Run path {} has weird extension. Don't know what to do, skipping.",
                    runname
                );
            }
        } else {
            extract_from_dir(runpath, &sample, targetdir)
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
            filter, 
            extract, 
            samplesheet } => {

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
            
            if let Some(targetdir) = extract {
                dump_fastq(&mut db, &candidates, &targetdir);
            }

            if let Some(targetfile) = samplesheet {
                dump_samplesheet(&candidates, &targetfile)?;
            }
        }
        config::Command::Update { force, rundir, celldir } => {
            let mut db = Database::new(&config.connstr, force)?;
            db.update(&rundir, &celldir)?;
        }
        config::Command::Initialize => {
            Database::new(&config.connstr, true)?;
        }
    }

    Ok(())
}
