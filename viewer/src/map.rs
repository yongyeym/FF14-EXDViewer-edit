//! 地图列表与预览（地图查看器）
//!
//! 左侧以文字列表展示全部地图（短编号 - 地图名），右侧预览地图图片。
//! 短编号来自 Map 表的 Id 列（如 w1d3/03、w1d3_1、w1d3_re），显示时斜杠改为下划线；
//! 名称来自 PlaceName 表（经 Map.PlaceName 链接），基本信息尽力从
//! ContentFinderCondition 表按 TerritoryType 关联补充。
//! 地图图片文件为 `ui/map/{id}/{file_name}_m.tex`（medium 尺寸），并与
//! `{file_name}m_m.tex` 掩码做 MultiplyBlend 混合（参考 SaintCoinach Map.cs）。
//! 同一基础短编号的多张分图（如 w1d3_1、w1d3_2）合并为一项，右侧并排展示。

use std::collections::{HashMap, HashSet};

use eframe::egui;
use egui::{Align, Button, Layout, RichText, TextEdit, Vec2, containers::panel::Panel};
use image::RgbaImage;
use ironworks::excel::Language;

use crate::backend::Backend;
use crate::excel::provider::{ExcelProvider, ExcelSheet};
use crate::sheet::{GlobalContext, TableContext};
use crate::utils::{CollapsibleSidePanel, Side, TrackedPromise};

/// 单张地图（分图）
#[derive(Clone, Default)]
pub struct MapSub {
    /// 完整短编号（含分图后缀，如 w1d3_1）
    pub code: String,
    /// 显示用短编号（斜杠改为下划线）
    pub display: String,
}

/// 地图列表项（按基础短编号分组，含一张或多张分图）
#[derive(Clone, Default)]
pub struct MapRow {
    /// 基础短编号（分图共用的部分，如 w1d3、w1d3/03）
    pub base_code: String,
    /// 显示用基础短编号（斜杠改为下划线）
    pub display: String,
    /// 地图中文名（来自 PlaceName）
    pub name: String,
    /// 二级地图名（来自 Map.PlaceNameSub，可能为空）
    pub name_sub: String,
    /// ContentFinderCondition 行号（关联到则用，否则用 Map 行号）
    pub row_id: u32,
    /// 基本信息：(标签, 值) 列表
    pub info: Vec<(String, String)>,
    /// 分图列表（至少 1 张）
    pub maps: Vec<MapSub>,
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
    /// 图片缓存：完整短编号 -> 纹理
    images: HashMap<String, MapImage>,
    /// 进行中的图片加载 promise
    image_promises: HashMap<String, TrackedPromise<anyhow::Result<RgbaImage>>>,
    /// 地图列表加载 promise（跨帧复用）
    load_promise: Option<TrackedPromise<anyhow::Result<Vec<MapRow>>>>,
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
            load_promise: None,
        }
    }
}

