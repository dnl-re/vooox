use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

const MAX_ENTRIES: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub text: String,
    pub timestamp: String, // RFC 3339
    pub model: String,
    pub language: String,
}

pub struct History {
    entries: VecDeque<HistoryEntry>,
    path: PathBuf,
}

fn history_path() -> PathBuf {
    super::paths::data_dir().join("history.jsonl")
}

impl History {
    pub fn load() -> Self {
        let path = history_path();
        let entries = Self::read_from(&path);
        History { entries, path }
    }

    fn read_from(path: &PathBuf) -> VecDeque<HistoryEntry> {
        let Ok(text) = fs::read_to_string(path) else {
            return VecDeque::new();
        };
        let mut entries: VecDeque<HistoryEntry> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        while entries.len() > MAX_ENTRIES {
            entries.pop_front();
        }
        entries
    }

    pub fn push(&mut self, entry: HistoryEntry) {
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(entry.clone());
        self.append_line(&entry);
    }

    fn append_line(&self, entry: &HistoryEntry) {
        ensure_history_dir_exists(&self.path);
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.path) {
            if let Ok(line) = serde_json::to_string(entry) {
                let _ = writeln!(f, "{line}");
            }
        }
    }

    pub fn remove_by_timestamp(&mut self, timestamp: &str) {
        self.entries.retain(|e| e.timestamp != timestamp);
        self.rewrite();
    }

    fn rewrite(&self) {
        ensure_history_dir_exists(&self.path);
        let Ok(mut f) = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)
        else {
            return;
        };
        for entry in &self.entries {
            if let Ok(line) = serde_json::to_string(entry) {
                let _ = writeln!(f, "{line}");
            }
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.entries.iter()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

fn ensure_history_dir_exists(path: &PathBuf) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
}

pub fn now_rfc3339() -> String {
    let total_seconds = seconds_since_unix_epoch();
    let (year, month, day) = approximate_calendar_date(total_seconds);
    let (h, m, s) = time_of_day(total_seconds);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

fn seconds_since_unix_epoch() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn time_of_day(total_seconds: u64) -> (u64, u64, u64) {
    let s = total_seconds % 60;
    let m = (total_seconds / 60) % 60;
    let h = (total_seconds / 3600) % 24;
    (h, m, s)
}

fn approximate_calendar_date(total_seconds: u64) -> (u64, u64, u64) {
    // simple approximation without chrono dependency
    let days = total_seconds / 86400;
    let year = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = day_of_year / 30 + 1;
    let day = day_of_year % 30 + 1;
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_data<F: FnOnce()>(f: F) {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        env::set_var("XDG_DATA_HOME", tmp.path());
        f();
        env::remove_var("XDG_DATA_HOME");
    }

    fn entry(text: &str) -> HistoryEntry {
        HistoryEntry {
            text: text.into(),
            timestamp: "2024-01-01T00:00:00Z".into(),
            model: "small".into(),
            language: "de".into(),
        }
    }

    #[test]
    fn push_and_retrieve() {
        with_temp_data(|| {
            let mut h = History::load();
            h.push(entry("Hallo Welt"));
            assert_eq!(h.len(), 1);
            assert_eq!(h.entries().next().unwrap().text, "Hallo Welt");
        });
    }

    #[test]
    fn max_entries_enforced() {
        with_temp_data(|| {
            let mut h = History::load();
            for i in 0..55usize {
                h.push(entry(&format!("text {i}")));
            }
            assert_eq!(h.len(), MAX_ENTRIES);
            // oldest entries were dropped, last one survives
            assert_eq!(h.entries().last().unwrap().text, "text 54");
        });
    }

    #[test]
    fn persists_to_jsonl() {
        with_temp_data(|| {
            {
                let mut h = History::load();
                h.push(entry("first"));
                h.push(entry("second"));
            }
            let h2 = History::load();
            assert_eq!(h2.len(), 2);
        });
    }
}
