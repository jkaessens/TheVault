use std::error::Error;
use std::path::Path;
use std::thread;

use time::Date;

use crate::run::Run;
use crate::runwalker::Walker;
use crate::sample::Sample;

const NUM_WORKER_THREADS: usize = 24;

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

    pub fn extract_fastqs(&mut self, samples: &[(String, Sample)]) -> Result<(), Box<dyn Error>> {
        for (run, s) in samples {
            // samples only contain fastq paths relative to the run root. Get the run root.
            let row = self
                .client
                .query_one("SELECT path FROM run WHERE name=$1", &[&run])?;
        }
        Ok(())
    }

    pub fn find_samples(&mut self, query: &str) -> Result<Vec<(String, Sample)>, Box<dyn Error>> {
        let rows = self.client.query(
            "SELECT DISTINCT sample_id FROM fastq WHERE filename like CONCAT('%', $1::text, '%')",
            &[&query],
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

        let mut w = Walker::new(path, NUM_WORKER_THREADS * 2);

        // set up worker threads that take care of the discovered runs
        let mut threads: Vec<thread::JoinHandle<Vec<Run>>> = Vec::new();
        for _i in 0..NUM_WORKER_THREADS {
            let rx = w.create_receiver();
            threads.push(thread::spawn(move || {
                let mut runs: Vec<Run> = Vec::new();
                while let Ok(p) = rx.recv() {
                    //println!("{:?} Picking up {:?}", std::thread::current().id(), &p);
                    match Run::from_path(&p) {
                        Ok(r) => {
                            runs.push(r);
                        }
                        Err(e) => {
                            eprintln!("Could not create run from {}: {}", p.display(), e)
                        }
                    }
                }
                runs
            }));
        }

        // start filling the thread queues
        w.run(&latest)?;

        // feed
        for t in threads.into_iter() {
            let runs = t.join().expect("Couldn't join with runner thread!");
            for r in runs.into_iter() {
                self.update_or_insert_run(r)?;
            }
        }
        Ok(())
    }
}
