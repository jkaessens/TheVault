#[macro_use]
extern crate diesel;

mod config;
mod run;
mod sample;
mod web;
mod vaultdb;


mod schema;
mod models;

use std::path::PathBuf;
use std::{collections::HashMap, error::Error, fs::File, io::BufRead, path::Path};
use std::io::Write;
use diesel::{PgConnection, QueryDsl, ExpressionMethods, RunQueryDsl};
use rayon::iter::IntoParallelIterator;
use rayon::iter::ParallelIterator;
use structopt::StructOpt;

extern crate log;

#[macro_use]
extern crate rocket;

type Result<T> = std::result::Result<T, Box<dyn Error>>;


fn extract_from_zip(path: &Path, fastqs: &[String],  targetdir: &Path) -> Result<()> {
    let zipfile = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(zipfile)?;
    for f in fastqs {
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

fn extract_from_dir(path: &Path, fastqs: &[String], targetdir: &Path) -> Result<()> {
    for f in fastqs {
        let mut src = path.to_path_buf();
        src.push(f);
        
        let mut target = PathBuf::from(targetdir);
        target.push(PathBuf::from(f).file_name().unwrap().to_string_lossy().to_string());

        std::fs::copy(&src, &target)?;
    }
    Ok(())
}

fn dump_fastq(conn: &PgConnection, samples: &HashMap<models::Sample, Vec<String>>, targetdir: &Path) {
    use crate::schema::run;

    // Make a list of paths that correspond to the runs
    let mut runs: Vec<&str> = samples.iter().map(|(s, _)| s.run.as_str()).collect();
    runs.sort();
    runs.dedup();

    info!(
        "Extracting fastq files from {} samples over {} runs",
        samples.len(),
        runs.len()
    );

    let mut runpaths: HashMap<String, PathBuf> = HashMap::new();
    for r in runs.into_iter() {
        let p: String = run::table
            .select(run::path)
            .filter(run::name.eq(r))
            .first(conn)
            .expect("Could not get run");
        runpaths.insert(r.to_string(), PathBuf::from(p));
        
    }

    samples.into_par_iter().for_each(|(sample, fastqs)| {
        let runpath = runpaths.get(&sample.run).unwrap();
        if let Some(ext) = runpath.extension() {
            if ext.to_ascii_lowercase() == "zip" {
                extract_from_zip(runpath, fastqs, targetdir).unwrap_or_else(|e| {
                    error!("Cannot extract from zip file {}: {}", runpath.display(), e)
                });
            } else {
                warn!(
                    "Run path {} has weird extension. Don't know what to do, skipping.",
                    sample.run
                );
            }
        } else {
            extract_from_dir(runpath, fastqs, targetdir)
                .unwrap_or_else(|e| error!("Cannot copy from run folder: {}", e));
        }
    });
    info!("Done");
}


fn dump_samplesheet(samples: &HashMap<models::Sample, Vec<String>>, targetfile: &Path) -> Result<()> {
    let mut ssheet = File::create(targetfile)?;
    
    write!(ssheet, "sample\trun\tcells\tprimer set\tproject\tLIMS ID\tDNA nr\n")?;
    for (s, _) in samples {
        
        write!(ssheet, "{}\t{}\t{}\t{}\t{}\t{}\t{}\n", 
            s.name, 
            s.run, 
            s.cells.unwrap_or(0), 
            &s.primer_set.as_ref().unwrap_or(&String::from("")), 
            s.project, 
            s.lims_id.unwrap_or(0), 
            s.dna_nr)?;
    }

    Ok(())
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
            limit,
            extract, 
            samplesheet } => {

            // collect queries from either stdin or a positional argument
            let mut queries: Vec<String> = Vec::new();

            if &query == "-" {
                for line in std::io::stdin().lock().lines() {
                    queries.push(line?);
                }
                info!("Performing {} queries...", queries.len());
            } else {
                queries.push(query);
            }

            // Collect filters
            let mut filters = HashMap::new();
            for f in filter.into_iter() {
                let parts = f.split("=").map(|p| p.to_string()).collect::<Vec<_>>();
                if parts.len() == 2 {
                    filters.insert(parts[0].to_string(), parts[1].to_string());
                } else {
                    error!("Ignoring malformed filter: {}", &f);
                }
            }

            // run the queries one after another and append the results to candidate list
            let conn = vaultdb::establish_connection(&config.connstr);
            let mut candidates: HashMap<models::Sample, Vec<String>> = HashMap::new();
            for q in queries {
                candidates.extend(vaultdb::query(&conn, &q, &filters, limit));
            }
            info!("{} candidates returned.", candidates.len());
            
            debug!("{:?}", candidates);
            
            // some more extras
            if let Some(targetdir) = extract {
                dump_fastq(&conn, &candidates, &targetdir);
            }
            
            if let Some(targetfile) = samplesheet {
                dump_samplesheet(&candidates, &targetfile)?;
            }
            
        }
        

        config::Command::Update { force, rundir, celldir } => {
            let conn = vaultdb::establish_connection(&config.connstr);
            if force {
                info!("Flushing database contents");
                vaultdb::flush(&conn);
            }
            vaultdb::update(&conn, &rundir, &celldir)?
        }
        
        config::Command::Web => {
            let _rocket = web::rocket();
            //block_on(rocket)
            
        }
    }

    Ok(())
}
