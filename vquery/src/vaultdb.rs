use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};


use diesel::pg::PgConnection;
use diesel::prelude::*;

use diesel::sql_types::Text;
use rayon::prelude::*;
use rocket_sync_db_pools::database;

use walkdir::WalkDir;

use crate::sample::{is_dna_nr, normalize_dna_nr};
use crate::{models, run};


#[database("vault")]
pub(crate) struct VaultDatabase(diesel::PgConnection);

pub fn establish_connection(url: &str) -> PgConnection {
    PgConnection::establish(url).expect("Error connecting to database")
}

pub fn flush(conn: &PgConnection)  {
    if let Err(e) = conn.transaction::<_, diesel::result::Error, _>(|| {
        diesel::delete(crate::schema::fastq::table).execute(conn)?;
        diesel::delete(crate::schema::sample::table).execute(conn)?;
        diesel::delete(crate::schema::run::table).execute(conn)?;
        Ok(())
    }) {
        error!("Could not flush db: {}", e);
    }
}

pub fn update(conn: &PgConnection, rundir: &Path, celldir: &Path) -> Result<(), Box<dyn Error>> {
    info!(
        "Starting run discovery using {} threads",
        rayon::current_num_threads()
    );

    // discover entries on file system
    let walker = WalkDir::new(rundir).follow_links(true).max_depth(3).into_iter();
    let mut paths: Vec<String> = Vec::new();
    for entry in walker {
        let entry = entry.unwrap();
        if entry.depth() == 3 {
            paths.push(entry.path().to_string_lossy().to_string());
        }
    }

    // try to make actual `Run`s of it
    let mut runs: Vec<run::Run> = vec![];
    runs.par_extend(
        paths
            .into_par_iter()
            .filter_map(|path| run::Run::from_path(&PathBuf::from(path), celldir).ok()),
    );

    info!("Populating database with {} runs", runs.len());
    // feed into database
    conn.transaction::<_, diesel::result::Error, _>(|| {
        for r in runs.into_iter() {
            let new_run = r.to_schema_run();
            let mut samples = r.samples;
            // Run is still a bit special and needs conversion
            
            debug!("Add run {}", &r.name);
            let run_query = diesel::insert_into(crate::schema::run::table)
                .values(&new_run);

            run_query
                .execute(conn).expect("Could not insert run");
            
            let sample_models = samples.iter_mut().map(|(a,_)| {a.run = new_run.name.clone(); &*a}).collect::<Vec<_>>();

            let sample_ids_query = diesel::insert_into(crate::schema::sample::table)
                .values(sample_models)
                .returning(crate::schema::sample::id);
 
            let sample_ids: Vec<i32> = sample_ids_query.get_results(conn)
                .expect("Could not insert samples");

            for (sample_idx, sample_id) in sample_ids.into_iter().enumerate() {
                let fastqs: Vec<models::Fastq> = samples[sample_idx].1.iter().map(|f| models::Fastq {filename: f.to_string(), sample_id }).collect();
                diesel::insert_into(crate::schema::fastq::table)
                    .values(fastqs)
                    .execute(conn)
                    .expect("Could not insert fastqs");
            }
            
        }
        Ok(())
    })?;

    Ok(())
}

