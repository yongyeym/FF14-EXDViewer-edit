//! 地图列表与预览（地图查看器）
//!
//! 左侧以文字列表展示全部地图（短编号 - 名称），右侧预览地图图片。
//! 地图列表与名称来自数据表 ContentFinderCondition（ShortCode / Name）；
//! 地图图片文件为 `ui/map/{code}/{code}_m.tex`（medium 尺寸），并与
//! `{code}m_m.tex` 掩码做 MultiplyBlend 混合（参考 SaintCoinach Map.cs）。

use std::collections::{HashMap, HashSet};

use eframe::egui;
use image::RgbaImage;
use ironworks::excel::Language;

use crate::backend::Backend;
use crate::excel::provider::{ExcelProvider, ExcelSheet};
use crate::sheet::{GlobalContext, TableContext};
use crate::utils::{CollapsibleSidePanel, Side, TrackedPromise};

/// 地图列表项
#[derive(Clone, Default)]
pub struct MapRow {
    /// 短编号（地图文件名/目录名）
    pub code: String,
    /// 副本名称（可能为空）
    pub name: String,
    /// ContentFinderCondition 行号
    pub row_id: u32,
    /// 基本信息：(标签, 值) 列表
    pub info: Vec<(String, String)>,
}

enum LoadState {
    Loading,
    Ready,
    Error(String),
}

/// 地图图片加载结果
#[derive(Clone)]
enum MapImage {
    Loading,
    Failed(String),
    Loaded(egui::TextureHandle),
}

pub struct MapViewer {
    pub rows: Vec<MapRow>,
    pub search: String,
    pub show_new_only: bool,
    pub new_codes: HashSet<String>,
    pub selected: Option<usize>,
    load_state: LoadState,
    /// 图片缓存：短编号 -> 纹理
    images: HashMap<String, MapImage>,
    /// 进行中的图片加载 promise
    image_promises: HashMap<String, TrackedPromise<anyhow::Result<RgbaImage>>>,
}

impl Default for MapViewer {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            search: String::new(),
            show_new_only: false,
            new_codes: HashSet::new(),
            selected: None,
            load_state: LoadState::Loading,
            images: HashMap::new(),
            image_promises: HashMap::new(),
        }
    }
}

/// 事件：由 App 处理保存等动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapEvent {
    /// 点击左侧某个地图
    Select(usize),
    /// 保存当前地图
    ExportCurrent,
    /// 保存全部地图
    ExportAll,
}

/// 列表是否已加载完成
pub fn is_ready(viewer: &MapViewer) -> bool {
    matches!(viewer.load_state, LoadState::Ready)
}

/// 列表加载失败信息（用于导出时提示）
pub fn load_error(viewer: &MapViewer) -> Option<&str> {
    match &viewer.load_state {
        LoadState::Error(e) => Some(e),
        _ => None,
    }
}

impl MapViewer {
    /// 触发地图列表加载（首次或失败重试时调用）
    pub fn request_load(&mut self) {
        if !matches!(self.load_state, LoadState::Loading) {
            self.load_state = LoadState::Loading;
        }
    }

    fn poll_load(&mut self, backend: &Backend, lang: Language) {
        if !matches!(self.load_state, LoadState::Loading) {
            return;
        }
        let backend_for_promise = backend.clone();
        let promise = TrackedPromise::spawn_local(async move {
            load_map_rows(&backend_for_promise, lang).await
        });
        // spawn_local 的任务在同一次 tick 内执行到第一个挂起点；
        // 单表读取量小，等待其完成
        if let Some(result) = promise.try_get() {
            match result {
                Ok(rows) => {
                    self.rows = rows.clone();
                    self.load_state = LoadState::Ready;
                    self.after_load(backend.clone());
                }
                Err(e) => {
                    self.load_state = LoadState::Error(e.to_string());
                }
            }
        } else {
            // 未完成：保留当前状态，下一帧继续（poll_load 每帧调用）
            self.load_state = LoadState::Loading;
            self.image_promises.clear();
        }
    }