/// 事件：由 App 处理保存等动作
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapEvent {
    /// 点击左侧某个地图
    Select(usize),
    /// 保存指定分图（弹出文件选择器）
    ExportOne(String),
    /// 复制指定分图到系统剪贴板
    CopyOne(String),
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
        self.load_promise = None;
        if !matches!(self.load_state, LoadState::Loading) {
            self.load_state = LoadState::Loading;
        }
    }

    fn poll_load(&mut self, backend: &Backend, lang: Language) {
        if !matches!(self.load_state, LoadState::Loading) {
            return;
        }
        // 只在没有进行中的加载任务时启动（不能每帧重建 promise）
        if self.load_promise.is_none() {
            let backend_for_promise = backend.clone();
            self.load_promise = Some(TrackedPromise::spawn_local(async move {
                load_map_rows(&backend_for_promise, lang).await
            }));
        }
        // 每帧检查同一个 promise 是否完成
        if let Some(promise) = self.load_promise.as_mut() {
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
                self.load_promise = None;
            }
        }
    }

    fn after_load(&mut self, backend: Backend) {
        // 保存地图列表到版本文件并对比新增项（复用 list_tracker）
        let version = backend.game_version().unwrap_or("local").to_string();
        let safe_ver = version.replace('.', "_");
        let items: Vec<String> = self.rows.iter().map(|r| r.display.clone()).collect();
        let list = crate::list_tracker::VersionedList {
            version: version.clone(),
            items,
        };
        let cur_path = std::path::PathBuf::from("config").join(format!("map_list_{safe_ver}.json"));
        crate::list_tracker::save_list(&cur_path, &list);
        // 对比并归档：从最近旧版本逐个往前对比找有变动的版本，其余旧 json 移到 config/bak/map_list/
        if let crate::list_tracker::ComparisonResult::NewItems(items) =
            crate::list_tracker::compare_and_archive(&version, &list, "map_list")
        {
            self.new_codes = items.into_iter().collect();
        }
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
        CollapsibleSidePanel::new("map_list", Side::Left)
            .default_width(300.0)
            .show(ui, |ui, is_open| {
            if !is_open {
                return;
            }
            Panel::top("map_list_header").show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        CollapsibleSidePanel::draw_arrow(ui, "map_list");
                        ui.vertical_centered_justified(|ui| ui.heading("地图"));
                    });
                });
                ui.add_space(4.0);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    if ui
                        .add_enabled(!self.search.is_empty(), Button::new("↩"))
                        .on_hover_text("清除")
                        .clicked()
                    {
                        self.search.clear();
                    }
                    // 仅显示新增项（单符号按钮，始终显示；无新增时禁用）
                    let new_count = self.new_codes.len();
                    if new_count > 0 {
                        ui.toggle_value(&mut self.show_new_only, "🔍")
                            .on_hover_text(format!("仅显示新增项（{new_count} 项）"));
                    } else {
                        ui.add_enabled(false, Button::new("🔍"))
                            .on_hover_text("当前版本无新增地图");
                    }
                    ui.add_sized(
                        Vec2::new(ui.available_width(), 0.0),
                        TextEdit::singleline(&mut self.search).hint_text("筛选"),
                    );
                });
                ui.add_space(4.0);
            });

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
                    ui.label(RichText::new(e).small().color(egui::Color32::GRAY));
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
                            if self.show_new_only && !self.new_codes.contains(&row.display) {
                                return false;
                            }
                            if query.is_empty() {
                                return true;
                            }
                            row.display.to_lowercase().contains(&query)
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
                                    row.display.clone()
                                } else if row.name_sub.is_empty() {
                                    format!("{} - {}", row.display, row.name)
                                } else {
                                    format!("{} - {} - {}", row.display, row.name, row.name_sub)
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
            // 折叠后左上角提供重新展开按钮（与其他列表一致）
            if CollapsibleSidePanel::is_collapsed(ui.ctx(), "map_list") {
                Panel::top("map_reexpand").show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| CollapsibleSidePanel::draw_arrow(ui, "map_list"));
                    ui.add_space(4.0);
                });
            }
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
        let name = row.name.clone();
        let maps = row.maps.clone();

        // ── 最上方：地图中文名（标题文本，居中）；有二级名则第二行小字号显示 ──
        if !name.is_empty() {
            ui.add_space(6.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Label::new(RichText::new(&name).size(24.0).strong()).wrap());
                if !row.name_sub.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(&row.name_sub).size(16.0))
                            .wrap(),
                    );
                }
            });
            ui.add_space(6.0);
        }

        // ── 基本信息：多行多列表格（不显示描述），整体居中 ──
        if !row.info.is_empty() {
            // 外层 horizontal 让 Frame 宽度收缩到内容（避免信息少时右侧留空）
            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    egui::Frame::group(ui.style())
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            // 每对(标签,值)为一项，内容宽度自适应并居中，超宽自动换行
                            ui.horizontal_wrapped(|ui| {
                                for (label, value) in &row.info {
                                    ui.horizontal(|ui| {
                                        // 标签用强调色，与内容值区分
                                        let accent = ui.visuals().hyperlink_color;
                                        ui.label(
                                            RichText::new(format!("{label}："))
                                                .strong()
                                                .color(accent),
                                        );
                                        ui.label(value);
                                    });
                                    ui.add_space(16.0);
                                }
                            });
                        });
                });
            });
            ui.add_space(6.0);
        }

        // ── 地图图片：等大展示，超出范围滚动 ──
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // 动态列数：每张图 800px 宽 + 间距，按可视宽度决定一行几张（最多4张）
                let avail_w = ui.available_width();
                let per_row = ((avail_w / 820.0).floor() as usize).clamp(1, 4);
                let per_row = per_row.max(1);
                for chunk in maps.chunks(per_row) {
                    ui.horizontal(|ui| {
                        for sub in chunk {
                            ui.vertical(|ui| {
                                ui.vertical_centered(|ui| {
                                // 统一缩放为高 800px：大的等比缩小，小的等比放大
                                match self.get_or_load_image(ui, &sub.code, backend) {
                                    MapImage::Loaded(tex) => {
                                        let (tw, th) = (tex.size()[0] as f32, tex.size()[1] as f32);
                                        let aspect = tw / th.max(1.0);
                                        let w = 800.0 * aspect;
                                        // SizedTexture::new 强制尺寸 + fit_to_exact_size 双保险
                                        let st = egui::load::SizedTexture::new(
                                            tex.id(),
                                            Vec2::new(w, 800.0),
                                        );
                                        ui.add(
                                            egui::Image::new(st)
                                                .fit_to_exact_size(Vec2::new(w, 800.0)),
                                        );
                                    }
                                    MapImage::Loading => {
                                        ui.spinner();
                                        ui.label("加载中...");
                                    }
                                    MapImage::Failed(e) => {
                                        ui.colored_label(
                                            egui::Color32::RED,
                                            format!("加载失败: {e}"),
                                        );
                                    }
                                }
                                // 下方：短编号 + 复制/保存按钮（居中，两个按钮间留间距）
                                ui.label(RichText::new(&sub.display).strong());
                                ui.horizontal(|ui| {
                                    if ui.button("复制此地图图片").clicked() {
                                        event = Some(MapEvent::CopyOne(sub.code.clone()));
                                    }
                                    ui.add_space(8.0);
                                    if ui.button("保存此地图图片文件").clicked() {
                                        event = Some(MapEvent::ExportOne(sub.code.clone()));
                                    }
                                });
                                });
                            });
                        }
                    });
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

/// 显示用短编号：斜杠改为下划线
pub fn display_code(code: &str) -> String {
    code.replace('/', "_")
}

/// 基础短编号：去掉末尾 `_数字` 后缀（分图归组用）
fn base_code(code: &str) -> String {
    let idx = code.rfind('_');
    if let Some(i) = idx {
        let suffix = &code[i + 1..];
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
            return code[..i].to_string();
        }
    }
    code.to_string()
}

