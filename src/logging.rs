//! File logging.
//!
//! The release build has no console window, so all diagnostics go to a log file in the
//! local data folder (on Windows `%LOCALAPPDATA%\mdviewer\logs`). A logging failure must
//! never stop the viewer:
//! every I/O error in this module is ignored, and the viewer then runs without a log.

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use log::{Level, LevelFilter, Log, Metadata, Record};

/// Environment variable that sets the log level: `off`, `error`, `warn`, `info`, `debug` or `trace`.
pub const LEVEL_ENV_VAR: &str = "MDVIEWER_LOG";

/// Log level when [`LEVEL_ENV_VAR`] is not set or not valid.
pub const DEFAULT_LEVEL: LevelFilter = LevelFilter::Info;

/// When the log file is larger than this at start, it is moved to the old log file.
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// The name of the log file in the log folder.
const LOG_FILE_NAME: &str = "mdviewer.log";

/// Starts file logging and records panics in the log.
///
/// Returns the path of the log file, or `None` when logging is off or not available.
pub fn init() -> Option<PathBuf> {
    let level = parse_level(std::env::var(LEVEL_ENV_VAR).ok().as_deref());
    if level == LevelFilter::Off {
        return None;
    }
    let dir = log_dir(crate::platform::local_data_base())?;
    fs::create_dir_all(&dir).ok()?;
    let path = dir.join(LOG_FILE_NAME);
    rotate_if_needed(&path, MAX_LOG_BYTES);
    let logger = FileLogger::open(&path, level).ok()?;
    // The logger must live until the process ends, so leaking it is intended.
    log::set_logger(Box::leak(Box::new(logger))).ok()?;
    log::set_max_level(level);
    install_panic_hook();
    Some(path)
}

/// Reads a log level from text. Unknown or missing text gives [`DEFAULT_LEVEL`].
pub fn parse_level(value: Option<&str>) -> LevelFilter {
    value
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(DEFAULT_LEVEL)
}

/// Gives the log folder below the local application data folder.
///
/// Returns `None` if the base folder is missing, empty or not absolute, so that the
/// viewer never writes a log into the current working folder.
pub fn log_dir(local_app_data: Option<OsString>) -> Option<PathBuf> {
    crate::paths::app_dir(local_app_data).map(|dir| dir.join("logs"))
}

/// Moves the log file to `<name>.old.log` if it is larger than `max_bytes`.
///
/// An existing old log is replaced. Returns `true` if the file was moved.
pub fn rotate_if_needed(path: &Path, max_bytes: u64) -> bool {
    match fs::metadata(path) {
        Ok(meta) if meta.len() > max_bytes => fs::rename(path, old_log_path(path)).is_ok(),
        _ => false,
    }
}

/// The path of the previous log: `mdviewer.log` → `mdviewer.old.log`.
fn old_log_path(path: &Path) -> PathBuf {
    path.with_extension("old.log")
}

/// Formats a time as ISO 8601 in UTC, with milliseconds: `2026-11-05T12:34:56.789Z`.
pub fn format_timestamp(since_epoch: Duration) -> String {
    const SECONDS_PER_DAY: u64 = 86_400;
    let seconds = since_epoch.as_secs();
    let (year, month, day) = civil_from_days((seconds / SECONDS_PER_DAY) as i64);
    let in_day = seconds % SECONDS_PER_DAY;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        in_day / 3600,
        in_day % 3600 / 60,
        in_day % 60,
        since_epoch.subsec_millis()
    )
}

/// Converts days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar.
///
/// Algorithm: Howard Hinnant, "chrono-Compatible Low-Level Date Algorithms".
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Formats one log line, including the final line break.
pub fn format_line(
    timestamp: &str,
    level: Level,
    pid: u32,
    target: &str,
    message: impl fmt::Display,
) -> String {
    format!(
        "{timestamp} {:<5} [{pid}] {target}: {message}\n",
        level.as_str()
    )
}

/// A logger that appends lines to one file.
///
/// Each line is written with one call, without a buffer, so that the log is complete
/// even if the process stops suddenly.
pub struct FileLogger {
    /// Records below this level are not written.
    level: LevelFilter,
    /// The open log file. The lock keeps lines from different threads apart.
    file: Mutex<File>,
}

impl FileLogger {
    /// Opens (or makes) the log file for appending.
    pub fn open(path: &Path, level: LevelFilter) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            level,
            file: Mutex::new(file),
        })
    }

    /// Locks the file. A panic in another thread does not stop the logging.
    fn file(&self) -> MutexGuard<'_, File> {
        // A panic in another thread must not stop logging, so a poisoned lock is still used.
        self.file
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format_line(
            &format_timestamp(now()),
            record.level(),
            std::process::id(),
            record.target(),
            record.args(),
        );
        let _ = self.file().write_all(line.as_bytes());
    }

    fn flush(&self) {
        let _ = self.file().flush();
    }
}

