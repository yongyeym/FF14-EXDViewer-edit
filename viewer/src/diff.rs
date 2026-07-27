use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use egui::Color32;

use crate::sheet::cell::CellResponse;

/// State for version-diff comparison.
#[derive(Clone)]
pub struct DiffState {
    pub active: bool,
    pub old_version: String,
    pub new_version: String,
    pub status: String,
    pub diff_rows: Vec<DiffRow>,
    pub columns: Vec<String>,
}

#[derive(Clone)]
pub struct DiffRow {
    pub row_key: String,
    pub diff_type: DiffType,
    pub cells: Vec<String>,
}

#[derive(Clone, PartialEq)]
pub enum DiffType {
    Added,
    Deleted,
}

impl DiffState {
    pub fn new() -> Self {
        Self {
            active: false,
            old_version: String::new(),
            new_version: String::new(),
            status: "idle".into(),
            diff_rows: Vec::new(),
            columns: Vec::new(),
        }
    }
}

/// Find version folders under export/data/.
pub fn find_version_folders() -> Vec<String> {
    let export_dir = exe_export_data_dir();
    let Ok(entries) = std::fs::read_dir(&export_dir) else { return Vec::new() };

    let mut folders: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();

    folders.sort_by(|a, b| {
        let a_p: Vec<i32> = a.split('.').filter_map(|s| s.parse().ok()).collect();
        let b_p: Vec<i32> = b.split('.').filter_map(|s| s.parse().ok()).collect();
        a_p.cmp(&b_p)
    });
    folders
}

/// Check if a CSV for the sheet exists in a version folder.
pub fn has_csv_for_sheet(version: &str, sheet_name: &str) -> bool {
    exe_export_data_dir().join(version).join(format!("{sheet_name}.csv")).exists()
}

/// Compute a fast row hash from cell values (avoids full string comparisons).
fn row_hash(values: &[String]) -> u64 {
    let mut h = DefaultHasher::new();
    values.len().hash(&mut h);
    for v in values {
        v.len().hash(&mut h);
        // First 8 chars for a quick fingerprint
        let prefix: String = v.chars().take(8).collect();
        prefix.hash(&mut h);
    }
    h.finish()
}

/// Load CSV, returning headers and a map of row_key → (hash, values).
pub fn load_csv_fast(
    version: &str,
    sheet_name: &str,
) -> Result<(Vec<String>, HashMap<String, (u64, Vec<String>)>), String> {
    let csv_path = exe_export_data_dir().join(version).join(format!("{sheet_name}.csv"));
    let raw = std::fs::read(&csv_path).map_err(|e| format!("读取CSV失败: {e}"))?;

    // Skip UTF-8 BOM if present
    let content = if raw.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&raw[3..]).to_string()
    } else {
        String::from_utf8_lossy(&raw).to_string()
    };

    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(content.as_bytes());

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| format!("CSV表头解析失败: {e}"))?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let mut rows: HashMap<String, (u64, Vec<String>)> = HashMap::new();

    for result in reader.records() {
        let record = result.map_err(|e| format!("CSV记录解析失败: {e}"))?;
        let row_id = record.get(0).unwrap_or("").to_string();
        let subrow = record.get(1).map(|s| s.to_string()).unwrap_or_default();
        let key = if subrow.is_empty() || subrow == "0" {
            row_id
        } else {
            format!("{row_id}.{subrow}")
        };

        let values: Vec<String> = record.iter().skip(2).map(|v| v.to_string()).collect();
        let hash = row_hash(&values);

        rows.entry(key).or_insert((hash, values));
    }

    Ok((headers, rows))
}

/// Compare two sets of rows using row hashes for fast comparison.
pub fn compute_diff_fast(
    old_rows: &HashMap<String, (u64, Vec<String>)>,
    new_rows: &HashMap<String, (u64, Vec<String>)>,
) -> Vec<DiffRow> {
    let mut result = Vec::with_capacity(
        old_rows.len().max(new_rows.len()) / 10 // estimate ~10% change
    );

    // Only scan keys present in old OR new
    let all_keys: Vec<&String> = old_rows.keys().chain(new_rows.keys()).collect();

    // Use sorted iteration for deterministic output
    let mut sorted_keys: Vec<&String> = Vec::with_capacity(all_keys.len());
    for k in &all_keys {
        if !sorted_keys.contains(k) {
            sorted_keys.push(k);
        }
    }
    sorted_keys.sort();

    for key in sorted_keys {
        match (old_rows.get(key), new_rows.get(key)) {
            (Some(_), None) => {
                // Deleted – skip cell data, we only need to show the key
                result.push(DiffRow {
                    row_key: key.clone(),
                    diff_type: DiffType::Deleted,
                    cells: Vec::new(),
                });
            }
            (None, Some((_, new_cells))) => {
                result.push(DiffRow {
                    row_key: key.clone(),
                    diff_type: DiffType::Added,
                    cells: new_cells.clone(),
                });
            }
            (Some((old_hash, old_cells)), Some((new_hash, new_cells))) => {
                // Quick hash comparison first
                if old_hash != new_hash || old_cells != new_cells {
                    result.push(DiffRow {
                        row_key: key.clone(),
                        diff_type: DiffType::Deleted,
                        cells: old_cells.clone(),
                    });
                    result.push(DiffRow {
                        row_key: key.clone(),
                        diff_type: DiffType::Added,
                        cells: new_cells.clone(),
                    });
                }
            }
            (None, None) => {}
        }
    }

    result
}

// ── Async-compatible diff operation ────────────────────────────────

