use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
pub enum Command {
    /// Query the Vault database
    Query {
        /// A full-text search string
        query: String,
    },

    /// Update the database
    Update {
        /// Force full refresh instead of picking up where it left last time
        #[structopt(long)]
        force: bool,

        /// Config file location, tries to locate sequencing runs from here
        #[structopt(default_value="/mnt/ngs/01-Rohdaten", long, parse(from_os_str))]
        rundir: PathBuf,

    },

    /// Re-creates an empty database
    Initialize,
}

#[derive(StructOpt, Debug)]
pub struct Opt {

    /// DB connection URI
    #[structopt(default_value="postgresql://postgres:password@localhost/vault", long)]
    pub connstr: String,

    #[structopt(subcommand)]
    pub cmd: Command
}