/// The time since 1970-01-01 (UTC), for the time stamp of a log line.
fn now() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

/// Writes each panic to the log, then calls the previous panic handler.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        log::logger().flush();
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    #[test]
    fn parse_level_accepts_known_levels_in_any_case() {
        assert_eq!(parse_level(Some("debug")), LevelFilter::Debug);
        assert_eq!(parse_level(Some("TRACE")), LevelFilter::Trace);
        assert_eq!(parse_level(Some(" warn ")), LevelFilter::Warn);
        assert_eq!(parse_level(Some("off")), LevelFilter::Off);
    }

    #[test]
    fn parse_level_uses_default_for_missing_or_unknown_text() {
        assert_eq!(parse_level(None), DEFAULT_LEVEL);
        assert_eq!(parse_level(Some("")), DEFAULT_LEVEL);
        assert_eq!(parse_level(Some("verbose")), DEFAULT_LEVEL);
    }

    #[test]
    fn log_dir_is_below_local_app_data() {
        let base = std::env::temp_dir();
        assert_eq!(
            log_dir(Some(base.clone().into_os_string())),
            Some(base.join("mdviewer").join("logs"))
        );
    }

    #[test]
    fn log_dir_rejects_missing_empty_and_relative_base() {
        assert_eq!(log_dir(None), None);
        assert_eq!(log_dir(Some(OsString::new())), None);
        assert_eq!(log_dir(Some(OsString::from("relative\\folder"))), None);
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29)); // leap day in a leap century
        assert_eq!(civil_from_days(20_762), (2026, 11, 5));
        assert_eq!(civil_from_days(47_540), (2100, 2, 28)); // 2100 is not a leap year
        assert_eq!(civil_from_days(47_541), (2100, 3, 1));
    }

    #[test]
    fn format_timestamp_gives_iso_8601_utc_with_milliseconds() {
        assert_eq!(format_timestamp(Duration::ZERO), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            format_timestamp(Duration::from_millis(1_793_882_096_789)),
            "2026-11-05T12:34:56.789Z"
        );
    }

    #[test]
    fn format_line_pads_level_and_ends_with_line_break() {
        assert_eq!(
            format_line(
                "T",
                Level::Info,
                42,
                "mdviewer::app",
                format_args!("hello {}", 1)
            ),
            "T INFO  [42] mdviewer::app: hello 1\n"
        );
        assert_eq!(
            format_line("T", Level::Error, 7, "x", "boom"),
            "T ERROR [7] x: boom\n"
        );
    }

    #[test]
    fn rotate_keeps_small_file() {
        let dir = TempDir::new("rotate-small");
        let path = dir.path().join(LOG_FILE_NAME);
        fs::write(&path, "12345").unwrap();

        assert!(!rotate_if_needed(&path, 5));
        assert_eq!(fs::read_to_string(&path).unwrap(), "12345");
        assert!(!old_log_path(&path).exists());
    }

    #[test]
    fn rotate_moves_large_file_and_replaces_old_log() {
        let dir = TempDir::new("rotate-large");
        let path = dir.path().join(LOG_FILE_NAME);
        let old = old_log_path(&path);
        fs::write(&old, "previous old log").unwrap();
        fs::write(&path, "123456").unwrap();

        assert!(rotate_if_needed(&path, 5));
        assert!(!path.exists());
        assert_eq!(fs::read_to_string(&old).unwrap(), "123456");
        assert_eq!(old.file_name().unwrap(), "mdviewer.old.log");
    }

    #[test]
    fn rotate_ignores_missing_file() {
        let dir = TempDir::new("rotate-missing");
        assert!(!rotate_if_needed(&dir.path().join(LOG_FILE_NAME), 0));
    }

    fn record_at(logger: &FileLogger, level: Level, message: &str) {
        logger.log(
            &Record::builder()
                .level(level)
                .target("test")
                .args(format_args!("{message}"))
                .build(),
        );
    }

    #[test]
    fn file_logger_writes_only_enabled_levels() {
        let dir = TempDir::new("logger-levels");
        let path = dir.path().join(LOG_FILE_NAME);
        let logger = FileLogger::open(&path, LevelFilter::Info).unwrap();

        record_at(&logger, Level::Info, "visible");
        record_at(&logger, Level::Debug, "hidden");
        logger.flush();

        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains(" INFO  ["), "line: {text}");
        assert!(text.contains("] test: visible\n"), "line: {text}");
        assert!(!text.contains("hidden"));
    }

    #[test]
    fn file_logger_appends_to_existing_file() {
        let dir = TempDir::new("logger-append");
        let path = dir.path().join(LOG_FILE_NAME);

        record_at(
            &FileLogger::open(&path, LevelFilter::Info).unwrap(),
            Level::Warn,
            "first",
        );
        record_at(
            &FileLogger::open(&path, LevelFilter::Info).unwrap(),
            Level::Warn,
            "second",
        );

        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("test: first"));
        assert!(lines[1].ends_with("test: second"));
    }
}