/// 链接表缓存（ContentType/ExVersion/ClassJobCategory 的行号→Name 映射）。
/// 这些表长时间才变动一次，缓存到 config/map_links_{version}.json，版本号变动才重新读取。
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct MapLinks {
    #[serde(default)]
    content_type: HashMap<String, String>,
    #[serde(default)]
    ex_version: HashMap<String, String>,
    #[serde(default)]
    class_job: HashMap<String, String>,
}

fn map_links_cache_path(version: &str) -> std::path::PathBuf {
    let safe = version.replace('.', "_");
    std::path::PathBuf::from("config").join(format!("map_links_{safe}.json"))
}

/// 读取一个表的 行号→Name 映射
async fn read_name_map(
    backend: &Backend,
    lang: Language,
    sheet_name: &str,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(sheet) = backend.excel().get_sheet(sheet_name, lang).await {
        let editable = crate::config_file::load_schema_for_export(backend, sheet_name).await;
        let schema = editable.as_ref().and_then(|e| e.get_schema());
        let ctx = TableContext::new(
            GlobalContext::new(
                egui::Context::default(),
                backend.clone(),
                lang,
                crate::utils::IconManager::new(),
            ),
            sheet,
            schema,
        );
        let mut name_ci: Option<u32> = None;
        for ci in 0..ctx.column_count() {
            if let Ok(((sc, _), offset_idx)) = ctx.get_column_by_index(ci as u32) {
                if sc.name() == "Name" {
                    name_ci = Some(offset_idx);
                    break;
                }
            }
        }
        if let Some(ci) = name_ci {
            for row_id in ctx.sheet().get_row_ids() {
                if let Ok(row) = ctx.sheet().get_row(row_id) {
                    if let Ok(cell) = ctx.cell_by_offset(row, ci) {
                        let v = cell.value_string();
                        if !v.is_empty() {
                            map.insert(row_id.to_string(), v);
                        }
                    }
                }
            }
        }
    }
    map
}

