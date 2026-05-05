use chrono::Local;
use log::{LevelFilter, Log, Metadata, Record, SetLoggerError};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

struct CurrentFile {
    /// "YYYY-MM-DD/HH" — used to detect when to roll to a new file.
    key: String,
    file: File,
}

struct RollingLogger {
    log_dir: PathBuf,
    current: Mutex<Option<CurrentFile>>,
}

impl RollingLogger {
    fn new(log_dir: PathBuf) -> Self {
        Self { log_dir, current: Mutex::new(None) }
    }
}

impl Log for RollingLogger {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let now = Local::now();
        let day  = now.format("%Y-%m-%d").to_string();
        let hour = now.format("%H").to_string();
        let key  = format!("{}/{}", day, hour);

        let mut guard = match self.current.lock() {
            Ok(g)  => g,
            Err(_) => return,
        };

        let need_rotate = guard.as_ref().map_or(true, |c| c.key != key);
        if need_rotate {
            let dir = self.log_dir.join(&day);
            if fs::create_dir_all(&dir).is_ok() {
                let path = dir.join(format!("{}.log", hour));
                if let Ok(file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
                    *guard = Some(CurrentFile { key, file });
                }
            }
        }

        if let Some(ref mut cur) = *guard {
            let _ = writeln!(
                cur.file,
                "[{}] {:5} [{}] {}",
                now.format("%Y-%m-%d %H:%M:%S%.3f"),
                record.level(),
                record.target(),
                record.args()
            );
        }

        #[cfg(debug_assertions)]
        eprintln!(
            "[{}] {:5} [{}] {}",
            now.format("%H:%M:%S%.3f"),
            record.level(),
            record.target(),
            record.args()
        );
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.current.lock() {
            if let Some(ref mut cur) = *guard {
                let _ = cur.file.flush();
            }
        }
    }
}

/// Initialise the global rolling file logger.
/// Cleans up log directories older than 30 days before starting.
pub fn init(log_dir: &Path) -> Result<(), SetLoggerError> {
    let _ = fs::create_dir_all(log_dir);
    cleanup_old_logs(log_dir);

    let logger = Box::new(RollingLogger::new(log_dir.to_path_buf()));
    log::set_boxed_logger(logger)?;
    log::set_max_level(if cfg!(debug_assertions) {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    });
    Ok(())
}

fn cleanup_old_logs(log_dir: &Path) {
    let Some(cutoff) = Local::now()
        .date_naive()
        .checked_sub_days(chrono::Days::new(30))
    else {
        return;
    };

    let Ok(entries) = fs::read_dir(log_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Ok(date) = chrono::NaiveDate::parse_from_str(&name, "%Y-%m-%d") {
            if date <= cutoff {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
}
