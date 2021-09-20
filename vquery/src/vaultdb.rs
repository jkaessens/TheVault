use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};


use diesel::pg::PgConnection;
use diesel::prelude::*;

use diesel::sql_types::Text;
use rayon::prelude::*;
use rocket_sync_db_pools::database;

use walkdir::WalkDir;

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
            let samples = &r.samples;
            // Run is still a bit special and needs conversion
            let new_run = r.to_schema_run();
            debug!("Add run {}", &r.name);
            diesel::insert_into(crate::schema::run::table)
                .values(&new_run)
                .execute(conn).expect("Could not insert run");

            let schema_samples: Vec<models::NewSample> = samples.iter().map(|old_s| old_s.to_schema_sample(&r.name)).collect();
            let sample_ids: Vec<i32> = diesel::insert_into(crate::schema::sample::table)
                .values(schema_samples)
                .returning(crate::schema::sample::id)
                .get_results(conn)
                .expect("Could not insert samples");

            for (sample_idx, sample_id) in sample_ids.into_iter().enumerate() {
                let fastqs: Vec<models::Fastq> = samples[sample_idx].files.iter().map(|f| models::Fastq {filename: f.to_string(), sample_id }).collect();
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