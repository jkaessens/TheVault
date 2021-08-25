use std::error::Error;
use std::path::{Path, PathBuf};
use time::Date;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

use crossbeam_channel::bounded;
use walkdir::{DirEntry, WalkDir};

/// Parses a date string from a run name, that typically starts with "YYMMDD"
pub(crate) fn parse_date(source: &str) -> Result<time::Date> {
    if source.len() < 6 {
        return Err(Box::from("Date string too short"));
    }
    let year = source[0..2].parse::<i32>()? + 2000;
    let month = source[2..4].parse::<u8>()?;
    let day = source[4..6].parse::<u8>()?;
    Ok(Date::try_from_ymd(year, month, day)?)
}

/// Serves as file filter for the directory tree walker.
///
/// # Rules
///
/// 1. Accept directories with one to three components (might by `/year`, `/year/month` or `/year/month/run`)
///    - if a start date is given, date-related directories will be filtered if their parsed date is
///      earlier than `start_date`
/// 2. Accept zip files only on the third level (e.g. `/year/month/run.zip`)
///    - if a start date is given, the date will be parsed from the file name. Will be filtered
///      if the file date is earlier than `start_date`
fn file_filter(entry: &DirEntry, start_date: &Option<time::Date>) -> bool {
    // Path must either be directory or zip file
    if !(entry.file_type().is_dir()) {
        if !entry
            .file_name()
            .to_ascii_lowercase()
            .to_string_lossy()
            .ends_with(".zip")
        {
            return false;
        }
    }

    // if depth is 1 or 2, that is, we are still in the year/month hierarchy,
    // filter by date early on
    if entry.file_type().is_dir() {
        match entry.depth() {
            // always allow root
            0 => {
                return true;
            }
            // "year" part
            1 => {
                if let Some(d) = start_date {
                    let file_year = entry.file_name().to_string_lossy()[0..3]
                        .parse::<i32>()
                        .unwrap();
                    if file_year < d.year() {
                        return false;
                    }
                }
            }
            // "month" part, year is already checked
            // some weird folders also have zip files here
            2 => {
                if let Some(d) = start_date {
                    let file_month = entry.file_name().to_string_lossy()[0..1]
                        .parse::<u8>()
                        .unwrap();
                    if file_month < d.month() {
                        return false;
                    }
                }
            }
            // either run directory or actual zip file
            3 => {
                if let Some(d) = start_date {
                    let file_date = parse_date(&entry.file_name().to_string_lossy());
                    if let Ok(fdate) = file_date {
                        if fdate < *d {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
            }
            _ => {
                return false;
            }
        }
    }

    true
}

/// Handling class for the directory tree walker that would discover runs on a file system
pub struct Walker {
    /// Root directory to start from
    ngsroot: PathBuf,

    /// Receiver channel. Receiver handles will be cloned from this endpoint.
    rx: crossbeam_channel::Receiver<PathBuf>,

    /// Transmitter channel. Will send new discoveries here.
    tx: crossbeam_channel::Sender<PathBuf>,
}

impl Walker {
    /// Creates a new path walker with a given capacity for the output channel
    pub fn new(root: &Path, upper_bound: usize) -> Self {
        let (tx, rx) = bounded(upper_bound);

        Walker {
            ngsroot: root.to_path_buf(),
            rx,
            tx,
        }
    }

    /// Get a new receiver handle to receive newly discovered runs from
    pub fn create_receiver(&mut self) -> crossbeam_channel::Receiver<PathBuf> {
        self.rx.clone()
    }

    /// Start the path walker, optionally with a given start date.
    ///
    /// It will start pushing discovered runs into the receiver channels. `run` consumes itself when
    /// its done, dropping the tx channel, making receivers return from blocking reads when the
    /// channel is finally empty.
    pub fn run(self, start_date: &Option<time::Date>) -> Result<()> {
        let walker = WalkDir::new(self.ngsroot).into_iter();
        for entry in walker.filter_entry(|d| file_filter(d, start_date)) {
            let entry = entry.unwrap();
            if entry.depth() == 3 {
                self.tx.send(entry.path().to_owned())?;
            }
        }

        Ok(())
    }
}
