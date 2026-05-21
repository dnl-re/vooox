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
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".local").join("share")
        });
    base.join("vooox").join("history.jsonl")
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
        let mut v: VecDeque<HistoryEntry> = text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        while v.len() > MAX_ENTRIES {
            v.pop_front();
        }
        v
    }

    pub fn push(&mut self, entry: HistoryEntry) {
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(entry.clone());
        self.append_line(&entry);
    }

    fn append_line(&self, entry: &HistoryEntry) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
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
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)
        {
            for entry in &self.entries {
                if let Ok(line) = serde_json::to_string(entry) {
                    let _ = writeln!(f, "{line}");
                }
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

pub fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // simple ISO-8601 without chrono dependency
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    // days since 1970-01-01 → approx date (good enough for a timestamp label)
    let year = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = day_of_year / 30 + 1;
    let day = day_of_year % 30 + 1;
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
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
