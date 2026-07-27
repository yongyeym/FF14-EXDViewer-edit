use std::collections::HashMap;
use std::path::PathBuf;
use egui::Color32;

/// State for version-diff comparison.
#[derive(Clone)]
pub struct DiffState {
    pub active: bool,
    pub old_version: String,
    pub new_version: String,
    pub status: String,       // "idle"/"selecting"/"comparing"/"done"/"error:..."
    pub diff_rows: Vec<DiffRow>,
    /// Column headers from the CSV (for rendering). Populated when CSV is parsed.
    pub columns: Vec<String>,
}

#[derive(Clone)]
pub struct DiffRow {
    pub row_key: String,       // "row_id" or "row_id.subrow_id"
    pub diff_type: DiffType,
    pub cells: Vec<String>,    // cell values in order (no Row column)
}

#[derive(Clone, PartialEq)]
pub enum DiffType {
    Added,
    Deleted,
    Modified,
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

/// Find version folders under export/data/ that contain CSV files.
pub fn find_version_folders() -> Vec<String> {
    let export_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("export").join("data")))
        .unwrap_or_else(|| PathBuf::from("export/data"));

    let Ok(entries) = std::fs::read_dir(&export_dir) else {
        return Vec::new();
    };

    let mut folders: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| name != "local") // skip default "local" export folder
        .collect();

    folders.sort_by(|a, b| {
        // Sort by version: parse number parts
        let a_parts: Vec<i32> = a.split('.').filter_map(|s| s.parse().ok()).collect();
        let b_parts: Vec<i32> = b.split('.').filter_map(|s| s.parse().ok()).collect();
        a_parts.cmp(&b_parts)
    });

    folders
}

/// Check if a version folder has a CSV file for the given sheet.
pub fn has_csv_for_sheet(version: &str, sheet_name: &str) -> bool {
    let export_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("export").join("data").join(version)))
        .unwrap_or_else(|| PathBuf::from("export/data").join(version));

    let csv_name = format!("{sheet_name}.csv");
    let csv_path = export_dir.join(&csv_name);
    csv_path.exists()
}

/// Read a CSV file for a given version+sheet and parse into rows.
pub fn load_csv(
    version: &str,
    sheet_name: &str,
) -> Result<(Vec<String>, HashMap<String, Vec<String>>), String> {
    let export_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("export").join("data").join(version)))
        .unwrap_or_else(|| PathBuf::from("export/data").join(version));

    let csv_path = export_dir.join(format!("{sheet_name}.csv"));
    let content = std::fs::read_to_string(&csv_path)
        .map_err(|e| format!("读取CSV失败: {e}"))?;

    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(content.as_bytes());

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| format!("CSV表头解析失败: {e}"))?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let mut rows: HashMap<String, Vec<String>> = HashMap::new();
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
        rows.entry(key.clone())
            .and_modify(|existing| {
                // If we already have this key, we need to concat subrows
                // This handles multi-subrow CSV files
                existing.extend(values.clone());
            })
            .or_insert(values);
    }

    Ok((headers, rows))
}

/// Draw the version diff selection window.
pub fn draw_diff_window(diff_state: &mut DiffState, ui: &mut egui::Ui, sheet_name: &str) {
    if !diff_state.active {
        return;
    }

    let versions = find_version_folders();
    let all_options: Vec<String> = std::iter::once("当前游戏解包数据".to_string())
        .chain(versions.iter().filter(|v| has_csv_for_sheet(v, sheet_name)).cloned())
        .collect();

    let mut old_ver = diff_state.old_version.clone();
    let mut new_ver = diff_state.new_version.clone();
    let status = diff_state.status.clone();
    let diff_count = diff_state.diff_rows.len();
    let mut close_diff = false;
    let mut run_compare = false;

    egui::Window::new("选择版本Diff对比的CSV文件")
        .id(egui::Id::new("diff_window"))
        .resizable(false)
        .default_size([400.0, 200.0])
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                ui.label("旧版本:");
                egui::ComboBox::from_id_salt("diff_old")
                    .selected_text(&old_ver)
                    .show_ui(ui, |ui| {
                        for opt in &all_options {
                            ui.selectable_value(&mut old_ver, opt.clone(), opt.as_str());
                        }
                    });
            });

            ui.horizontal(|ui| {
                ui.label("新版本:");
                egui::ComboBox::from_id_salt("diff_new")
                    .selected_text(&new_ver)
                    .show_ui(ui, |ui| {
                        for opt in &all_options {
                            ui.selectable_value(&mut new_ver, opt.clone(), opt.as_str());
                        }
                    });
            });

            ui.separator();

            match status.as_str() {
                "comparing" => {
                    ui.label("正在对比中，请稍后……");
                    ui.spinner();
                }
                "done" => {
                    ui.label(format!("对比完成，共 {diff_count} 行变更"));
                }
                s if s.starts_with("error:") => {
                    ui.colored_label(egui::Color32::RED, &s[6..]);
                }
                _ => {}
            }

            ui.horizontal(|ui| {
                if ui.button("取消对比").clicked() {
                    close_diff = true;
                }
                if status != "comparing" && ui.button("开始对比").clicked() {
                    run_compare = true;
                }
            });
        });

    if close_diff {
        diff_state.active = false;
        diff_state.status = "idle".into();
        return;
    }

    diff_state.old_version = old_ver;
    diff_state.new_version = new_ver;

    if run_compare {
        if diff_state.old_version == diff_state.new_version {
            diff_state.status = "error:相同版本无法对比！".into();
        } else {
            diff_state.status = "comparing".into();
            do_diff(diff_state, sheet_name);
        }
    }
}

