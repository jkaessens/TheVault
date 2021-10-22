use crate::schema::*;

use serde::Serialize;
use chrono::NaiveDate;

#[derive(Queryable,QueryableByName,Insertable,Debug,Serialize)]
#[table_name="run"]
pub struct Run {
    pub name: String,
    pub date: NaiveDate,
    pub assay: String,
    pub chemistry: String,
    pub description: Option<String>,
    pub investigator: String,
    pub path: String,
}

#[derive(Queryable,QueryableByName,Debug,Serialize, PartialEq, Eq, Hash, PartialOrd, Ord, Clone, Default)]
#[table_name = "sample"]
pub struct Sample {
    pub run: String,
    pub name: String,
    pub dna_nr: Option<String>,
    pub project: Option<String>,
    pub lims_id: Option<i64>,
    pub primer_set: Option<String>,
    pub id: i32,
    pub cells: Option<i32>,
}

#[derive(Insertable,Debug,Serialize,Clone,Default)]
#[table_name="sample"]
pub struct NewSample {
    pub run: String,
    pub name: String,
    pub dna_nr: Option<String>,
    pub project: Option<String>,
    pub lims_id: Option<i64>,
    pub primer_set: Option<String>,
    pub cells: Option<i32>,
}

#[derive(Queryable, QueryableByName, Insertable,Debug,Serialize)]
#[table_name="fastq"]
pub struct Fastq {
    pub filename: String,
    pub sample_id: i32
}

impl NewSample {
    pub fn from_sample(s: &Sample) -> NewSample {
        NewSample {
            run: s.run.clone(),
            name: s.name.clone(),
            dna_nr: s.dna_nr.clone(),
            project: s.project.clone(),
            lims_id: s.lims_id,
            primer_set: s.primer_set.clone(),
            cells: s.cells
        }
    }
}