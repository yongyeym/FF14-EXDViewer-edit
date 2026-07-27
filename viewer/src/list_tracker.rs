use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A version-stamped list of item identifiers (sheet names, music paths, etc.).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VersionedList {
    pub version: String,
    pub items: Vec<String>,
}

/// Config directory relative to the executable.
fn config_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("config")))
        .unwrap_or_else(|| PathBuf::from("config"))
}

/// File path for a versioned sheet list with the given version string.
fn sheet_list_path(version: &str) -> PathBuf {
    let safe_ver = version.replace('.', "_");
    config_dir().join(format!("sheet_list_{safe_ver}.json"))
}

/// File path for a versioned music list with the given version string.
fn music_list_path(version: &str) -> PathBuf {
    let safe_ver = version.replace('.', "_");
    config_dir().join(format!("music_list_{safe_ver}.json"))
}

/// Save a versioned list to disk.
pub fn save_list(path: &PathBuf, list: &VersionedList) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(path, &json);
    }
}

/// Load a versioned list from disk.
pub fn load_list(path: &PathBuf) -> Option<VersionedList> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Remove stale list files (keep only the two most recent).
pub fn cleanup_lists(prefix: &str, current_version: &str) {
    let dir = config_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let safe_cur = current_version.replace('.', "_");

    let mut list_files: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) {
                // Extract version from filename: prefix_{version}.json
                let ver = name
                    .strip_prefix(prefix)
                    .and_then(|s| s.strip_suffix(".json"))
                    .unwrap_or("");
                Some((ver.to_string(), e.path()))
            } else {
                None
            }
        })
        .filter(|(ver, _)| *ver != safe_cur)
        .collect();

    // Sort by version (string comparison works for sorted version strings)
    list_files.sort_by(|a, b| b.0.cmp(&a.0));

    // Keep at most 1 old list (current + 1 previous = 2 total)
    while list_files.len() > 1 {
        let (_, path) = list_files.pop().unwrap();
        let _ = std::fs::remove_file(&path);
    }
}

/// Compute the list of items that are present in `current` but not in `previous`.
pub fn compute_new_items(current: &[String], previous: &[String]) -> Vec<String> {
    let prev_set: HashSet<&str> = previous.iter().map(String::as_str).collect();
    current
        .iter()
        .filter(|item| !prev_set.contains(item.as_str()))
        .cloned()
        .collect()
}

/// Result of comparing two lists.
pub enum ComparisonResult {
    /// First run – no previous list existed.
    NoPrevious,
    /// Version matches the stored list – no game update.
    SameVersion,
    /// Version changed – new items found.
    NewItems(Vec<String>),
    /// Error message to display.
    Error(String),
}

/// Load saved lists, compare versions, and return the comparison result.
pub fn compare_lists(
    current_version: &str,
    sheet_list: &VersionedList,
    prev_list: Option<&VersionedList>,
) -> ComparisonResult {
    let Some(prev) = prev_list else {
        return ComparisonResult::NoPrevious;
    };

    if prev.version == current_version {
        return ComparisonResult::SameVersion;
    }

    let new = compute_new_items(&sheet_list.items, &prev.items);
    if new.is_empty() {
        ComparisonResult::SameVersion
    } else {
        ComparisonResult::NewItems(new)
    }
}
