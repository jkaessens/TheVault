mod config;
mod database;
mod runwalker;
mod run;
mod sample;

use std::error::Error;

use structopt::StructOpt;
use database::Database;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn main() -> Result<()> {
    let config = config::Opt::from_args();

    match config.cmd {
        config::Command::Query { query: _query } => {
            unimplemented!()
        },
        config::Command::Update {force, rundir} => {
            let mut db = Database::new(&config.connstr, force)?;
            db.update(&rundir)?;
        },
        config::Command::Initialize => {
            let mut db = Database::new(&config.connstr, true)?;
            db.initialize()?;
        }
    }

    Ok(())
}