fn do_diff(diff_state: &mut DiffState, sheet_name: &str) {
    let old_rows = if diff_state.old_version == "当前游戏解包数据" {
        Err("请先将当前版本导出为CSV文件后再对比".into())
    } else {
        load_csv(&diff_state.old_version, sheet_name)
    };

    let new_rows = if diff_state.new_version == "当前游戏解包数据" {
        Err("请先将当前版本导出为CSV文件后再对比".into())
    } else {
        load_csv(&diff_state.new_version, sheet_name)
    };

    match (old_rows, new_rows) {
        (Ok((headers, old)), Ok((_, new))) => {
            diff_state.columns = headers.clone();
            diff_state.diff_rows = compute_diff(&headers, &old, &new);
            diff_state.status = "done".into();
        }
        (Err(e), _) | (_, Err(e)) => {
            diff_state.status = format!("error:{e}");
        }
    }
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
                // Header row
                ui.label("Row");
                ui.label("Diff");
                for col in columns.iter().skip(2) {
                    ui.label(col);
                }
                ui.end_row();

                // Data rows
                for row in rows {
                    // Row key
                    let label = egui::Label::new(&row.row_key).sense(egui::Sense::click());
                    ui.add(label);

                    // Diff marker
                    let marker = match row.diff_type {
                        DiffType::Deleted => egui::RichText::new("-").color(Color32::RED),
                        DiffType::Added => egui::RichText::new("+").color(Color32::GREEN),
                        DiffType::Modified => egui::RichText::new("±").color(Color32::YELLOW),
                    };
                    ui.add(egui::Label::new(marker).sense(egui::Sense::click()));

                    // Cell data
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

use crate::sheet::cell::CellResponse;


/// Compare old and new row data, producing diff entries.
pub fn compute_diff(
    _headers: &[String],
    old_rows: &HashMap<String, Vec<String>>,
    new_rows: &HashMap<String, Vec<String>>,
) -> Vec<DiffRow> {
    let mut result = Vec::new();

    // Collect all keys
    let mut all_keys: Vec<&String> = old_rows.keys().chain(new_rows.keys()).collect();
    all_keys.sort(); // Sort by key (which is row_id or row_id.subrow_id)
    all_keys.dedup();

    for key in all_keys {
        let old_val = old_rows.get(key);
        let new_val = new_rows.get(key);

        match (old_val, new_val) {
            (Some(old), None) => {
                // Deleted
                result.push(DiffRow {
                    row_key: key.clone(),
                    diff_type: DiffType::Deleted,
                    cells: old.clone(),
                });
            }
            (None, Some(new)) => {
                // Added
                result.push(DiffRow {
                    row_key: key.clone(),
                    diff_type: DiffType::Added,
                    cells: new.clone(),
                });
            }
            (Some(old), Some(new)) => {
                if old != new {
                    // Modified: show old first, then new
                    result.push(DiffRow {
                        row_key: key.clone(),
                        diff_type: DiffType::Deleted,
                        cells: old.clone(),
                    });
                    result.push(DiffRow {
                        row_key: key.clone(),
                        diff_type: DiffType::Added,
                        cells: new.clone(),
                    });
                }
            }
            (None, None) => unreachable!(),
        }
    }

    result
}
