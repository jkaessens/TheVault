use std::error::Error;
use std::collections::HashMap;
use crate::models;

use calamine::{Reader, Xlsx, open_workbook};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

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



// Convert DNA numbers to XX-XXXXX format, will be filled up with zeros if necessary
pub(crate) fn normalize_dna_nr(dnanr: &str) -> String {
    
    let dnanr = if dnanr.starts_with("D-") { &dnanr[2..] } else { dnanr };
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

// only a few conversion routines between schema and model
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

// Check if given string seems to be a valid DNA number (cheap check, not 100%)
pub(crate) fn is_dna_nr(dna_nr: &str) -> bool {
    if dna_nr.len() < 8 {   // XX-XXXXX
        return false;
    }

    let actual_dna_nr = if dna_nr.starts_with("D-") {
        &dna_nr[2..]
    } else {
        dna_nr
    };

    // should be enough, no need to parse for numbers
    actual_dna_nr.len() == 8 && actual_dna_nr.as_bytes()[2] == b'-'

}

// update Sample list with columns from the given XLSX Sample sheet.
// import_str format: "col1[,col2[...,coln]]=filename.xlsx"
// where col1..coln are column headers and filename.xlsx is a 
// sample sheet in XLSX format (no XLS or TSV or CSV supported)
pub fn import_columns(samples: &mut [Sample], filename: &str, cols: &[&str]) -> Result<()> {

    // open Excel workbook
    let mut ss: Xlsx<_> = open_workbook(filename)?;
    let sheetname = ss.sheet_names()[0].clone();
    let sheet = ss.worksheet_range( &sheetname).unwrap()?;
    
    // get first column and make lookup table for column headers
    let rows = sheet.rows();
    let first_row = sheet.rows().next().unwrap();
    let mut colkeys: HashMap<String, usize> = HashMap::new();
    let mut col_dna: Option<usize> = None;
    
    for (idx,val) in first_row.iter().enumerate() {
        let s = val.to_string();
        debug!("Discovered column: {} at {}", &s, idx);
        if s == "DNA nr" {
            col_dna = Some(idx);
        } else if cols.iter().position(|&r| r == s).is_some() {
            colkeys.insert(s, idx);
        }
    }

    let mut update_count = 0;
    let mut row_count = 0;

    // get column indices for important sample lookup clues
    let col_dna = col_dna.expect("Could not find column 'DNA nr' in sample sheet. Exiting.");
    let col_lims_id = first_row.iter().enumerate().find(|(_, v)| v.to_string() == "LIMS ID").map(|(i,_)| i);
    let col_primer_set = first_row.iter().enumerate().find(|(_, v)| v.to_string() == "primer set").map(|(i,_)| i);

    // try to match sample list against sample sheet rows
    for row in rows.skip(1) {
        row_count += 1;
        let ss_dna_nr = row[col_dna].to_string();
        
        // 1. Sieve by DNA number
        // ss DNA numbers are usually more verbose, i.e. "D-12-34567" instead of "12-34567"
        // so try to find the known DNA number in the ss DNA number.
        let candidates: Vec<&mut Sample> = samples.iter_mut().filter(|s| {
            ss_dna_nr.find(&s.dna_nr).is_some() && (is_dna_nr(&s.dna_nr) && is_dna_nr(&ss_dna_nr) || (!is_dna_nr(&s.dna_nr) && !is_dna_nr(&ss_dna_nr)))
        }).collect();
     
        // 2. Sieve by LIMS ID if we have one
        let candidates: Vec<&mut Sample> = if let Some(actual_col_lims_id) = col_lims_id {
            let ss_lims_id = row[actual_col_lims_id].to_string().parse::<i64>().unwrap_or(0);
            candidates.into_iter().filter(|s| (ss_lims_id == s.lims_id) || ss_lims_id==0 || s.lims_id == 0 ).collect()
        } else {
            candidates
        };

        // 3. Sieve by primer set if we have one. This one is a bit heuristic, so only use if we have more than 1 candidate
        let candidates = if candidates.len() > 1 {
            if let Some(actual_col_primer_set) = col_primer_set {
                let ss_primer_set = row[actual_col_primer_set].to_string();
                candidates.into_iter().filter(|s| ss_primer_set.find(&s.primer_set).is_some() || s.primer_set.is_empty() ).collect()
            } else {
                candidates
            }
        } else {
            candidates
        };

        // 4. Sieve by name. This should only ever happen with BC and Aqua samples. In this case, the database entry is more verbose than the SS entry
        let mut candidates = if candidates.len() > 1 {
            candidates.into_iter().filter(|s| s.name.find(&row[0].to_string()).is_some() ).collect()
        } else {
            candidates
        };

        // early abort if != 1 candidates left
        if candidates.len() > 1 {
            debug!("still have these candidates: {:?}", candidates);
            warn!("Sample {} in sample sheet is ambiguous with respect to DNA nr, LIMS ID and primer set. Skipping update. Please check for duplicates in sample sheet.", row[0].to_string());
            continue;
        }
        if candidates.is_empty() {
            warn!("No match for sample {} with respect to DNA nr, LIMS ID and primer set. Please adjust filters or fix sample sheet.", row[0].to_string());
            continue;
        }

        // update Sample
        let matched_sample = candidates.remove(0);
        for (col,idx) in colkeys.iter() {
            let idx = idx.clone();
            match col.as_ref() {
                "Sample" | "DNA nr" | "run" => warn!("Cannot update column types 'Sample', 'DNA nr' or 'run'"),
                "cells" => {
                    matched_sample.cells = row[idx].to_string().parse::<i32>().unwrap_or(0);
                },
                "primer set" => matched_sample.primer_set = row[idx].get_string().unwrap().to_string(),
                "project" => matched_sample.project = row[idx].get_string().unwrap().to_string(),
                "LIMS ID" => {
                    matched_sample.lims_id = row[idx].to_string().parse::<i64>().unwrap_or(0);
                },
                c@_ => {
                    matched_sample.extra.insert(c.to_string(), row[idx].get_string().unwrap_or_default().to_string());
                },
            }
        }
        update_count += 1;
    }

    info!("Columns {} have been imported for {} samples (out of {} samples in database query, {} rows in sample sheet)",
        colkeys.into_keys().fold(String::new(), |acc, key| { if acc.is_empty() { key } else { format!("{},{}", acc, key) } }),
        update_count,
        samples.len(),
        row_count);

    Ok(())
}