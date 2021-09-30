
/*
#[derive(Debug, Default, PartialEq)]
pub struct Sample {
    pub id: Option<i32>,
    pub name: String,
    pub dna_nr: String,
    pub project: String,
    /// fastq file map where each key is the primer set name and the value a list of
    /// .fastq.gz files, relative to the run root
    pub files: Vec<String>,
    pub lims_id: i64,
    pub primer_set: String,
    pub cells: i32,
    pub extra: HashMap<String,String>,
}
*/


// Convert DNA numbers to XX-XXXXX format, will be filled up with zeros if necessary
pub(crate) fn normalize_dna_nr(dnanr: &str) -> String {
    
    let dnanr = dnanr.strip_prefix("D-").unwrap_or(dnanr);
    let parts: Vec<&str> = dnanr.split('-').collect();
    if parts.len() != 2 {
        return dnanr.to_string();
    }
    format!(
        "{:02}-{:05}",
        parts[0].parse::<u32>().unwrap(),
        parts[1].parse::<u32>().unwrap()
    )
}

// Check if given string seems to be a valid DNA number (cheap check, not 100%)
pub(crate) fn is_dna_nr(dna_nr: &str) -> bool {
    if dna_nr.len() < 8 {   // XX-XXXXX
        return false;
    }

    let actual_dna_nr = dna_nr.strip_prefix("D-").unwrap_or(dna_nr);

    // should be enough, no need to parse for numbers
    actual_dna_nr.len() == 8 && actual_dna_nr.as_bytes()[2] == b'-'

}
