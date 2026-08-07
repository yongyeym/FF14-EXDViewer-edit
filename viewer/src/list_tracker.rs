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

/// 归档目录：exe目录/config/bak/{kind}/
fn bak_dir(kind: &str) -> PathBuf {
    config_dir().join("bak").join(kind)
}

/// 将列表文件移动到存档目录 config/bak/{kind}/{文件名}
fn archive_file(path: &PathBuf, kind: &str) {
    let bak = bak_dir(kind);
    let _ = std::fs::create_dir_all(&bak);
    let fname = path.file_name().unwrap_or_default();
    let dest = bak.join(fname);
    if dest.exists() {
        let _ = std::fs::remove_file(&dest);
    }
    if std::fs::rename(path, &dest).is_err() {
        let _ = std::fs::copy(path, &dest);
        let _ = std::fs::remove_file(path);
    }
}

/// 列出 config 目录下某列表类型（prefix）的全部版本 json（除当前版本），按版本从新到旧排序。
fn list_old_files(prefix: &str, current_version: &str) -> Vec<(String, PathBuf)> {
    let dir = config_dir();
    let safe_cur = current_version.replace('.', "_");
    // prefix 需带尾随下划线（文件名形如 sheet_list_2026_07_16.json）
    let file_prefix = if prefix.ends_with('_') {
        prefix.to_string()
    } else {
        format!("{prefix}_")
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut list_files: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&file_prefix) {
                let ver = name
                    .strip_prefix(&file_prefix)
                    .and_then(|s| s.strip_suffix(".json"))
                    .unwrap_or("");
                Some((ver.to_string(), e.path()))
            } else {
                None
            }
        })
        .filter(|(ver, _)| *ver != safe_cur)
        .collect();
    list_files.sort_by(|a, b| b.0.cmp(&a.0));
    list_files
}

/// 对比当前版本与旧版本列表并归档。
///
/// 逻辑：从最近的旧版本开始，逐个对比（当前 vs 该版本）。找到第一个有变动的版本后，
/// 新增项即当前相对该版本的新增；随后把“当前版本 + 该有变动版本”之外的旧 json
/// 移动到 `config/bak/{kind}/` 存档目录（kind 为 sheet_list / music_list / map_list）。
/// 若所有旧版本都无变动（小型修复版本数据未变），则保留当前 + 最近一个旧版本，其余归档。
pub fn compare_and_archive(
    current_version: &str,
    current_list: &VersionedList,
    kind: &str,
) -> ComparisonResult {
    let old_files = list_old_files(kind, current_version);
    if old_files.is_empty() {
        return ComparisonResult::NoPrevious;
    }

    // 从新到旧逐个对比，找第一个有变动的版本
    for (ver, path) in &old_files {
        if let Some(prev_list) = load_list(path) {
            match compare_lists(current_version, current_list, Some(&prev_list)) {
                ComparisonResult::NewItems(items) => {
                    // 归档：保留当前版本 + 该有变动版本，其余旧 json 移到 bak
                    for (v, p) in &old_files {
                        if v != ver {
                            archive_file(p, kind);
                        }
                    }
                    log::debug!(
                        "{}: 对比有变动版本 {ver} 发现 {} 项新增，其余旧版本已归档到 config/bak/{kind}/",
                        kind,
                        items.len()
                    );
                    return ComparisonResult::NewItems(items);
                }
                _ => continue, // 无变动，继续对比更早版本
            }
        }
    }

    // 所有旧版本均无变动：保留当前 + 最近一个旧版本，其余归档
    for (i, (_, p)) in old_files.iter().enumerate() {
        if i > 0 {
            archive_file(p, kind);
        }
    }
    log::debug!("{kind}: 所有旧版本均无变动，仅保留最近一个旧版本，其余已归档到 config/bak/{kind}/");
    ComparisonResult::SameVersion
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
