
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
pub enum Command {
    /// Query the Vault database
    Query {
        /// Extract fastqs
        #[structopt(short,long, parse(from_os_str))]
        extract: Option<PathBuf>,

        /// Create samplesheet from results
        #[structopt(short,long)]
        samplesheet: Option<PathBuf>,

        /// Filter
        #[structopt(long)]
        filter: Vec<String>,

        /// Limit result count
        #[structopt(long)]
        limit: Option<usize>,

        /// A full-text search string
        query: String,
    },

    /// Update the database
    Update {
        /// Force full refresh instead of picking up where it left last time
        #[structopt(long)]
        force: bool,

        /// Root folder for sequencing runs
        #[structopt(default_value = "/mnt/ngs/01-Rohdaten", long, parse(from_os_str))]
        rundir: PathBuf,

        /// Root folder for Cellsheet/spikeINBC lookup
        #[structopt(default_value = "/mnt/kaessens-j/L/05-Molekulargenetik/09-NGS/01-Markerscreening", long, parse(from_os_str))]
        celldir: PathBuf,
    },
    /// Start the Rocket handler
    Web,
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
