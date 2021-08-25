mod config;
mod database;
mod run;
mod runwalker;
mod sample;

use std::error::Error;

use crate::config::OutputType;
use crate::database::Database;
use crate::sample::Sample;
use structopt::StructOpt;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn dump_csv(samples: Vec<(String, Sample)>, sep: &str) {
    for s in samples.into_iter() {
        let fastqs = s.1.files.join(" ");
        println!("{}", vec![s.0, s.1.name, fastqs].join(sep));
    }
}

fn main() -> Result<()> {
    let config = config::Opt::from_args();

    // set up global thread pool
    rayon::ThreadPoolBuilder::new()
        .num_threads(config.threads)
        .build_global()?;

    match config.cmd {
        config::Command::Query { query, output } => {
            let mut db = Database::new(&config.connstr, false)?;
            let candidates = db.find_samples(&query)?;
            match output {
                OutputType::CSV => dump_csv(candidates, ","),
                OutputType::TSV => dump_csv(candidates, "\t"),
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
