use crate::models;

#[derive(Debug, Default, PartialEq)]
pub struct Sample {
    pub name: String,
    pub dna_nr: String,
    pub project: String,
    /// fastq file map where each key is the primer set name and the value a list of
    /// .fastq.gz files, relative to the run root
    pub files: Vec<String>,
    pub lims_id: i64,
    pub primer_set: String,
    pub cells: i32,
}

pub(crate) fn normalize_dna_nr(dnanr: &str) -> String {
    
    let parts: Vec<&str> = dnanr.split("-").collect();
    if parts.len() != 2 {
        return dnanr.to_string();
    }
    format!(
        "{:02}-{:05}",
        parts[0].parse::<u32>().unwrap(),
        parts[1].parse::<u32>().unwrap()
    )
}

impl Sample {
    pub fn to_schema_sample(&self, run: &str) -> models::NewSample {
        models::NewSample {
            name: self.name.clone(),
            cells: if self.cells > 0 { Some(self.cells) } else { None },
            dna_nr: normalize_dna_nr(&self.dna_nr),
            lims_id: if self.lims_id > 0 { Some(self.lims_id) } else { None },
            primer_set: if self.primer_set.is_empty() { None} else { Some(self.primer_set.clone()) },
            project: self.project.clone(),
            run: run.to_string(),
        }
    }

}