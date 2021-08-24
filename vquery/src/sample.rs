
#[derive(Debug, Default)]
pub struct Sample {
    pub name: String,
    pub dna_nr: String,
    pub project: String,
    /// fastq file map where each key is the primer set name and the value a list of
    /// .fastq.gz files, relative to the run root
    pub files: Vec<String>,
}