    fn after_load(&mut self, backend: Backend) {
        // 保存地图列表到版本文件并对比新增项（复用 list_tracker）
        let version = backend.game_version().unwrap_or("local").to_string();
        let safe_ver = version.replace('.', "_");
        let items: Vec<String> = self.rows.iter().map(|r| r.code.clone()).collect();
        let list = crate::list_tracker::VersionedList {
            version: version.clone(),
            items,
        };
        let cur_path = std::path::PathBuf::from("config").join(format!("map_list_{safe_ver}.json"));
        crate::list_tracker::save_list(&cur_path, &list);
        if let Some(prev_path) = Self::find_prev_list(&safe_ver) {
            if let Some(prev) = crate::list_tracker::load_list(&prev_path) {
                if let crate::list_tracker::ComparisonResult::NewItems(items) =
                    crate::list_tracker::compare_lists(&version, &list, Some(&prev))
                {
                    self.new_codes = items.into_iter().collect();
                }
            }
        }
        crate::list_tracker::cleanup_lists("map_list_", &version);
    }

    fn find_prev_list(current_safe: &str) -> Option<std::path::PathBuf> {
        let dir = std::path::PathBuf::from("config");
        let Ok(entries) = std::fs::read_dir(&dir) else { return None };
        entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| name.starts_with("map_list_") && !name.contains(current_safe))
            })
            .max_by_key(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            })
    }

    /// 主要 UI：左侧列表 + 右侧预览
    pub fn ui(&mut self, ui: &mut egui::Ui, backend: &Backend, lang: Language) -> Option<MapEvent> {
        self.poll_load(backend, lang);

        let mut event = None;

        // ── 左侧列表 ──
        CollapsibleSidePanel::new("map_list", Side::Left).show(ui, |ui, is_open| {
            if !is_open {
                return;
            }
                ui.add_space(4.0);
                // 筛选文本框
                ui.text_edit_singleline(&mut self.search)
                    .on_hover_text("筛选地图");
                // 仅显示新增项
                ui.horizontal(|ui| {
                    let new_count = self.new_codes.len();
                    ui.checkbox(&mut self.show_new_only, "仅显示新增项")
                        .on_hover_text(if new_count > 0 {
                            format!("当前版本新增 {new_count} 个地图")
                        } else {
                            "当前版本无新增地图".into()
                        });
                });
                ui.add_space(4.0);

                match &self.load_state {
                    LoadState::Loading => {
                        ui.spinner();
                        ui.label("正在加载地图列表...");
                    }
                    LoadState::Error(e) => {
                        ui.colored_label(
                            egui::Color32::RED,
                            "读取ContentFinderCondition中的数据失败，无法获取到地图基本信息！",
                        );
                        ui.label(egui::RichText::new(e).small().color(egui::Color32::GRAY));
                        if ui.button("重试").clicked() {
                            self.request_load();
                        }
                    }
                    LoadState::Ready => {
                        let query = self.search.trim().to_lowercase();
                        let filtered: Vec<usize> = self
                            .rows
                            .iter()
                            .enumerate()
                            .filter(|(_, row)| {
                                if self.show_new_only && !self.new_codes.contains(&row.code) {
                                    return false;
                                }
                                if query.is_empty() {
                                    return true;
                                }
                                row.code.to_lowercase().contains(&query)
                                    || row.name.to_lowercase().contains(&query)
                            })
                            .map(|(i, _)| i)
                            .collect();

                        if filtered.is_empty() {
                            ui.label("没有匹配的地图");
                        }
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for idx in filtered {
                                    let row = &self.rows[idx];
                                    let label = if row.name.is_empty() {
                                        row.code.clone()
                                    } else {
                                        format!("{} - {}", row.code, row.name)
                                    };
                                    let selected = self.selected == Some(idx);
                                    if ui.selectable_label(selected, &label).clicked() {
                                        self.selected = Some(idx);
                                        event = Some(MapEvent::Select(idx));
                                    }
                                }
                            });
                    }
                }
            });

        // ── 右侧预览 ──
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(e) = self.draw_preview(ui, backend) {
                event = Some(e);
            }
        });

        event
    }

    fn draw_preview(&mut self, ui: &mut egui::Ui, backend: &Backend) -> Option<MapEvent> {
        let mut event = None;
        let Some(idx) = self.selected else {
            ui.centered_and_justified(|ui| {
                ui.label("在左侧选择地图以预览");
            });
            return None;
        };
        let Some(row) = self.rows.get(idx) else {
            return None;
        };
        let code = row.code.clone();
        let name = row.name.clone();

        ui.add_space(8.0);
        // 上方：副本名称（大字体居中）
        if !name.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(&name).size(24.0).strong()).wrap());
            });
            ui.add_space(6.0);
        }

        // 基本信息（来自 ContentFinderCondition）
        if !row.info.is_empty() {
            egui::Frame::group(ui.style())
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (label, value) in &row.info {
                            ui.label(egui::RichText::new(format!("{label}：")).strong());
                            ui.label(value);
                            ui.add_space(12.0);
                        }
                    });
                });
            ui.add_space(6.0);
        }

        // 地图图片（居中，大一些）
        ui.vertical_centered(|ui| {
            let available = ui.available_width().min(ui.available_height()).max(100.0);
            match self.get_or_load_image(ui, &code, backend) {
                MapImage::Loaded(tex) => {
                    let (w, h) = (tex.size()[0] as f32, tex.size()[1] as f32);
                    let aspect = h / w.max(1.0);
                    let target_w = available.min(800.0);
                    let target_h = (target_w * aspect).clamp(100.0, available.min(800.0));
                    ui.add(
                        egui::Image::new(egui::load::SizedTexture::from_handle(&tex))
                            .fit_to_exact_size(egui::vec2(target_w, target_h)),
                    );
                }
                MapImage::Loading => {
                    ui.spinner();
                    ui.label("正在加载地图图片...");
                }
                MapImage::Failed(e) => {
                    ui.colored_label(egui::Color32::RED, format!("加载地图图片失败: {e}"));
                }
            }
            ui.add_space(8.0);
            // 保存当前地图按钮
            if ui.button("保存地图").clicked() {
                event = Some(MapEvent::ExportCurrent);
            }
        });

        event
    }

    fn get_or_load_image(
        &mut self,
        ui: &mut egui::Ui,
        code: &str,
        backend: &Backend,
    ) -> MapImage {
        // 已缓存：克隆 TextureHandle（轻量 Arc）
        if let Some(MapImage::Loaded(tex)) = self.images.get(code) {
            return MapImage::Loaded(tex.clone());
        }
        if let Some(MapImage::Failed(e)) = self.images.get(code) {
            return MapImage::Failed(e.clone());
        }
        // 检查进行中的 promise 是否完成
        let mut done: Option<MapImage> = None;
        if let Some(promise) = self.image_promises.get_mut(code) {
            if let Some(result) = promise.try_get() {
                let entry = match result {
                    Ok(img) => {
                        let size = [img.width() as usize, img.height() as usize];
                        let color_image =
                            egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                        MapImage::Loaded(ui.ctx().load_texture(
                            format!("map_{code}"),
                            color_image,
                            egui::TextureOptions::LINEAR,
                        ))
                    }
                    Err(e) => MapImage::Failed(e.to_string()),
                };
                done = Some(entry);
            }
        }
        if let Some(entry) = done {
            self.image_promises.remove(code);
            self.images.insert(code.to_string(), entry.clone());
            return entry;
        }
        // 未开始：启动加载
        if !self.image_promises.contains_key(code) {
            let files = backend.files().clone();
            let code_owned = code.to_string();
            let promise = TrackedPromise::spawn_local(async move {
                load_map_texture(&*files, &code_owned).await
            });
            self.image_promises.insert(code.to_string(), promise);
        }
        MapImage::Loading
    }
}

