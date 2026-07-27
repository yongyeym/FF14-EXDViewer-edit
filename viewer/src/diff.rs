use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui::Color32;
use crate::sheet::cell::CellResponse;

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
pub enum DiffType { Added, Deleted }

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

#[derive(Clone)]
pub struct DiffResult {
    pub columns: Vec<String>,
    pub diff_rows: Vec<DiffRow>,
    pub error: Option<String>,
}

/// Shared state for background diff computation.
pub type DiffSharedResult = Arc<Mutex<Option<DiffResult>>>;

pub fn find_version_folders() -> Vec<String> {
    let d = exe_export_data_dir();
    let Ok(entries) = std::fs::read_dir(&d) else { return vec![] };
    let mut f: Vec<String> = entries.filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    f.sort_by(|a, b| {
        let ap: Vec<i32> = a.split('.').filter_map(|s| s.parse().ok()).collect();
        let bp: Vec<i32> = b.split('.').filter_map(|s| s.parse().ok()).collect();
        ap.cmp(&bp)
    });
    f
}

pub fn has_csv_for_sheet(version: &str, sheet_name: &str) -> bool {
    exe_export_data_dir().join(version).join(format!("{sheet_name}.csv")).exists()
}

/// Load CSV into a map: row_id -> (row_hash, cell_values).
/// Uses case-insensitive hashing to avoid FALSE/false noise.
fn load_csv_ci(version: &str, sheet_name: &str) -> Result<(Vec<String>, HashMap<String, (u64, Vec<String>)>), String> {
    let csv_path = exe_export_data_dir().join(version).join(format!("{sheet_name}.csv"));
    let raw = std::fs::read(&csv_path).map_err(|e| format!("读取CSV失败: {e}"))?;
    let content = if raw.starts_with(b"\xef\xbb\xbf") {
        String::from_utf8_lossy(&raw[3..]).to_string()
    } else {
        String::from_utf8_lossy(&raw).to_string()
    };

    let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(content.as_bytes());
    let headers: Vec<String> = reader.headers().map_err(|e| format!("CSV表头解析失败: {e}"))?
        .iter().map(|h| h.to_string()).collect();

    let mut rows: HashMap<String, (u64, Vec<String>)> = HashMap::new();

    // Detect if CSV has 'Subrow' column (skip it if present)
    let has_subrow = headers.get(1).map(|h| h == "Subrow").unwrap_or(false);

    for result in reader.records() {
        let record = result.map_err(|e| format!("CSV记录解析失败: {e}"))?;
        let row_id = record.get(0).unwrap_or("").to_string();
        let values: Vec<String> = if has_subrow {
            record.iter().skip(2) // Row, Subrow → data starts at col 2
        } else {
            record.iter().skip(1) // Row → data starts at col 1
        }
        .map(|v| v.to_ascii_lowercase())
        .collect();
        let hash = ci_hash(&values);
        rows.entry(row_id).or_insert((hash, values));
    }
    Ok((headers, rows))
}

/// Case-insensitive hash of cell values.
fn ci_hash(values: &[String]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    values.len().hash(&mut h);
    for v in values {
        v.len().hash(&mut h);
        v.hash(&mut h);
    }
    h.finish()
}

