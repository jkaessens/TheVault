use clap::arg_enum;
use std::path::PathBuf;
use structopt::StructOpt;

arg_enum! {
    #[derive(Debug)]
    pub enum OutputType {
        CSV,
        TSV,
        Fastq
    }
}

#[derive(StructOpt, Debug)]
pub enum Command {
    /// Query the Vault database
    Query {
        /// Type of output
        #[structopt(possible_values=&OutputType::variants(), default_value="TSV", case_insensitive=true, short, long)]
        output: OutputType,

        /// A full-text search string
        query: String,
    },

    /// Update the database
    Update {
        /// Force full refresh instead of picking up where it left last time
        #[structopt(long)]
        force: bool,

        /// Config file location, tries to locate sequencing runs from here
        #[structopt(default_value = "/mnt/ngs/01-Rohdaten", long, parse(from_os_str))]
        rundir: PathBuf,
    },

    /// Re-creates an empty database
    Initialize,
}

#[derive(StructOpt, Debug)]
pub struct Opt {
    /// DB connection URI
    #[structopt(default_value = "postgresql://postgres:password@localhost/vault", long)]
    pub connstr: String,

    /// Number of threads to use (default: all cores)
    #[structopt(default_value = "0", long, short)]
    pub threads: usize,

    #[structopt(subcommand)]
    pub cmd: Command,
}
