use std::error::Error;
use std::path::{Path, PathBuf};

use time::Date;

use crate::run::Run;
use crate::runwalker::Walker;
use crate::sample::Sample;

use rayon::prelude::*;

struct Statements {
    delete_run: postgres::Statement,
    insert_run: postgres::Statement,
    insert_sample: postgres::Statement,
    insert_fastq: postgres::Statement,
}

pub struct Database {
    client: postgres::Client,
}

impl Database {
    pub fn new(connstr: &str, initialize: bool) -> Result<Self, Box<dyn Error>> {
        let mut client = postgres::Client::connect(connstr, postgres::NoTls)?;

        if initialize {
            let drop_sql = include_str!("../../db/db-drop.sql");
            let init_sql = include_str!("../../db/db-initialize.sql");
            client.batch_execute(drop_sql)?;
            client.batch_execute(init_sql)?;
        }

        let db = Database { client };

        Ok(db)
    }

    pub fn get_sample(&mut self, sample_id: i32) -> Result<(String, Sample), Box<dyn Error>> {
        let samplerow = self.client.query_one(
            "SELECT id,run,name,project,dna_nr FROM sample WHERE sample.id=$1",
            &[&sample_id],
        )?;

        let fastqrows = self.client.query(
            "SELECT filename FROM fastq WHERE sample_id=$1",
            &[&sample_id],
        )?;
        let fastqs: Vec<String> = fastqrows.into_iter().map(|r| r.get("filename")).collect();
        Ok((
            samplerow.get("run"),
            Sample {
                name: samplerow.get("name"),
                project: samplerow.get("project"),
                dna_nr: samplerow.get("dna_nr"),
                files: fastqs,
            },
        ))
    }

    pub fn get_run(&mut self, name: &str, fetch_samples: bool) -> Result<Run, Box<dyn Error>> {
        let row = self.client.query_one(
            "SELECT date,assay,chemistry,description,investigator,path FROM run WHERE name=$1",
            &[&name.to_string()],
        )?;

        let pbs: String = row.get("path");
        let mut run = Run {
            name: name.to_string(),
            date: row.get("date"),
            assay: row.get("assay"),
            chemistry: row.get("chemistry"),
            description: row.get("description"),
            investigator: row.get("investigator"),
            path: PathBuf::from(pbs),
            samples: Vec::new(),
        };

        if fetch_samples {
            let ids = self
                .client
                .query(
                    "SELECT id FROM sample WHERE run_name=$1",
                    &[&name.to_string()],
                )?
                .into_iter()
                .map(|row| row.get::<&str, i32>("id"));

            run.samples = ids
                .filter_map(|id| self.get_sample(id).ok())
                .map(|(_, sample)| sample)
                .collect();
        }

        Ok(run)
    }

    pub fn find_samples(&mut self, query: &str) -> Result<Vec<(String, Sample)>, Box<dyn Error>> {
        let rows = self.client.query(
            "SELECT DISTINCT sample_id FROM fastq WHERE LOWER(filename) like CONCAT('%', $1::text, '%')",
            &[&query.to_lowercase()],
        )?;

        let mut samples: Vec<(String, Sample)> = Vec::new();

        for row in rows.into_iter() {
            let sample_id: i32 = row.get("sample_id");
            samples.push(self.get_sample(sample_id)?);
        }

        Ok(samples)
    }

    fn get_last_run_date(&mut self) -> Result<Option<Date>, Box<dyn Error>> {
        match self
            .client
            .query_opt("SELECT date FROM run ORDER BY date ASC LIMIT 1", &[])?
        {
            Some(row) => {
                let t: Date = row.get("date");
                Ok(Some(t))
            }
            None => Ok(None),
        }
    }

    /// Update or inserts a run into the datebase
    ///
    /// Returns true if a new run has been inserted, false if an existing one has been updated, or an Error
    fn update_or_insert_run(&mut self, r: Run) -> Result<bool, Box<dyn Error>> {
        let statements = Statements {
        delete_run: self.client.prepare("DELETE FROM run WHERE name=$1")?,
        insert_run: self.client.prepare("INSERT INTO run (name, date, assay, chemistry, description, investigator, path) VALUES ($1,$2,$3,$4,$5,$6,$7)")?,
        insert_sample: self.client.prepare("INSERT INTO sample (run, name, dna_nr, project) VALUES ($1, $2, $3, $4) RETURNING id")?,
        insert_fastq: self.client.prepare("INSERT INTO fastq (filename, sample_id, primer_set, lane, r) VALUES ($1,$2,$3,$4,$5)")?,
    };

        // drop the run. It's easier to
        let rows = self.client.execute(&statements.delete_run, &[&r.name])?;
        /*
        delete_run: client.prepare("DELETE FROM run WHERE name=$1")?,
        insert_run: client.prepare("INSERT INTO run (name, `date`, assay, chemistry, description, investigator, path,) VALUES ($1,$2,$3,$4,$5,$6,$7)")?,
        insert_sample: client.prepare("INSERT INTO sample (run, name, dna_nr) VALUES (run, name, dna_nr) RETURNING id")?,
        insert_fastq: client.prepare("INSERT INTO fastq (filename, sample_id, primer_set, lane, r) VALUES ($1,$2,$3,$4,$5)")?,
        */
        self.client.execute(
            &statements.insert_run,
            &[
                &r.name,
                &r.date,
                &r.assay,
                &r.chemistry,
                &r.description,
                &r.investigator,
                &r.path.display().to_string(),
            ],
        )?;

        for s in r.samples.into_iter() {
            //println!("add sample {}/{}", r.name, s.name);
            let row = self.client.query_one(
                &statements.insert_sample,
                &[&r.name, &s.name, &s.dna_nr, &s.project],
            )?;
            let id: i32 = row.get::<usize, i32>(0);
            for f in s.files {
                self.client
                    .execute(&statements.insert_fastq, &[&f, &id, &"", &0, &0])?;
            }
        }
        Ok(rows == 0)
    }

    pub fn update(&mut self, path: &Path) -> Result<(), Box<dyn Error>> {
        let latest = self.get_last_run_date()?;
        match &latest {
            None => {
                println!("No run in database, starting from scratch.");
            }
            Some(d) => {
                println!("Latest run in database is from {}, starting there.", d);
            }
        }

        let w = Walker::new(path);
        info!(
            "Starting run discovery using {} threads",
            rayon::current_num_threads()
        );

        let paths: Vec<String> = w.run(&None)?;
        let mut runs: Vec<Run> = vec![];
        runs.par_extend(
            paths
                .into_par_iter()
                .filter_map(|path| Run::from_path(&PathBuf::from(path)).ok()),
        );

        info!("Populating database");
        // feed into database
        for r in runs.into_iter() {
            self.update_or_insert_run(r)?;
        }
        Ok(())
    }
}