/// Holds the entire diff computation result so it can be sent across async boundaries.
#[derive(Clone)]
pub struct DiffResult {
    pub columns: Vec<String>,
    pub diff_rows: Vec<DiffRow>,
    pub error: Option<String>,
}

/// Run the full diff in a CPU-bound background thread and return the result.
/// This does NOT block the UI thread.
pub fn run_diff_background(
    old_version: String,
    new_version: String,
    sheet_name: String,
) -> DiffResult {
    let old = load_csv_fast(&old_version, &sheet_name);
    let new = load_csv_fast(&new_version, &sheet_name);

    match (old, new) {
        (Ok((headers, old_map)), Ok((_, new_map))) => {
            let diff_rows = compute_diff_fast(&old_map, &new_map);
            log::info!("Diff完成: {} 行变更", diff_rows.len());
            DiffResult { columns: headers, diff_rows, error: None }
        }
        (Err(e), _) | (_, Err(e)) => {
            DiffResult { columns: Vec::new(), diff_rows: Vec::new(), error: Some(e) }
        }
    }
}

// ── UI functions ───────────────────────────────────────────────────

/// Draw the version diff selection window.
pub fn draw_diff_window(
    diff_state: &mut DiffState,
    ui: &mut egui::Ui,
    sheet_name: &str,
) -> Option<DiffAction> {
    if !diff_state.active {
        return None;
    }

    let versions = find_version_folders();
    let versions_for_sheet: Vec<String> = versions.into_iter()
        .filter(|v| has_csv_for_sheet(v, sheet_name))
        .collect();

    if versions_for_sheet.is_empty() {
        egui::Window::new("版本Diff")
            .id(egui::Id::new("diff_window"))
            .resizable(false)
            .default_size([350.0, 120.0])
            .show(ui.ctx(), |ui| {
                ui.label("未找到已导出的CSV文件，请先导出CSV。");
                if ui.button("关闭").clicked() {
                    diff_state.active = false;
                    diff_state.status = "idle".into();
                }
            });
        return None;
    }

    let mut action: Option<DiffAction> = None;
    let mut close = false;

    // Clone values for closure
    let mut old_ver = diff_state.old_version.clone();
    let mut new_ver = diff_state.new_version.clone();
    let status = diff_state.status.clone();
    let diff_count = diff_state.diff_rows.len();

    egui::Window::new("选择版本Diff对比的CSV文件")
        .id(egui::Id::new("diff_window"))
        .resizable(false)
        .default_size([380.0, 180.0])
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                ui.label("旧版本:");
                egui::ComboBox::from_id_salt("diff_old")
                    .selected_text(&old_ver)
                    .show_ui(ui, |ui| {
                        for opt in &versions_for_sheet {
                            ui.selectable_value(&mut old_ver, opt.clone(), opt.as_str());
                        }
                    });
            });

            ui.horizontal(|ui| {
                ui.label("新版本:");
                egui::ComboBox::from_id_salt("diff_new")
                    .selected_text(&new_ver)
                    .show_ui(ui, |ui| {
                        for opt in &versions_for_sheet {
                            ui.selectable_value(&mut new_ver, opt.clone(), opt.as_str());
                        }
                    });
            });

            ui.separator();

            match status.as_str() {
                "comparing" => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("正在对比中，请稍后……");
                    });
                }
                "done" => {
                    ui.label(format!("对比完成，共 {diff_count} 行变更"));
                }
                s if s.starts_with("error:") => {
                    ui.colored_label(Color32::RED, &s[6..]);
                }
                _ => {}
            }

            ui.horizontal(|ui| {
                if ui.button("取消对比").clicked() {
                    close = true;
                }
                if status != "comparing" {
                    if ui.button("开始对比").clicked() {
                        if old_ver == new_ver {
                            diff_state.status = "error:相同版本无法对比！".into();
                        } else {
                            action = Some(DiffAction::Compare {
                                old: old_ver.clone(),
                                new: new_ver.clone(),
                                sheet: sheet_name.to_string(),
                            });
                        }
                    }
                }
            });
        });

    if close {
        diff_state.active = false;
        diff_state.status = "idle".into();
        return None;
    }

    diff_state.old_version = old_ver;
    diff_state.new_version = new_ver;
    action
}

pub enum DiffAction {
    Compare { old: String, new: String, sheet: String },
}

/// Draw the diff results table.
pub fn draw_diff_table(diff: &DiffState, ui: &mut egui::Ui) -> CellResponse {
    let rows = &diff.diff_rows;
    let columns = &diff.columns;

    egui::ScrollArea::horizontal().auto_shrink(false).show(ui, |ui| {
        egui::Grid::new("diff_grid")
            .striped(true)
            .min_col_width(60.0)
            .show(ui, |ui| {
                ui.label("Row");
                ui.label("Diff");
                for col in columns.iter().skip(2) {
                    ui.label(col);
                }
                ui.end_row();

                for row in rows {
                    let marker = match row.diff_type {
                        DiffType::Deleted => egui::RichText::new("-").color(Color32::RED),
                        DiffType::Added => egui::RichText::new("+").color(Color32::GREEN),
                    };
                    ui.label(&row.row_key);
                    ui.add(egui::Label::new(marker).sense(egui::Sense::click()));

                    for cell in &row.cells {
                        ui.add(
                            egui::Label::new(cell.as_str())
                                .sense(egui::Sense::click())
                                .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                    }
                    ui.end_row();
                }
            });
    });
    CellResponse::None
}

fn exe_export_data_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("export").join("data")))
        .unwrap_or_else(|| PathBuf::from("export/data"))
}
