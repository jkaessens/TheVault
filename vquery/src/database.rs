use std::collections::HashMap;
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

#[derive(Default)]
pub struct UpdateStats {
    pub runs: usize,
    pub samples: usize,
    pub fastqs: usize,
}

impl Database {
    /// Creates the database connection and optionally fully resets it
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

    /// Retrieves a sample by their id
    pub fn get_sample(&mut self, sample_id: i32) -> Result<(String, Sample), Box<dyn Error>> {
        let samplerow = self.client.query_one(
            "SELECT id,run,name,project,dna_nr,lims_id,primer_set,cells FROM sample WHERE sample.id=$1",
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
                lims_id: samplerow.get("lims_id"),
                primer_set: samplerow.get("primer_set"),
                files: fastqs,
                cells: samplerow.get("cells"),
            },
        ))
    }

    /// Fetches a run from the database, optionally including all samples and their associated
    /// fastq entries.
    /// # Parameters
    /// * name: exact name of the run
    /// * fetch_samples: whether the contained samples should also be fetched (expensive)
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

    fn filter_filters(filter: &Vec<String>) -> HashMap<String, String> {
        let mut hf: HashMap<String, String> = HashMap::new();
        for f in filter {
            let parts: Vec<String> = f.split("=").map(|s| s.to_string().to_lowercase()).collect();
            if parts.len() < 2 || parts.len() > 2 {
                warn!(
                    "Skipping filter '{}'. Invalid syntax. Only KEY=VALUE is allowed",
                    f
                );
                continue;
            }

            hf.insert(parts[0].to_string(), parts[1].to_string());
        }
        hf
    }

    /// find all samples where an associated FASTQ file has a match on the query
    pub fn find_samples(
        &mut self,
        query: &str,
        filter: &Vec<String>,
    ) -> Result<Vec<(String, Sample)>, Box<dyn Error>> {
        // merge filters into SQL query
        let filters = Self::filter_filters(filter);
        let mut filter_sql = String::from("");
        for f in filters.keys() {
            filter_sql.push_str(&format!(
                " AND LOWER(sample.{}) like '{}'",
                f,
                filters.get(f).unwrap()
            ));
        }

        let statement =
            format!("SELECT distinct sample.id as sample_id FROM sample INNER JOIN fastq ON sample.id=fastq.sample_id WHERE LOWER(CONCAT(dna_nr,filename)) like CONCAT('%', $1::text,'%'){}", filter_sql);

        debug!("Q: {}", &statement);
        let rows = self.client.query(&*statement, &[&query.to_lowercase()])?;

        let mut samples: Vec<(String, Sample)> = Vec::new();

        let mut sample_ids: Vec<i32> = rows.into_iter().map(|r| r.get("sample_id")).collect();
        sample_ids.sort();
        sample_ids.dedup();

        for id in sample_ids {
            samples.push(self.get_sample(id)?);
        }
        Ok(samples)
    }

    /// Retrieve the latest known run date
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

    fn normalize_dna_nr(dnanr: &str) -> String {
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
    /// Update or inserts a run into the datebase
    ///
    /// Returns true if a new run has been inserted, false if an existing one has been updated, or an Error
    fn update_or_insert_run<C: postgres::GenericClient>(
        client: &mut C,
        r: Run,
        stats: &mut UpdateStats,
    ) -> Result<(), Box<dyn Error>> {
        let statements = Statements {
            delete_run: client.prepare("DELETE FROM run WHERE name=$1")?,
            insert_run: client.prepare("INSERT INTO run (name, date, assay, chemistry, description, investigator, path) VALUES ($1,$2,$3,$4,$5,$6,$7)")?,
            insert_sample: client.prepare("INSERT INTO sample (run, name, dna_nr, project,lims_id,primer_set,cells) VALUES ($1, $2, $3, $4, $5, $6,$7) RETURNING id")?,
            insert_fastq: client.prepare("INSERT INTO fastq (filename, sample_id) VALUES ($1,$2)")?,
        };

        // drop the run. It's easier to
        client.execute(&statements.delete_run, &[&r.name])?;
        client.execute(
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
        stats.runs += 1;
        stats.samples += r.samples.len();

        for s in r.samples.into_iter() {
            let new_dna_nr = Self::normalize_dna_nr(&s.dna_nr);
            let row = client.query_one(
                &statements.insert_sample,
                &[
                    &r.name,
                    &s.name,
                    &new_dna_nr,
                    &s.project,
                    &s.lims_id,
                    &s.primer_set,
                    &s.cells,
                ],
            )?;
            let id: i32 = row.get::<usize, i32>(0);
            stats.fastqs += s.files.len();
            for f in s.files {
                client.execute(&statements.insert_fastq, &[&f, &id])?;
            }
        }
        Ok(())
    }

    /// Updates the run database starting with the latest run that could be found
    pub fn update(&mut self, rundir: &Path, celldir: &Path) -> Result<(), Box<dyn Error>> {
        let latest = self.get_last_run_date()?;
        match &latest {
            None => {
                info!("No run in database, starting from scratch.");
            }
            Some(d) => {
                info!("Latest run in database is from {}, starting there.", d);
            }
        }

        // run discovery on path
        let w = Walker::new(rundir);
        info!(
            "Starting run discovery using {} threads",
            rayon::current_num_threads()
        );

        let paths: Vec<String> = w.run(&None)?;
        let mut runs: Vec<Run> = vec![];
        runs.par_extend(
            paths
                .into_par_iter()
                .filter_map(|path| 
                    Run::from_path(&PathBuf::from(path), celldir).ok())
                ,
        );

        let mut stats = UpdateStats::default();
        info!("Populating database");
        // feed into database
        let mut transaction = self.client.transaction()?;
        for r in runs.into_iter() {
            Self::update_or_insert_run(&mut transaction, r, &mut stats)?;
        }
        transaction.commit()?;
        info!(
            "Done. {} runs, {} samples and {} fastq files where added to the database.",
            stats.runs, stats.samples, stats.fastqs
        );
        Ok(())
    }
}
