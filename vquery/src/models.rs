use std::collections::HashMap;

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

#[derive(Queryable,QueryableByName,Debug,Serialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[table_name = "sample"]
pub struct Sample {
    pub run: String,
    pub name: String,
    pub dna_nr: String,
    pub project: String,
    pub lims_id: Option<i64>,
    pub primer_set: Option<String>,
    pub id: i32,
    pub cells: Option<i32>,
}

#[derive(Insertable,Debug,Serialize)]
#[table_name="sample"]
pub struct NewSample {
    pub run: String,
    pub name: String,
    pub dna_nr: String,
    pub project: String,
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

impl Sample {
    pub fn to_model(&self) -> crate::sample::Sample {
        let mut s = crate::sample::Sample {
            cells: self.cells.unwrap_or(0),
            dna_nr: self.dna_nr.clone(),
            files: Vec::new(),
            extra: HashMap::new(),
            lims_id: self.lims_id.unwrap_or(0),
            name: self.name.clone(),
            primer_set: self.primer_set.as_ref().unwrap_or(&String::from("")).clone(),
            project: self.project.clone()
        };
        s.extra.insert("run".to_string(), self.run.clone());
        s
    }
}