/// 读取链接表数据：优先读缓存（版本一致），否则读取游戏表并保存缓存
async fn load_map_links(backend: &Backend, lang: Language, version: &str) -> MapLinks {
    let cache_path = map_links_cache_path(version);
    if let Ok(s) = std::fs::read_to_string(&cache_path) {
        if let Ok(links) = serde_json::from_str::<MapLinks>(&s) {
            log::debug!("地图链接表缓存命中: {}", cache_path.display());
            return links;
        }
    }
    log::info!("读取地图链接表（ContentType/ExVersion/ClassJobCategory）...");
    let links = MapLinks {
        content_type: read_name_map(backend, lang, "ContentType").await,
        ex_version: read_name_map(backend, lang, "ExVersion").await,
        class_job: read_name_map(backend, lang, "ClassJobCategory").await,
    };
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(&links) {
        let _ = std::fs::write(&cache_path, json);
        log::info!("地图链接表已缓存: {}", cache_path.display());
        // 旧版本 map_links 直接删除，只保留最新版（数据随版本更新，无需归档）
        cleanup_map_links(version);
    }
    links
}

/// 删除 config 目录下除当前版本外的全部 map_links_*.json（旧版本直接删除，不归档）
fn cleanup_map_links(current_version: &str) {
    let safe_cur = current_version.replace('.', "_");
    let target = format!("map_links_{safe_cur}.json");
    if let Ok(entries) = std::fs::read_dir(std::path::PathBuf::from("config")) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("map_links_") && name.ends_with(".json") && name != target {
                let _ = std::fs::remove_file(e.path());
                log::debug!("已删除旧版本地图链接缓存: {name}");
            }
        }
    }
}