/// Compare two row maps and produce diff entries.
fn compute_diff(
    old: &HashMap<String, (u64, Vec<String>)>,
    new: &HashMap<String, (u64, Vec<String>)>,
) -> Vec<DiffRow> {
    let mut result = Vec::new();
    let all: Vec<&String> = old.keys().chain(new.keys()).collect();
    let mut keys: Vec<&String> = Vec::with_capacity(all.len());
    for k in &all { if !keys.contains(k) { keys.push(k); } }
    keys.sort();

    let mut diff_count = 0;
    for key in keys {
        match (old.get(key), new.get(key)) {
            (Some(_), None) => { result.push(DiffRow { row_key: key.clone(), diff_type: DiffType::Deleted, cells: vec![] }); }
            (None, Some((_, cells))) => { result.push(DiffRow { row_key: key.clone(), diff_type: DiffType::Added, cells: cells.clone() }); }
            (Some((_, oc)), Some((_, nc))) => {
                let common = oc.len().min(nc.len());
                let cells_match = oc[..common] == nc[..common];
                if !cells_match {
                    // Smart comparison: check if differing cells are numerically equal
                    let mut real_diffs = Vec::new();
                    for i in 0..common {
                        if oc[i] != nc[i] {
                            // Try numeric comparison (handles scientific notation vs decimal)
                            let numeric_eq = match (oc[i].parse::<f64>(), nc[i].parse::<f64>()) {
                                (Ok(a), Ok(b)) => (a - b).abs() < 0.5,
                                _ => false,
                            };
                            if !numeric_eq {
                                real_diffs.push(i);
                            }
                        }
                    }
                    if !real_diffs.is_empty() {
                        if diff_count < 3 {
                            log::debug!("Row {:?} REAL diffs at cols {:?}: old[col48]={:?} new[col48]={:?}", key, real_diffs, oc.get(48), nc.get(48));
                        }
                        diff_count += 1;
                        result.push(DiffRow { row_key: key.clone(), diff_type: DiffType::Deleted, cells: oc.clone() });
                        result.push(DiffRow { row_key: key.clone(), diff_type: DiffType::Added, cells: nc.clone() });
                    }
                }
            }
            _ => {}
        }
    }
    log::debug!("Diff total: {} row groups with changes", result.len());
    result
}

/// Run diff on a BACKGROUND THREAD. Returns immediately; the result becomes available
/// via the shared `Arc<Mutex<Option<DiffResult>>>`.
pub fn start_background_diff(
    old_version: String,
    new_version: String,
    sheet_name: String,
) -> DiffSharedResult {
    let result = Arc::new(Mutex::new(None::<DiffResult>));
    let r2 = result.clone();

    std::thread::spawn(move || {
        log::info!("后台Diff线程启动: {old_version} vs {new_version}");
        let old = load_csv_ci(&old_version, &sheet_name);
        let new = load_csv_ci(&new_version, &sheet_name);

        let diff_result = match (old, new) {
            (Ok((headers, old_map)), Ok((_, new_map))) => {
                let diff_rows = compute_diff(&old_map, &new_map);
                log::info!("Diff完成: {} 行变更", diff_rows.len());
                DiffResult { columns: headers, diff_rows, error: None }
            }
            (Err(e), _) | (_, Err(e)) => DiffResult {
                columns: vec![], diff_rows: vec![], error: Some(e),
            },
        };

        let mut lock = r2.lock().unwrap();
        *lock = Some(diff_result);
        log::info!("后台Diff结果已就绪");
    });

    result
}

// ── UI ─────────────────────────────────────────────────────────────

