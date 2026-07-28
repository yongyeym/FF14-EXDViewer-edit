use std::collections::HashMap;
use crate::excel::provider::ExcelProvider;
use crate::sheet::cell::draw_icon;
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
    pub modal_icon_id: Option<u32>,
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
            modal_icon_id: None,
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
    log::debug!("load_csv_ci: {:?}", csv_path);
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

        match r2.lock() {
            Ok(mut lock) => { *lock = Some(diff_result); log::info!("后台Diff结果已就绪"); }
            Err(e) => log::error!("后台Diff Mutex损坏: {e}"),
        }
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

pub fn draw_diff_table(diff: &DiffState, ui: &mut egui::Ui, context: &crate::sheet::TableContext) -> CellResponse {
    let rows = &diff.diff_rows;
    let cols = &diff.columns;
    let data_col_start = if cols.len() > 1 && cols[1] == "Subrow" { 2 } else { 1 };

    // Build icon column names
    log::debug!("draw_diff_table: rows={} cols={}", rows.len(), cols.len());
    let all_cols = context.columns().unwrap_or_default();
    let mut icon_col_names = std::collections::HashSet::new();
    for (sc, _sd) in &all_cols {
        if format!("{:?}", sc.meta()) == "Icon" {
            icon_col_names.insert(sc.name().to_string());
        }
    }

    // Pre-fetch header metadata
    let mut col_meta: Vec<(String, String)> = Vec::new(); // (column_name, metadata_string)
    let header_row_height = ui.text_style_height(&egui::TextStyle::Heading)
        + ui.spacing().item_spacing.y
        + ui.text_style_height(&egui::TextStyle::Small)
        + 4.0;
    let col_count = context.column_count();

    for ci in 0..col_count {
        if let Ok(((sc, shc), _)) = context.get_column_by_index(ci as u32) {
            let meta = format!("{} | {} (0x{:02X}) | {:?}",
                ci, sc.name(), shc.offset(), shc.kind());
            col_meta.push((sc.name().to_string(), meta));
        } else {
            col_meta.push((cols.get(data_col_start + ci).cloned().unwrap_or_default(), String::new()));
        }
    }

    let total_cols = 2 + col_count; // Diff + Row + data columns

    let id = egui::Id::new("diff_egui_table");
    let mut modal_icon_id: Option<u32> = None;
    ui.push_id(id, |ui| {
        let table = egui_table::Table::new()
            .num_rows(rows.len() as u64)
            .columns(vec![egui_table::Column::new(80.0).resizable(true); total_cols])
            .num_sticky_cols(2)
            .headers([egui_table::HeaderRow::new(header_row_height)]);

        table.show(ui, &mut DiffTableDelegate {
            rows,
            col_meta: &col_meta,
            icon_col_names: &icon_col_names,
            context,
            modal_icon_id: &mut modal_icon_id,
        });

        // Icon preview modal
        if let Some(icon_id) = modal_icon_id.take() {
            let ctx = context.global();
            let sg = ctx.backend().excel().clone();
            let icon_mgr = &ctx.icon_manager();
            egui::Modal::new(egui::Id::new("diff_icon_modal")).show(ui.ctx(), |ui| {
                let snapshot = icon_mgr.get_or_insert_icon(icon_id, true, ui.ctx(), move || {
                    let sg = sg.clone();
                    crate::utils::TrackedPromise::spawn_local(
                        async move { sg.get_icon(icon_id, true).await }
                    )
                });
                if let crate::utils::ManagedIcon::Loaded(source) = snapshot {
                    ui.add(egui::Image::new(source).fit_to_exact_size(egui::vec2(400.0, 400.0)));
                } else {
                    ui.label("加载中...");
                }
                if ui.button("关闭").clicked() {
                    ui.close();
                }
            });
        }
    });
    CellResponse::None
}

struct DiffTableDelegate<'a> {
    rows: &'a [DiffRow],
    col_meta: &'a [(String, String)],
    icon_col_names: &'a std::collections::HashSet<String>,
    context: &'a crate::sheet::TableContext,
    modal_icon_id: &'a mut Option<u32>,
}