/// 读取地图列表。
/// 短编号来自 Map 表的 Id 列；名称来自 PlaceName 表（经 Map.PlaceName 链接）；
/// 基本信息尽力从 ContentFinderCondition 表按 TerritoryType 关联补充。
async fn load_map_rows(backend: &Backend, lang: Language) -> anyhow::Result<Vec<MapRow>> {
    let excel = backend.excel().clone();
    // 链接表数据（ContentType/ExVersion/ClassJobCategory 缓存 + 描述每次读取）
    let version = backend.game_version().unwrap_or("local").to_string();
    let links = load_map_links(backend, lang, &version).await;

    // ── 读取 Map 表 ──
    let map_sheet = excel.get_sheet("Map", lang).await?;
    let map_editable = crate::config_file::load_schema_for_export(backend, "Map").await;
    let map_schema = map_editable.as_ref().and_then(|e| e.get_schema());
    let map_ctx = TableContext::new(
        GlobalContext::new(
            egui::Context::default(),
            backend.clone(),
            lang,
            crate::utils::IconManager::new(),
        ),
        map_sheet,
        map_schema,
    );
    let mut map_id_ci: Option<u32> = None;
    let mut map_tt_ci: Option<u32> = None;
    let mut map_place_ci: Option<u32> = None;
    let mut map_place_sub_ci: Option<u32> = None;
    for ci in 0..map_ctx.column_count() {
        if let Ok(((sc, _), offset_idx)) = map_ctx.get_column_by_index(ci as u32) {
            match sc.name() {
                "Id" => map_id_ci = Some(offset_idx),
                "TerritoryType" => map_tt_ci = Some(offset_idx),
                "PlaceName" => map_place_ci = Some(offset_idx),
                "PlaceNameSub" => map_place_sub_ci = Some(offset_idx),
                _ => {}
            }
        }
    }
    let Some(map_id_ci) = map_id_ci else {
        anyhow::bail!("Map 表未找到 Id 列");
    };

    // ── 读取 PlaceName 表（地图中文名）──
    let mut place_names: HashMap<String, String> = HashMap::new();
    if let Ok(place_sheet) = excel.get_sheet("PlaceName", lang).await {
        let place_editable = crate::config_file::load_schema_for_export(backend, "PlaceName").await;
        let place_schema = place_editable.as_ref().and_then(|e| e.get_schema());
        let place_ctx = TableContext::new(
            GlobalContext::new(
                egui::Context::default(),
                backend.clone(),
                lang,
                crate::utils::IconManager::new(),
            ),
            place_sheet,
            place_schema,
        );
        let mut place_name_ci: Option<u32> = None;
        for ci in 0..place_ctx.column_count() {
            if let Ok(((sc, _), offset_idx)) = place_ctx.get_column_by_index(ci as u32) {
                if sc.name() == "Name" {
                    place_name_ci = Some(offset_idx);
                    break;
                }
            }
        }
        if let Some(off) = place_name_ci {
            for row_id in place_ctx.sheet().get_row_ids() {
                if let Ok(row) = place_ctx.sheet().get_row(row_id) {
                    if let Ok(cell) = place_ctx.cell_by_offset(row, off) {
                        let name = cell.value_string();
                        if !name.is_empty() {
                            place_names.insert(row_id.to_string(), name);
                        }
                    }
                }
            }
        }
    }

    // ── 读取 ContentFinderCondition 表（基本信息，尽力关联）──
    let cfc_sheet = excel.get_sheet("ContentFinderCondition", lang).await?;
    let cfc_editable =
        crate::config_file::load_schema_for_export(backend, "ContentFinderCondition").await;
    let cfc_schema = cfc_editable.as_ref().and_then(|e| e.get_schema());
    let cfc_ctx = TableContext::new(
        GlobalContext::new(
            egui::Context::default(),
            backend.clone(),
            lang,
            crate::utils::IconManager::new(),
        ),
        cfc_sheet,
        cfc_schema,
    );
    let col_labels: &[(&str, &str)] = &[
        ("ContentType", "类型"),
        ("RequiredExVersion", "资料片"),
        ("AcceptClassJobCategory", "限制职能"),
        ("ClassJobLevelRequired", "最低入场等级"),
        ("ClassJobLevelSync", "等级同步"),
        ("ItemLevelRequired", "最低入场装等"),
        ("ItemLevelSync", "装等同步"),
        ("AllowUndersized", "允许解限"),
    ];
    let mut cfc_col_offsets: HashMap<String, u32> = HashMap::new();
    let mut cfc_tt_ci: Option<u32> = None;
    for ci in 0..cfc_ctx.column_count() {
        if let Ok(((sc, _), offset_idx)) = cfc_ctx.get_column_by_index(ci as u32) {
            let name = sc.name().to_string();
            if name == "TerritoryType" {
                cfc_tt_ci = Some(offset_idx);
            } else if col_labels.iter().any(|(n, _)| *n == name) {
                cfc_col_offsets.insert(name, offset_idx);
            }
        }
    }
    // CFC: TerritoryType -> (row_id, info)
    let mut cfc_by_tt: HashMap<String, (u32, Vec<(String, String)>)> = HashMap::new();
    if let Some(tt_off) = cfc_tt_ci {
        for row_id in cfc_ctx.sheet().get_row_ids() {
            let Ok(row) = cfc_ctx.sheet().get_row(row_id) else { continue };
            let tt = cfc_ctx
                .cell_by_offset(row, tt_off)
                .map(|c| c.value_string())
                .unwrap_or_default();
            if tt.is_empty() {
                continue;
            }
            let mut info = Vec::new();
            info.push(("编号".to_string(), row_id.to_string()));
            for (col_name, label) in col_labels {
                if let Some(&off) = cfc_col_offsets.get(*col_name) {
                    let val = cfc_ctx
                        .cell_by_offset(row, off)
                        .map(|c| c.value_string())
                        .unwrap_or_default();
                    // 链接列显示引用表的数据（而非原始行号）
                    let display = match *col_name {
                        "ContentType" => {
                            if val == "0" {
                                "地域地图".to_string()
                            } else {
                                links
                                    .content_type
                                    .get(&val)
                                    .cloned()
                                    .unwrap_or_else(|| val.clone())
                            }
                        }
                        "RequiredExVersion" => links
                            .ex_version
                            .get(&val)
                            .cloned()
                            .unwrap_or_else(|| val.clone()),
                        "AcceptClassJobCategory" => links
                            .class_job
                            .get(&val)
                            .cloned()
                            .unwrap_or_else(|| val.clone()),
                        // 等级/装等：0 显示"无限制"
                        "ClassJobLevelRequired" | "ClassJobLevelSync" | "ItemLevelRequired"
                        | "ItemLevelSync" => {
                            if val == "0" { "无限制".to_string() } else { val }
                        }
                        // 允许解限：true/1 → √，false/0 → ×
                        "AllowUndersized" => match val.as_str() {
                            "1" | "true" | "True" | "TRUE" => "√".to_string(),
                            "0" | "false" | "False" | "FALSE" => "×".to_string(),
                            _ => val,
                        },
                        _ => val,
                    };
                    info.push((label.to_string(), display));
                }
            }
            cfc_by_tt.entry(tt).or_insert((row_id, info));
        }
    }

    // ── 遍历 Map 表组装（按基础短编号分组）──
    struct RawMap {
        code: String,
        tt: String,
        place: String,
        place_sub: String,
        row_id: u32,
    }
    let mut raws: Vec<RawMap> = Vec::new();
    for row_id in map_ctx.sheet().get_row_ids() {
        let Ok(row) = map_ctx.sheet().get_row(row_id) else { continue };
        let code = map_ctx
            .cell_by_offset(row, map_id_ci)
            .map(|c| c.value_string())
            .unwrap_or_default();
        if code.is_empty() {
            continue;
        }
        let tt = map_tt_ci
            .and_then(|off| map_ctx.cell_by_offset(row, off).ok())
            .map(|c| c.value_string())
            .unwrap_or_default();
        let place = map_place_ci
            .and_then(|off| map_ctx.cell_by_offset(row, off).ok())
            .map(|c| c.value_string())
            .and_then(|id| place_names.get(&id).cloned())
            .unwrap_or_default();
        // 二级地图名（PlaceNameSub 链接 → PlaceName 表文本；0/空视为无）
        let place_sub = map_place_sub_ci
            .and_then(|off| map_ctx.cell_by_offset(row, off).ok())
            .map(|c| c.value_string())
            .filter(|id| !id.is_empty() && id != "0")
            .and_then(|id| place_names.get(&id).cloned())
            .unwrap_or_default();
        raws.push(RawMap {
            code,
            tt,
            place,
            place_sub,
            row_id,
        });
    }

    // 分组：base_code 相同合并
    let mut groups: HashMap<String, Vec<RawMap>> = HashMap::new();
    for raw in raws {
        groups.entry(base_code(&raw.code)).or_default().push(raw);
    }

    let mut rows: Vec<MapRow> = groups
        .into_iter()
        .map(|(base, mut raws)| {
            raws.sort_by(|a, b| a.code.cmp(&b.code));
            // 同一完整短编号的多行（Map表同Id的不同MapIndex变体）对应同一图片文件，去重只留一张
            raws.dedup_by(|a, b| a.code == b.code);
            // 先提取第一张分图的名称数据（避免借用冲突）
            let first_place = raws[0].place.clone();
            let first_sub = raws[0].place_sub.clone();
            let first_tt = raws[0].tt.clone();
            let first_row_id = raws[0].row_id;
            let name = first_place;
            let (cfc_row_id, mut info) = match cfc_by_tt.get(&first_tt) {
                Some(v) => v.clone(),
                None => (first_row_id, vec![("编号".to_string(), first_row_id.to_string())]),
            };
            // 统一：编号 之后是 短编号
            info.insert(1, ("短编号".to_string(), display_code(&base)));
            let maps: Vec<MapSub> = raws
                .into_iter()
                .map(|raw| MapSub {
                    display: display_code(&raw.code),
                    code: raw.code,
                })
                .collect();
            MapRow {
                display: display_code(&base),
                base_code: base,
                name,
                name_sub: first_sub,
                row_id: cfc_row_id,
                info,
                maps,
            }
        })
        .collect();
    // 按数据表行号排序（编号大小），编号相同时再按短编号二级排序
    rows.sort_by(|a, b| {
        a.row_id
            .cmp(&b.row_id)
            .then_with(|| a.display.cmp(&b.display))
    });
    Ok(rows)
}

