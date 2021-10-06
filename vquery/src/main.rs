#[macro_use]
extern crate diesel;

mod config;
mod run;
mod web;
mod vaultdb;
mod samplesheet;

mod schema;
mod models;

use std::path::PathBuf;
use std::{collections::HashMap, error::Error, fs::File, io::BufRead};
use std::io::Write;
use diesel::PgConnection;
use env_logger::Env;
use structopt::StructOpt;

extern crate log;

#[macro_use]
extern crate rocket;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn query(conn: PgConnection, query: String, filter: Vec<String>, limit: Option<usize>, extract: Option<PathBuf>, samplesheet: Option<PathBuf>) -> Result<()> {
    // collect queries from either stdin or a positional argument
    let mut queries: Vec<String> = Vec::new();

    if &query == "-" {
        for line in std::io::stdin().lock().lines() {
            let mut line  = line?;
            if !line.contains('%') {
                line = format!("%{}%", line);
            }
            queries.push(line);
        }
        info!("Performing {} queries...", queries.len());
    } else {
        let mut line  = query;
        if !line.contains('%') {
            line = format!("%{}%", line);
        }
        queries.push(line);
    }

    // Collect filters
    let mut filters = HashMap::new();
    for f in filter.into_iter() {
        let parts = f.split('=').map(|p| p.to_string()).collect::<Vec<_>>();
        if parts.len() == 2 {
            filters.insert(parts[0].to_string(), parts[1].to_string());
        } else {
            error!("Ignoring malformed filter: {}", &f);
        }
    }

    // run the queries one after another and append the results to candidate list
    let mut candidates: HashMap<models::Sample, Vec<String>> = HashMap::new();
    for q in queries {
        candidates.extend(vaultdb::query(&conn, &q, &filters, limit));
    }
    info!("{} candidates returned.", candidates.len());
    
    debug!("{:?}", candidates);
    let ss: samplesheet::SampleSheet = candidates.into_keys().collect::<Vec<models::Sample>>().into();
    if let Some(targetdir) = extract {
        ss.extract_fastqs(&conn, &targetdir)?;
    }
    if let Some(targetfile) = samplesheet {
        let mut f = File::create(targetfile)?;
        f.write_all(ss.write_csv("\t", &Vec::<&str>::new()).as_bytes())?;
    }
    Ok(())
}

fn import(conn: PgConnection, extract: Option<PathBuf>, samplesheet: Option<PathBuf>, overrides: Option<String>, xlsx: PathBuf) -> Result<()> {

    let ss = match crate::samplesheet::SampleSheet::from_xlsx(xlsx.to_str().unwrap(), &conn) {
        Ok(s) => s,
        Err(e) => { error!("Could not parse samplesheet: {}", e); panic!("Could not parse samplesheet!"); },
    };

    // parse comma-separated overrides string into string vector
    let overrides = overrides.map(|s| { 
        s.split(',').map(|p| p.to_string()).collect::<Vec<String>>()
    }).unwrap_or_default();


    if let Some(samplesheet) = &samplesheet {
        let mut f = File::create(samplesheet)?;
        info!("Writing sample sheet to {}...", samplesheet.display());
        f.write_all(ss.write_csv("\t", &overrides.iter().map(|s| s.as_ref()).collect::<Vec<&str>>()).as_bytes())?;
    }

    if let Some(extract) = &extract {
        info!("Extracting FASTQs of {} samples, please wait...", ss.entries.len());
        ss.extract_fastqs(&conn, extract)?;
        info!("Done.");
    }

    if extract.is_none() && samplesheet.is_none() {
        warn!("Importing doesn't do anything if you don't specify what to do afterwards. Please use --samplesheet or --extract or both.");
    }
    Ok(())
}

fn update(conn: PgConnection, rundir: PathBuf, celldir: PathBuf) -> Result<()> {
    vaultdb::flush(&conn);
    vaultdb::update(&conn, &rundir, &celldir)
}

fn main() -> Result<()> {
    let config = config::Opt::from_args();

    // set up logging
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // set up global thread pool
    rayon::ThreadPoolBuilder::new()
        .num_threads(config.threads)
        .build_global()?;

    let db = vaultdb::establish_connection(&config.connstr);

    match config.cmd {
        
        config::Command::Query {
            query: user_query,
            filter, 
            limit,
            extract, 
            samplesheet} => {
                query(db, user_query, filter, limit, extract, samplesheet)

        }

        config::Command::Import { extract, samplesheet, overrides, xlsx } => {
            import(db, extract, samplesheet, overrides, xlsx)
        }

        config::Command::Update { rundir, celldir } => {
            update(db, rundir, celldir)
        }
        
        config::Command::Web => {
            let _rocket = web::rocket();
            Ok(())
        }
    }
}