pub fn query(conn: &PgConnection, needle: &str, filters: &HashMap<String,String>, limit: Option<usize>) -> HashMap<models::Sample, Vec<String>> {
    // get sample ids of samples where the query string matches a fastq filename

    let mut filter_sql = String::from("");
    for f in filters.keys() {
        match f.as_ref() {
            "cells<" | "cells>" | "cells" | "lims_id<" | "lims_id>" | "lims_id" => {
                filter_sql.push_str(&format!(
                    " AND {}={}",
                    f,
                    filters.get(f).unwrap()
                ));
            },
            "run" | "name" | "dna_nr" | "project" | "primer_set" | "filename" => {
                 filter_sql.push_str(&format!(
                    " AND {} ILIKE '{}'",
                    f,
                    filters.get(f).unwrap()
                ));
            },
            _ => {
                warn!("Ignoring unsupported filter: {}", f);
            }

        }
    }
    
    if let Some(count) = limit {
        filter_sql.push_str(&format!(" LIMIT {}", count));
    }

    let statement =
        format!("SELECT sample.*,fastq.* FROM sample INNER JOIN fastq ON sample.id=fastq.sample_id AND sample.id in (SELECT DISTINCT sample.id FROM sample INNER JOIN fastq ON sample.id=fastq.sample_id WHERE fastq.filename ILIKE $1 {})", filter_sql);
    debug!("Q: {}", statement);
    let results: Vec<(models::Sample,models::Fastq)> = diesel::sql_query(&statement)
        .bind::<Text,_>(needle)
        .load(conn)
        .expect("Couldn't retrieve results");

    let mut result: HashMap<models::Sample, Vec<String>> = HashMap::new();
    for (sample, fastq) in results.into_iter() {
        if let Some(v ) = result.get_mut(&sample) {
            v.push(fastq.filename);
        } else {
            result.insert(sample, vec![fastq.filename]);
        }
    }

    result
}


pub enum MatchStatus {
    None(String),
    One(models::Sample),
    Multiple(Vec<models::Sample>),
}

pub fn match_samples(db: &PgConnection, lims_id: Option<i64>, dna_nr: Option<String>, primer_set: Option<String>, name: Option<String>, run: String) -> Result<MatchStatus, Box<dyn std::error::Error>> {
    use crate::schema::sample;
    let candidates: Vec<models::Sample> = sample::table.filter(sample::run.eq(&run)).load(db)?;
    if candidates.is_empty() {
        return Ok(MatchStatus::None(format!("No samples in specified run {}", run)));
    }

    debug!("match_samples: lims_id {:?} dna_nr {:?} primer_set {:?} name: {:?} run {}, run candidates: {}", lims_id, dna_nr, primer_set, name, run, candidates.len());
    // filter by LIMS ID
    let candidates = if let Some(lims_id) = lims_id {
        candidates.into_iter().filter(|s| s.lims_id == Some(lims_id)).collect()
    } else {
        candidates
    };
    
    if candidates.is_empty() {
        return Err(Box::from("No candidates left after LIMS filter"));
    }

    // filter by DNA nr
    let candidates = if let Some(dna_nr) = dna_nr {
        if is_dna_nr(&dna_nr) {
            candidates.into_iter().filter(|s| s.dna_nr == normalize_dna_nr(&dna_nr) ).collect()
        } else {
            candidates
        }
    } else {
        candidates
    };
    if candidates.is_empty() {
        return Ok(MatchStatus::None(String::from("Sample has passed LIMS filter but not dna_nr filter")));
    }

    // filter by primer set (DB contains short version ("FR1") whereas sample sheets/queries often contain the full name "IGH-FR1" or so)
    let candidates = if let Some(primer_set) = primer_set {
        
        candidates.into_iter()
                .filter(|s| if let Some(s_primer_set) = &s.primer_set { primer_set.contains(s_primer_set) } else { false } )
                .collect()
    } else {
        candidates
    };
    if candidates.is_empty() {
        return Ok(MatchStatus::None(String::from("Candidates passed LIMS and DNA filter but not primer_set filter")));
    }
    // filter by name
    let mut candidates = if let Some(name) = name {
        candidates.into_iter().filter(|s| s.name.contains(&name) || name.contains(&s.name)).collect()
    } else {
        candidates
    };

    match candidates.len() {
        0 => Ok(MatchStatus::None(String::from("Candidates passed LIMS, DNA and primer_set filters but not name filter"))),
        1 => Ok(MatchStatus::One(candidates.remove(0))),
        _ => Ok(MatchStatus::Multiple(candidates))
    }
}