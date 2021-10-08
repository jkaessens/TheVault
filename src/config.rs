
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
pub enum Command {
    /// Query the Vault database
    Query {
        /// Extract fastqs
        #[structopt(short,long, parse(from_os_str))]
        extract: Option<PathBuf>,

        /// Create samplesheet from results. Format depends on filename (.xlsx, .tsv)
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

    /// Import a samplesheet and match samples against the database
    Import {
        /// Extract fastqs
        #[structopt(short,long, parse(from_os_str))]
        extract: Option<PathBuf>,

        /// Create samplesheet from results. Format depends on filename (.xlsx, .tsv)
        #[structopt(short,long)]
        samplesheet: Option<PathBuf>,

        /// Override DB entries with these samplesheet columns (comma-separated)
        #[structopt(long)]
        overrides: Option<String>,

        xlsx: PathBuf,
    },

    /// Update the database
    Update {
        /// Root folder for sequencing runs
        #[structopt(default_value = "/mnt/ngs/01-Rohdaten", long, parse(from_os_str))]
        rundir: PathBuf,

        /// Root folder for Cellsheet/spikeINBC lookup
        #[structopt(default_value = "/mnt/L/05-Molekulargenetik/09-NGS/01-Markerscreening", long, parse(from_os_str))]
        celldir: PathBuf,
    },
    /// Start the Rocket handler
    Web,
}

#[derive(StructOpt, Debug)]
pub struct Opt {
    /// DB connection URI
    #[structopt(default_value = "postgresql://vaultuser:_@vault.med2.uni-kiel.local/vault", long)]
    pub connstr: String,

    /// Number of threads to use (default: all cores)
    #[structopt(default_value = "0", long, short)]
    pub threads: usize,

    #[structopt(subcommand)]
    pub cmd: Command,
}