/// 读取地图列表（ContentFinderCondition 表）。
/// 读取失败返回 Err（此时调用方回退为仅显示文件名列表）。
async fn load_map_rows(backend: &Backend, lang: Language) -> anyhow::Result<Vec<MapRow>> {
    let excel = backend.excel().clone();
    let sheet = excel.get_sheet("ContentFinderCondition", lang).await?;

    // 真实 schema（yml）
    let editable =
        crate::config_file::load_schema_for_export(backend, "ContentFinderCondition").await;
    let schema = editable.as_ref().and_then(|e| e.get_schema());
    let context = TableContext::new(
        GlobalContext::new(
            egui::Context::default(),
            backend.clone(),
            lang,
            crate::utils::IconManager::new(),
        ),
        sheet,
        schema,
    );

    // 需要读取的列（按 column_layout.json 中 ContentFinderCondition 配置，
    // 去掉 副本名称(Name) 和 图片(Image)，短编号(ShortCode) 前加 编号=行号）
    let col_labels: &[(&str, &str)] = &[
        ("ShortCode", "短编号"),
        ("ContentType", "类型"),
        ("RequiredExVersion", "资料片"),
        ("Transient", "描述"),
        ("AcceptClassJobCategory", "限制职能"),
        ("ClassJobLevelRequired", "最低入场等级"),
        ("ClassJobLevelSync", "等级同步"),
        ("ItemLevelRequired", "最低入场装等"),
        ("ItemLevelSync", "装等同步"),
        ("AllowUndersized", "允许解限"),
    ];

    // 列名 -> offset
    let mut col_offsets: HashMap<String, u32> = HashMap::new();
    let col_count = context.column_count();
    for ci in 0..col_count {
        if let Ok(((sc, shc), _)) = context.get_column_by_index(ci as u32) {
            let name = sc.name().to_string();
            if col_labels.iter().any(|(n, _)| *n == name) || name == "Name" {
                col_offsets.insert(name, shc.offset() as u32);
            }
        }
    }

    let mut rows = Vec::new();
    for row_id in context.sheet().get_row_ids() {
        let Ok(row) = context.sheet().get_row(row_id) else {
            continue;
        };
        // 短编号
        let code = col_offsets
            .get("ShortCode")
            .and_then(|&off| context.cell_by_offset(row, off).ok())
            .map(|c| c.value_string())
            .unwrap_or_default();
        if code.is_empty() {
            continue;
        }
        // 名称
        let name = col_offsets
            .get("Name")
            .and_then(|&off| context.cell_by_offset(row, off).ok())
            .map(|c| c.value_string())
            .unwrap_or_default();
        // 基本信息
        let mut info = Vec::new();
        info.push(("编号".to_string(), row_id.to_string()));
        for (col_name, label) in col_labels {
            if let Some(&off) = col_offsets.get(*col_name) {
                let val = context
                    .cell_by_offset(row, off)
                    .map(|c| c.value_string())
                    .unwrap_or_default();
                info.push((label.to_string(), val));
            }
        }
        rows.push(MapRow {
            code,
            name,
            row_id,
            info,
        });
    }
    rows.sort_by(|a, b| a.code.cmp(&b.code));
    Ok(rows)
}