pub fn draw_diff_window(
    diff_state: &mut DiffState,
    ui: &mut egui::Ui,
    sheet_name: &str,
    diff_result: &DiffSharedResult,
) -> Option<DiffAction> {
    if !diff_state.active { return None; }

    // Poll background diff result
    if diff_state.status == "comparing" {
        if let Ok(mut lock) = diff_result.lock() {
            if let Some(result) = lock.take() {
                if let Some(err) = result.error {
                    diff_state.status = format!("error:{err}");
                } else {
                    diff_state.columns = result.columns;
                    diff_state.diff_rows = result.diff_rows;
                    diff_state.status = "done".into();
                }
            }
        }
    }

    let versions = find_version_folders();
    let vs: Vec<String> = versions.into_iter()
        .filter(|v| has_csv_for_sheet(v, sheet_name)).collect();

    if vs.is_empty() {
        egui::Window::new("版本Diff").id("diff_window".into()).resizable(false)
            .default_size([350.0, 120.0]).show(ui.ctx(), |ui| {
                ui.label("未找到CSV文件，请先导出CSV。");
                if ui.button("关闭").clicked() { diff_state.active = false; diff_state.status = "idle".into(); }
            });
        return None;
    }

    let mut action: Option<DiffAction> = None;
    let mut close = false;
    let mut old_ver = diff_state.old_version.clone();
    let mut new_ver = diff_state.new_version.clone();
    let status = diff_state.status.clone();
    let diff_count = diff_state.diff_rows.len();

    egui::Window::new("选择版本Diff对比的CSV文件").id("diff_window2".into())
        .resizable(false).default_size([380.0, 180.0]).show(ui.ctx(), |ui| {
        ui.horizontal(|ui| { ui.label("旧版本:");
            egui::ComboBox::from_id_salt("diff_old").selected_text(&old_ver).show_ui(ui, |ui| {
                for opt in &vs { ui.selectable_value(&mut old_ver, opt.clone(), opt.as_str()); }
            });
        });
        ui.horizontal(|ui| { ui.label("新版本:");
            egui::ComboBox::from_id_salt("diff_new").selected_text(&new_ver).show_ui(ui, |ui| {
                for opt in &vs { ui.selectable_value(&mut new_ver, opt.clone(), opt.as_str()); }
            });
        });
        ui.separator();
        match status.as_str() {
            "comparing" => { ui.horizontal(|ui| { ui.spinner(); ui.label("正在对比中，请稍后……"); }); }
            "done" => { ui.label(format!("对比完成，共 {diff_count} 行变更")); }
            s if s.starts_with("error:") => { ui.colored_label(Color32::RED, &s[6..]); }
            _ => {}
        }
        ui.horizontal(|ui| {
            if ui.button("取消对比").clicked() { close = true; }
            if status != "comparing" && ui.button("开始对比").clicked() {
                if old_ver == new_ver { diff_state.status = "error:相同版本无法对比！".into(); }
                else { action = Some(DiffAction::Compare { old: old_ver.clone(), new: new_ver.clone(), sheet: sheet_name.to_string() }); }
            }
        });
    });

    if close { diff_state.active = false; diff_state.status = "idle".into(); return None; }
    diff_state.old_version = old_ver;
    diff_state.new_version = new_ver;
    action
}

pub enum DiffAction {
    Compare { old: String, new: String, sheet: String },
}

pub fn draw_diff_table(diff: &DiffState, ui: &mut egui::Ui) -> CellResponse {
    let rows = &diff.diff_rows;
    let cols = &diff.columns;
    let data_col_start = if cols.len() > 1 && cols[1] == "Subrow" { 2 } else { 1 };
    let header_names: Vec<&str> = cols.iter().skip(data_col_start).map(|s| s.as_str()).collect();

    egui::ScrollArea::both().auto_shrink(false).id_salt("diff_body").show(ui, |ui| {
        // Header: Diff, Row, then all data columns
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Diff").strong().size(11.0));
            ui.label(egui::RichText::new("Row").strong().size(11.0));
            for name in &header_names {
                ui.label(egui::RichText::new(*name).strong().size(11.0));
            }
        });
        ui.separator();

        // All diff rows, no bg color, no truncation
        for row in rows {
            ui.horizontal(|ui| {
                // Diff marker FIRST
                let m = match row.diff_type {
                    DiffType::Deleted => egui::RichText::new("-").color(Color32::RED).strong(),
                    DiffType::Added => egui::RichText::new("+").color(Color32::GREEN).strong(),
                };
                ui.add(egui::Label::new(m).sense(egui::Sense::click()));
                // Row key
                ui.label(&row.row_key);
                // ALL cell values
                for cell in &row.cells {
                    ui.add(egui::Label::new(cell.as_str()).sense(egui::Sense::click()).wrap_mode(egui::TextWrapMode::Truncate));
                }
            });
        }
    });
    CellResponse::None
}
fn exe_export_data_dir() -> PathBuf {
    std::env::current_exe().ok()
        .and_then(|p| p.parent().map(|p| p.join("export").join("data")))
        .unwrap_or_else(|| PathBuf::from("export/data"))
}