impl egui_table::TableDelegate for DiffTableDelegate<'_> {
    fn header_cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::HeaderCellInfo) {
        let egui_table::HeaderCellInfo { col_range, .. } = cell;

        let data_idx = col_range.start.checked_sub(2);

        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 2)).show(ui, |ui| {
            if col_range.start == 0 {
                ui.centered_and_justified(|ui| ui.heading("Diff"));
            } else if col_range.start == 1 {
                ui.centered_and_justified(|ui| ui.heading("Row"));
            } else if let Some(di) = data_idx {
                if let Some((name, meta)) = self.col_meta.get(di) {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.heading(name);
                            ui.label(egui::RichText::new(meta).small().color(Color32::GRAY));
                        });
                    });
                }
            }
        });
    }

    fn default_row_height(&self) -> f32 {
        20.0
    }

    fn row_top_offset(&self, _ctx: &egui::Context, _table_id: egui::Id, row_nr: u64) -> f32 {
        let line_height = 20.0;
        let min_icon_height = 32.0; // minimum height to fully display an icon
        let chars_per_col = 30.0;
        let mut offset = 0.0f32;
        for r in 0..=row_nr {
            if r == 0 { continue; }
            let ri = (r - 1) as usize;
            let mut max_lines = 1u32;
            let mut has_icon = false;
            if let Some(row) = self.rows.get(ri) {
                for (ci, cell) in row.cells.iter().enumerate() {
                    let est_lines = ((cell.len() as f32) / chars_per_col).ceil() as u32;
                    if est_lines > max_lines { max_lines = est_lines; }
                    // Check if this column is an icon column
                    if let Some(col_info) = self.col_meta.get(ci) {
                        if self.icon_col_names.contains(&col_info.0) {
                            has_icon = true;
                        }
                    }
                }
            }
            let text_height = (max_lines as f32).max(1.0) * line_height;
            let row_h = if has_icon { text_height.max(min_icon_height) } else { text_height };
            offset += row_h;
        }
        offset
    }

    fn cell_ui(&mut self, ui: &mut egui::Ui, cell: &egui_table::CellInfo) {
        let egui_table::CellInfo { row_nr, col_nr, .. } = *cell;
        let row_idx = row_nr as usize;

        if row_idx >= self.rows.len() {
            return;
        }

        let diff_row = &self.rows[row_idx];

        // Alternating row background
        if row_nr % 2 == 1 {
            let rect = ui.max_rect();
            ui.painter().rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
        }

        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 2)).show(ui, |ui| {
            if col_nr == 0 {
                // Diff marker
                let m = match diff_row.diff_type {
                    DiffType::Deleted => egui::RichText::new("-").color(Color32::RED).strong(),
                    DiffType::Added => egui::RichText::new("+").color(Color32::GREEN).strong(),
                };
                ui.centered_and_justified(|ui| ui.add(egui::Label::new(m).sense(egui::Sense::click())));
            } else if col_nr == 1 {
                // Row key
                ui.add(egui::Label::new(&diff_row.row_key).sense(egui::Sense::click()));
            } else {
                // Data column
                let ci = col_nr - 2;
                if ci < diff_row.cells.len() && ci < self.col_meta.len() {
                    let col_name = &self.col_meta[ci].0;
                    if self.icon_col_names.contains(col_name) {
                        if let Ok(id) = diff_row.cells[ci].parse::<u32>() {
                            let resp = {
                                use crate::excel::provider::ExcelHeader;
                                draw_icon(self.context.global(), ui, id, self.context.sheet().name(), col_name)
                            };
                            if resp.clicked() {
                                *self.modal_icon_id = Some(id);
                            }
                            return;
                        }
                    }
                    let text = diff_row.cells[ci].clone();
                    let resp = ui.add(egui::Label::new(&text).sense(egui::Sense::click()).wrap_mode(egui::TextWrapMode::Wrap));
                    resp.context_menu(|ui| {
                        if ui.button("复制").clicked() {
                            ui.ctx().copy_text(text);
                            ui.close();
                        }
                    });
                }
            }
        });
    }
}
fn exe_export_data_dir() -> PathBuf {
    std::env::current_exe().ok()
        .and_then(|p| p.parent().map(|p| p.join("export").join("data")))
        .unwrap_or_else(|| { log::warn!("无法获取exe路径，使用export/data"); PathBuf::from("export/data") })
}