/// 读取地图图片（medium 尺寸 + 掩码 MultiplyBlend，参考 SaintCoinach Map.cs）
/// code 为 Map 表原始 Id（可能含斜杠，如 w1d3/03）；路径用原始 id，文件名为去斜杠。
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
            let blended = RgbaImage::from_raw(w, h, result)
                .ok_or_else(|| anyhow::anyhow!("无法构建地图图像"))?;
            return Ok(trim_transparent(&blended));
        }
    }
    Ok(trim_transparent(&image))
}

/// 裁剪图片四周全透明像素，返回内容包围盒区域（2048 固定画布中居中/非方形内容放大展示用）
fn trim_transparent(img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let (mut min_x, mut min_y) = (w, h);
    let (mut max_x, mut max_y) = (0u32, 0u32);
    let mut found = false;
    for y in 0..h {
        for x in 0..w {
            if img.get_pixel(x, y)[3] > 0 {
                found = true;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    if !found {
        return img.clone();
    }
    let cw = max_x - min_x + 1;
    let ch = max_y - min_y + 1;
    image::imageops::crop_imm(img, min_x, min_y, cw, ch).to_image()
}

/// 导出文件名：`{显示短编号} - {名称}.png`；有二级地名则为
/// `{显示短编号} - {名称} - {二级地名}.png`（无名称时仅 `{显示短编号}.png`）
pub fn map_file_name(display: &str, name: &str, name_sub: &str) -> String {
    let safe = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' || c == '（' || c == '）' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>()
            .trim()
            .to_string()
    };
    let safe_name = safe(name);
    let safe_sub = safe(name_sub);
    if safe_name.is_empty() {
        format!("{display}.png")
    } else if safe_sub.is_empty() {
        format!("{display} - {safe_name}.png")
    } else {
        format!("{display} - {safe_name} - {safe_sub}.png")
    }
}