/// 读取地图图片（medium 尺寸 + 掩码 MultiplyBlend，参考 SaintCoinach Map.cs）
pub async fn load_map_texture(
    files: &dyn crate::data::FileProvider,
    code: &str,
) -> anyhow::Result<RgbaImage> {
    let file_name = code.replace('/', "");
    let path = format!("ui/map/{code}/{file_name}_m.tex");
    let image = files.read_tex(&path).await?;

    // 掩码：ui/map/{code}/{file_name}m_m.tex（存在则 MultiplyBlend）
    let mask_path = format!("ui/map/{code}/{file_name}m_m.tex");
    if let Ok(mask) = files.read_tex(&mask_path).await {
        if image.dimensions() == mask.dimensions() {
            let (w, h) = image.dimensions();
            let a = image.as_raw();
            let b = mask.as_raw();
            let mut result = a.clone();
            // 与 SaintCoinach 一致：mask alpha==0 时保留原像素，否则 RGB 相乘、保留 alpha
            for i in (0..result.len()).step_by(4) {
                let b_alpha = b[i + 3];
                if b_alpha != 0 {
                    result[i] = ((a[i] as u32 * b[i] as u32) / 255) as u8;
                    result[i + 1] = ((a[i + 1] as u32 * b[i + 1] as u32) / 255) as u8;
                    result[i + 2] = ((a[i + 2] as u32 * b[i + 2] as u32) / 255) as u8;
                }
                // result[i+3] 保留原 alpha
            }
            return Ok(RgbaImage::from_raw(w, h, result)
                .ok_or_else(|| anyhow::anyhow!("无法构建地图图像"))?);
        }
    }
    Ok(image)
}

/// 导出文件名：`{code} - {name}.png`（无名称时仅 `{code}.png`）
pub fn map_file_name(row: &MapRow) -> String {
    let safe_name: String = row
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' || c == '（' || c == '）' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe_name = safe_name.trim();
    if safe_name.is_empty() {
        format!("{}.png", row.code)
    } else {
        format!("{} - {}.png", row.code, safe_name)
    }
}
