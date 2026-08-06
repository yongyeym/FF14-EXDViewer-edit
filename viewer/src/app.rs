use std::{cell::OnceCell, collections::HashSet, io::Write, num::NonZero, rc::Rc, sync::Arc};

#[cfg(target_arch = "wasm32")]
use crate::utils::{PromiseKind, UnsendPromise};
use anyhow::Result;
use egui::{
    Button, CentralPanel, FontData, FontDefinitions, FontFamily, Layout, RichText, ScrollArea,
    TextEdit, Vec2, Widget,
    containers::{menu::MenuButton, panel::Panel},
    style::ScrollStyle,
};
use egui_extras::install_image_loaders;
use ironworks::excel::Language;
use ironworks::file::File;
use itertools::{EitherOrBoth, Itertools};
use lru::LruCache;
use matchit::Params;
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{
    about,
    backend::Backend,
    data::FileProviderExt,
    editable_schema::EditableSchema,
    excel::{
        base::BaseSheet,
        provider::{ExcelHeader, ExcelProvider, ExcelSheet},
    },
    github::CALLBACK_PATH,
    goto, music,
    pr_window::{self, PrAction, PrWindow},
    router::{Router, path::Path, route::RouteResponse},
    schema::{provider::SchemaProvider, web::WebProvider},
    settings::{
        ALWAYS_HIRES, BACKEND_CONFIG, BackendConfig, CODE_SYNTAX_THEME, COLOR_THEME,
        CURRENT_SHEET_LANGUAGES, DISPLAY_FIELD_SHOWN, EVALUATE_STRINGS, GithubSchemaBranch,
        ICON_SAVE_REQUEST, InstallLocation, LANGUAGE, LOGGER_SHOWN, MISC_SHEETS_SHOWN,
        PR_CHANGED_ONLY, SCHEMA_EDITOR_VISIBLE, SELECTED_SHEET, SHEET_FILTER_OPTIONS,
        SHEET_FILTERS, SHEETS_FILTER, SOLID_SCROLLBAR, SORTED_BY_OFFSET, SchemaLocation,
        TEMP_HIGHLIGHTED_ROW, TEMP_SCROLL_TO, TEXT_MAX_LINES, TEXT_USE_SCROLL, TEXT_WRAP_WIDTH,
    },
    setup::{self, SetupWindow},
    sheet::{
        CellResponse, FilterInputType, GlobalContext, MatchOptions, SheetTable, TableContext,
        export_csv,
    },
    shortcuts::{GOTO_ROW, GOTO_SHEET},
    utils::{
        CodeTheme, CollapsibleSidePanel, ColorTheme, ConvertiblePromise, FuzzyMatcher, GameVersion,
        IconManager, PromiseKind, Side, TrackedPromise, UnsendPromise, opt_slider, shortcut,
        tick_promises,
    },
};

type CachedSheetEntry = (
    Language, // language
    String,   // sheet name
);

/// Actions that can be triggered from the export menu.
enum ExportAction {
    /// Export a single sheet. (TableContext, resolve_display_field)
    Single(TableContext, bool),
    /// Export all sheets. (resolve_display_field)
    All(bool),
    /// Export favorited sheets. (resolve_display_field)
    Favorites(bool),
}

type CachedSheetPromise = TrackedPromise<Result<BaseSheet>>;
type ConvertibleSheetPromise = ConvertiblePromise<CachedSheetPromise, Result<SheetTable>>;

type CachedSchemaEntry = String; // sheet name

type CachedSchemaPromise = TrackedPromise<Option<Result<String>>>;
type ConvertibleSchemaPromise = ConvertiblePromise<CachedSchemaPromise, Result<EditableSchema>>;

type CachedLanguagesPromise = TrackedPromise<Result<Vec<Language>>>;
type ConvertibleLanguagesPromise =
    ConvertiblePromise<CachedLanguagesPromise, Result<Vec<Language>>>;

/// Fuzzy-matched sheet names (name + score) cached per (filter text, show-misc) key.
type SheetFilterData = LruCache<(String, bool), Rc<Vec<(String, i32)>>>;

/// Identifies which pull request a changed-schema set belongs to: (owner, repo, number).
type ChangedSchemasKey = (String, String, u32);

type CachedChangedSchemasPromise = TrackedPromise<Result<Vec<String>>>;
/// Converts to the set of PR-changed sheet names, or `None` if the fetch failed
/// (in which case the changed-only filter is treated as inactive).
type ConvertibleChangedSchemasPromise =
    ConvertiblePromise<CachedChangedSchemasPromise, Option<Rc<HashSet<String>>>>;

/// The state of the "changed schemas only" filter for the active schema source.
enum PrChangedState {
    /// The active schema source is not a pull request; the filter does not apply.
    NotPr,
    /// A pull request is active but its changed-file list is still loading.
    Pending,
    /// The changed-file list failed to load; the filter is inert (show everything).
    Failed,
    /// The set of sheet names the pull request changed.
    Ready(Rc<HashSet<String>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CjkFont {
    Japanese,
    Korean,
    ChineseSimplified,
    ChineseTraditional,
}

impl CjkFont {
    fn for_language(language: Language) -> Option<Self> {
        match language {
            Language::Korean => Some(Self::Korean),
            Language::ChineseSimplified => Some(Self::ChineseSimplified),
            Language::ChineseTraditional | Language::TaiwanChinese => {
                Some(Self::ChineseTraditional)
            }
            Language::Japanese
            | Language::None
            | Language::English
            | Language::German
            | Language::French => Some(Self::Japanese),
        }
    }

    fn family_name(self) -> &'static str {
        match self {
            Self::Japanese => "NotoSans-JP",
            Self::Korean => "NotoSans-KR",
            Self::ChineseSimplified => "NotoSans-SC",
            Self::ChineseTraditional => "NotoSans-TC",
        }
    }

    fn asset_file(self) -> &'static str {
        match self {
            Self::Japanese => "NotoSansJP-Regular.ttf",
            Self::Korean => "NotoSansKR-Regular.ttf",
            Self::ChineseSimplified => "NotoSansSC-Regular.ttf",
            Self::ChineseTraditional => "NotoSansTC-Regular.ttf",
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn embedded_bytes(self) -> &'static [u8] {
        match self {
            Self::Japanese => include_bytes!("../assets/NotoSansJP-Regular.ttf"),
            Self::Korean => include_bytes!("../assets/NotoSansKR-Regular.ttf"),
            Self::ChineseSimplified => include_bytes!("../assets/NotoSansSC-Regular.ttf"),
            Self::ChineseTraditional => include_bytes!("../assets/NotoSansTC-Regular.ttf"),
        }
    }
}


#[derive(Clone)]
pub struct ExportProgress {
    pub active: bool,
    pub title: String,
    pub current: usize,
    pub total: usize,
    pub current_name: String,
    pub done: bool,
    pub error: Option<String>,
    /// 完成时刻（用于成功后自动关闭窗口计时）
    pub done_at: Option<std::time::Instant>,
    /// 中断导出标志（窗口内“中断导出”按钮置位）
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Default for ExportProgress {
    fn default() -> Self {
        Self {
            active: false,
            title: String::new(),
            current: 0,
            total: 0,
            current_name: String::new(),
            done: false,
            error: None,
            done_at: None,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

pub type SharedExportProgress = std::sync::Arc<std::sync::Mutex<Option<ExportProgress>>>;

pub struct App {
    router: Rc<OnceCell<Router<Self>>>,
    icon_manager: IconManager,
    setup_window: Option<setup::SetupWindow>,
    backend: Option<Backend>,
    favorites: std::collections::HashSet<String>,
    sheet_data: LruCache<CachedSheetEntry, ConvertibleSheetPromise>,
    schema_data: LruCache<CachedSchemaEntry, ConvertibleSchemaPromise>,
    sheet_languages: LruCache<String, ConvertibleLanguagesPromise>,
    sheet_matcher: FuzzyMatcher,
    sheet_filter_data: SheetFilterData,
    changed_schemas: Option<(ChangedSchemasKey, ConvertibleChangedSchemasPromise)>,
    save_promise: Option<TrackedPromise<()>>,
    export_promise: Option<TrackedPromise<()>>,
    export_progress: SharedExportProgress,
    /// Promise for loading list diffs (sheets/music) on startup.
    list_promise: Option<TrackedPromise<()>>,
    /// Promise for version diff computation (runs in background).
    diff_result: crate::diff::DiffSharedResult,
    /// Promise for auto-initializing with saved config on restart, skipping setup page.
    auto_init_promise: Option<UnsendPromise<anyhow::Result<(Backend, BackendConfig)>>>,
    pr_window: PrWindow,
    goto_window: Option<goto::GoToWindow>,
    about_open: bool,
    music: music::MusicPlayer,
    map: crate::map::MapViewer,
    last_system_theme: Option<egui::Theme>,
    /// `None` = Latin only
    loaded_cjk: Option<CjkFont>,
    #[cfg(target_arch = "wasm32")]
    font_promise: Option<(CjkFont, UnsendPromise<anyhow::Result<Vec<u8>>>)>,

    // ── New-only list tracking ──
    show_new_sheets_only: bool,
    show_new_music_only: bool,
    sheet_new_items: Vec<String>,
    music_new_items: Vec<String>,
    sheet_list_state: String,
    music_list_state: String,

    
                // ── Version Diff ──
    diff_state: crate::diff::DiffState,

    // ── Download ──
    download_exdschema_status: crate::downloader::SharedStatus,
    download_hca_status: crate::downloader::SharedStatus,
    // ── 表格展示模式切换 ──
    /// false=优先使用配置文件个性化展示；true=强制展示完整表格
    table_layout_full: bool,
    /// 是否显示二次确认窗口
    table_layout_confirm: bool,
    /// 切换中提示（完成后自动关闭）
    table_layout_switching: bool,
    /// 切换提示开始时间（用于延时自动关闭）
    table_layout_switch_start: Option<std::time::Instant>,
    /// 待执行的表格缓存重载标志（在借用释放后处理）
    table_layout_reload_pending: bool,
    // ── 删除CSV版本窗口 ──
    delete_csv_open: bool,
    delete_csv_versions: Vec<String>,
    delete_csv_selected: std::collections::HashSet<String>,
    delete_csv_confirming: bool,
    delete_csv_done_at: Option<std::time::Instant>,
}

fn create_router(ctx: egui::Context) -> Result<Router<App>> {
    let mut builder = Router::<App>::new(ctx);
    builder.set_title_formatter(|title| format!("{title} - FF14 EXDViewer edit"));
    builder.add_route("/", App::on_setup, App::draw_setup)?;
    builder.add_route("/sheet", App::on_unnamed_sheet, App::draw_unnamed_sheet)?;
    builder.add_route("/sheet/{*name}", App::on_named_sheet, App::draw_named_sheet)?;
    builder.add_route("/music", App::on_music, App::draw_music)?;
    builder.add_route("/maps", App::on_maps, App::draw_maps)?;
    builder.add_route("/music/{id}", App::on_music_track, App::draw_music)?;
    builder.add_route(
        CALLBACK_PATH,
        App::on_auth_callback,
        App::draw_auth_callback,
    )?;
    Ok(builder)
}

impl App {
    fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.router
            .get_or_init(|| create_router(ctx.clone()).unwrap());

        let on_music = self
            .router
            .get()
            .unwrap()
            .current_path()
            .path()
            .starts_with("/music");

        let on_maps = self
            .router
            .get()
            .unwrap()
            .current_path()
            .path()
            .starts_with("/maps");

        if !on_music && !on_maps && shortcut::consume(&ctx, GOTO_ROW) {
            self.goto_window = Some(goto::GoToWindow::to_row());
        }
        if !on_music && !on_maps && shortcut::consume(&ctx, GOTO_SHEET) {
            self.goto_window = Some(goto::GoToWindow::to_sheet());
        }

        self.update_fonts(&ctx);
        self.update_sheet_languages(&ctx);
        self.pr_window.poll(&ctx);
        about::draw(&ctx, &mut self.about_open);
        self.draw_menubar(ui, on_music, on_maps);
        self.draw_logger(ui.ctx());
        self.draw_pr_window(ui.ctx());
        self.draw_export_progress_window(ui.ctx());
        self.poll_list_promise();

        CentralPanel::default().show(ui, |ui| {
            self.draw_router(ui);
        });
    }

    fn draw_router(&mut self, ui: &mut egui::Ui) {
        self.router.clone().get().unwrap().ui(self, ui);
    }

    fn update_sheet_languages(&mut self, ctx: &egui::Context) {
        let Some(backend) = self.backend.as_ref() else {
            return;
        };
        let Some(sheet_name) = SELECTED_SHEET.get(ctx) else {
            return;
        };

        let entry = self.sheet_languages.get_or_insert_mut_ref(&sheet_name, || {
            let sheet_name = sheet_name.clone();
            let excel = backend.excel().clone();
            ConvertiblePromise::new_promise(TrackedPromise::spawn_local(async move {
                excel.get_available_languages(&sheet_name).await
            }))
        });
        let just_resolved = !entry.converted() && entry.should_swap();
        if let Some(Ok(languages)) = entry.get(|r| r) {
            CURRENT_SHEET_LANGUAGES.set(ctx, (sheet_name.clone(), languages.clone()));
        }
        if just_resolved {
            ctx.request_repaint();
        }
    }

    fn navigate(&self, path: impl Into<Path>) {
        self.router.get().unwrap().navigate(path).unwrap();
    }

    fn navigate_replace(&self, path: impl Into<Path>) {
        self.router.get().unwrap().replace(path).unwrap();
    }

    /// 全局导出进度窗口（任何页面都会显示，含中断导出按钮）
    fn draw_export_progress_window(&mut self, ctx: &egui::Context) {
        {
            let ep = self.export_progress.lock().unwrap().clone();
            if let Some(ep) = ep {
                // 无错误完成时3秒后自动关闭
                if ep.done && ep.error.is_none() {
                    let mut auto_close = false;
                    {
                        let mut lock = self.export_progress.lock().unwrap();
                        if let Some(p) = lock.as_mut() {
                            match p.done_at {
                                Some(t) => {
                                    if t.elapsed().as_secs_f32() >= 3.0 {
                                        auto_close = true;
                                    }
                                }
                                None => p.done_at = Some(std::time::Instant::now()),
                            }
                        }
                    }
                    if auto_close {
                        *self.export_progress.lock().unwrap() = None;
                    }
                }
                let ep2 = self.export_progress.lock().unwrap().clone();
                if let Some(ep) = ep2 {
                    let mut show = true;
                    egui::Window::new("导出进度")
                        .id("export_progress".into())
                        .open(&mut show)
                        .show(ctx, |ui| {
                            ui.label(ep.title.clone());
                            if ep.total > 0 {
                                let frac = ep.current as f32 / ep.total as f32;
                                ui.add(
                                    egui::ProgressBar::new(frac)
                                        .show_percentage()
                                        .text(format!("{}/{}", ep.current, ep.total)),
                                );
                            }
                            if !ep.current_name.is_empty() {
                                ui.label(format!("当前: {}", ep.current_name));
                            }
                            if let Some(err) = &ep.error {
                                ui.colored_label(egui::Color32::RED, err);
                            }
                            if ep.done {
                                ui.label("已完成");
                                if ui.button("关闭").clicked() {
                                    *self.export_progress.lock().unwrap() = None;
                                }
                            } else {
                                // 进行中：中断导出按钮
                                if ui.button("中断导出").clicked() {
                                    if let Some(p) = self.export_progress.lock().unwrap().as_mut() {
                                        p.cancel
                                            .store(true, std::sync::atomic::Ordering::Relaxed);
                                    }
                                }
                            }
                        });
                }
            }
        }
    }

    fn draw_goto(&mut self, ctx: &egui::Context) {
        if let Some(window) = self.goto_window.take() {
            let misc_sheets_shown = MISC_SHEETS_SHOWN.get(ctx);
            match window.draw(
                ctx,
                &self.sheet_matcher,
                &self.backend.as_ref().map_or(vec![], |b| {
                    b.excel()
                        .get_entries()
                        .iter()
                        .filter(|(_, id)| misc_sheets_shown || **id >= 0)
                        .map(|(s, _)| s.as_str())
                        .collect()
                }),
            ) {
                Ok(Some(data)) => {
                    let sheet = match &data {
                        EitherOrBoth::Left(sheet_name) | EitherOrBoth::Both(sheet_name, _) => {
                            Some(sheet_name.clone())
                        }
                        EitherOrBoth::Right(_) => SELECTED_SHEET.get(ctx),
                    };
                    let location = match &data {
                        EitherOrBoth::Left(_) => None,
                        EitherOrBoth::Right(loc) | EitherOrBoth::Both(_, loc) => Some(loc),
                    };

                    if let Some(sheet_name) = sheet {
                        if let Some((row, subrow)) = location {
                            self.navigate(format!(
                                "/sheet/{sheet_name}#R{row}{}",
                                if let Some(subrow) = subrow {
                                    format!(".{subrow}")
                                } else {
                                    String::new()
                                }
                            ));
                        } else {
                            self.navigate(format!("/sheet/{sheet_name}"));
                        }
                    }
                }
                Ok(None) => {}
                Err(window) => {
                    self.goto_window = Some(window);
                }
            }
        }
    }

    fn draw_menubar(&mut self, ui: &mut egui::Ui, on_music: bool, on_maps: bool) {
        let ctx = &ui.ctx().clone();
        Panel::top("top_panel")
            .frame(
                egui::Frame::side_top_panel(&ctx.global_style())
                    .fill(ctx.global_style().visuals.code_bg_color),
            )
            .show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    let bar_left = ui.min_rect().left();
                    let bar_width = ui.available_width();

                    ui.menu_button("应用", |ui| {
                        if ui.button("设置").clicked() {
                            self.navigate("/");
                            ui.close();
                        }
                        if !super::IS_WEB && ui.button("退出").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            ui.close();
                        }
                    });

                    if !on_music {
                        ui.menu_button("跳转", |ui| {
                            if shortcut::button(ui, "跳转到 行…", GOTO_ROW).clicked() {
                                self.goto_window = Some(goto::GoToWindow::to_row());
                                ui.close();
                            }
                            if shortcut::button(ui, "跳转到 表…", GOTO_SHEET).clicked() {
                                self.goto_window = Some(goto::GoToWindow::to_sheet());
                                ui.close();
                            }
                        });
                    }

                    ui.menu_button("数据语言", |ui| {
                        let saved_lang = LANGUAGE.get(ctx);
                        let selected_sheet = SELECTED_SHEET.get(ctx);
                        let sheet_languages = CURRENT_SHEET_LANGUAGES
                            .try_get(ctx)
                            .filter(|(name, _)| Some(name.as_str()) == selected_sheet.as_deref())
                            .map(|(_, langs)| langs);
                        let restrict = sheet_languages
                            .as_ref()
                            .is_some_and(|langs| langs.iter().any(|&l| l != Language::None));
                        for lang in Language::iter() {
                            if lang == Language::None {
                                continue;
                            }
                            let available = !restrict
                                || sheet_languages
                                    .as_ref()
                                    .is_some_and(|langs| langs.contains(&lang));
                            let response = ui.add_enabled(
                                available,
                                egui::Button::selectable(saved_lang == lang, lang.to_string()),
                            );
                            if response.clicked() {
                                LANGUAGE.set(ctx, lang);
                                ui.close();
                            }
                        }
                    });

                    ui.menu_button("视图设置", |ui| {
                        ui.menu_button("颜色主题", |ui| {
                            let mut color_theme = COLOR_THEME.get(ui.ctx());
                            for theme in ColorTheme::themes() {
                                if ui
                                    .selectable_value(&mut color_theme, *theme, theme.name())
                                    .changed()
                                {
                                    color_theme.apply(ui.ctx());
                                    let solid_scrollbar = SOLID_SCROLLBAR.get(ctx);
                                    ctx.all_styles_mut(|s| {
                                        s.spacing.scroll = if solid_scrollbar {
                                            ScrollStyle::solid()
                                        } else {
                                            ScrollStyle::default()
                                        };
                                    });

                                    COLOR_THEME.set(ui.ctx(), color_theme);
                                }
                            }
                        });

                        ui.menu_button("代码主题", |ui| {
                            let mut theme = CODE_SYNTAX_THEME.get(ui.ctx());

                            for (id, name) in CodeTheme::themes() {
                                if ui
                                    .selectable_value(&mut theme.theme, id.to_string(), name)
                                    .changed()
                                {
                                    CODE_SYNTAX_THEME.set(ui.ctx(), theme.clone());
                                }
                            }
                        });

                        ui.menu_button("列排序规则", |ui| {
                            let mut sorted_by_offset = SORTED_BY_OFFSET.get(ctx);
                            let r = ui.selectable_value(&mut sorted_by_offset, true, "偏移Offset");
                            let r =
                                r.union(ui.selectable_value(&mut sorted_by_offset, false, "序号Index"));
                            if r.changed() {
                                ui.close();
                                SORTED_BY_OFFSET.set(ctx, sorted_by_offset);
                            }
                        });

                        ui.menu_button("文本换行", |ui| {
                            let r = opt_slider(
                                ui,
                                TEXT_WRAP_WIDTH.get(ctx).map(|e| e.into()),
                                50..=1000,
                                "最大宽度",
                                "不自动换行",
                                "px",
                            );

                            let r2 = opt_slider(
                                ui,
                                TEXT_MAX_LINES.get(ctx).map(|e| e.into()),
                                1..=20,
                                "最大行数",
                                "无限制",
                                "",
                            );

                            if r.response.changed() || r2.response.changed() {
                                TEXT_WRAP_WIDTH.set(
                                    ctx,
                                    r.inner.map(|e| NonZero::new(e.get() as u16).unwrap()),
                                );

                                TEXT_MAX_LINES.set(
                                    ctx,
                                    r2.inner.map(|e| NonZero::new(e.get() as u8).unwrap()),
                                );

                                for sheet in &mut self.sheet_data {
                                    if let Ok(Ok(s)) = sheet.1.try_get_mut() {
                                        s.invalidate_sizes(ui);
                                    }
                                }
                            }

                            let mut use_scroll = TEXT_USE_SCROLL.get(ctx);
                            ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.style_mut().spacing.item_spacing.x /= 2.0;
                                ui.label("文本过长时使用");
                                if ui
                                    .selectable_label(
                                        use_scroll,
                                        if use_scroll { " [滚动条] " } else { " [气泡提示框] " },
                                    )
                                    .clicked()
                                {
                                    use_scroll = !use_scroll;
                                    TEXT_USE_SCROLL.set(ctx, use_scroll);
                                }
                                ui.label("显示");
                            })
                        });

                        {
                            let mut solid_scrollbar = SOLID_SCROLLBAR.get(ctx);
                            if ui
                                .checkbox(&mut solid_scrollbar, "滚动条永久显示")
                                .changed()
                            {
                                SOLID_SCROLLBAR.set(ctx, solid_scrollbar);
                                ctx.all_styles_mut(|s| {
                                    s.spacing.scroll = if solid_scrollbar {
                                        ScrollStyle::solid()
                                    } else {
                                        ScrollStyle::default()
                                    };
                                });
                                ui.close();
                            }
                        }

                        {
                            let mut always_hires = ALWAYS_HIRES.get(ctx);
                            if ui.checkbox(&mut always_hires, "高清图标").changed() {
                                ALWAYS_HIRES.set(ctx, always_hires);
                                ui.close();
                            }
                        }

                        {
                            let mut evaluate_strings = EVALUATE_STRINGS.get(ctx);
                            if ui
                                .checkbox(&mut evaluate_strings, "动态刷新字段")
                                .changed()
                            {
                                EVALUATE_STRINGS.set(ctx, evaluate_strings);

                                for sheet in &mut self.sheet_data {
                                    if let Ok(Ok(s)) = sheet.1.try_get_mut() {
                                        s.invalidate_sizes(ui);
                                    }
                                }
                            }
                        }

                        {
                            let mut display_field_shown = DISPLAY_FIELD_SHOWN.get(ctx);
                            if ui
                                .checkbox(&mut display_field_shown, "启用可见列")
                                .changed()
                            {
                                DISPLAY_FIELD_SHOWN.set(ctx, display_field_shown);
                                ui.close();
                            }
                        }

                        {
                            let mut logger_shown = LOGGER_SHOWN.get(ctx);
                            if ui.checkbox(&mut logger_shown, "显示Log日志窗口").changed() {
                                LOGGER_SHOWN.set(ctx, logger_shown);
                            }
                        }
                    });

                    // Download menu in top bar
                    if self.backend.is_some() {
                        ui.menu_button("下载", |ui| {
                            if ui.button("EXDSchema").clicked() {
                                let url = crate::config_file::load_backend_config()
                                    .and_then(|c| c.exdschema_url)
                                    .unwrap_or_else(|| crate::downloader::DEFAULT_EXDSCHEMA_URL.to_string());
                                let status = self.download_exdschema_status.clone();
                                crate::downloader::start_download_exdschema(&url, status);
                                ui.close();
                            }
                            if ui.button("HCADecoder").clicked() {
                                let url = crate::config_file::load_backend_config()
                                    .and_then(|c| c.hca_url)
                                    .unwrap_or_else(|| crate::downloader::DEFAULT_HCA_DECODER_URL.to_string());
                                let status = self.download_hca_status.clone();
                                crate::downloader::start_download_hca(&url, status);
                                ui.close();
                            }
                        });
                    }

                    // Export menu in top bar
                    if self.backend.is_some() {
                        ui.menu_button("导出", |ui| {
                            if ui.button("导出全部CSV").clicked() {
                                let lang = LANGUAGE.get(ctx);
                                if let Some(backend) = self.backend.clone() {
                                    let version = self.backend.as_ref().and_then(|b| b.game_version())
                                        .and_then(|v| GameVersion::new(v).ok())
                                        .or_else(|| BACKEND_CONFIG.get(ctx).and_then(|c| {
                                            if let InstallLocation::Web(_, _, v) = &c.location {
                                                v.clone()
                                            } else {
                                                None
                                            }
                                        }));
                                    self.command_export_all_csv(
                                        backend, lang, true, version,
                                    );
                                }
                                ui.close();
                            }
                            if ui.button("导出全部CSV源文件").clicked() {
                                let lang = LANGUAGE.get(ctx);
                                if let Some(backend) = self.backend.clone() {
                                    let version = self.backend.as_ref().and_then(|b| b.game_version())
                                        .and_then(|v| GameVersion::new(v).ok())
                                        .or_else(|| BACKEND_CONFIG.get(ctx).and_then(|c| {
                                            if let InstallLocation::Web(_, _, v) = &c.location {
                                                v.clone()
                                            } else {
                                                None
                                            }
                                        }));
                                    self.command_export_all_csv(
                                        backend, lang, false, version,
                                    );
                                }
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("导出收藏CSV").clicked() {
                                let lang = LANGUAGE.get(ctx);
                                if let Some(backend) = self.backend.clone() {
                                    let version = self.backend.as_ref().and_then(|b| b.game_version())
                                        .and_then(|v| GameVersion::new(v).ok())
                                        .or_else(|| BACKEND_CONFIG.get(ctx).and_then(|c| {
                                            if let InstallLocation::Web(_, _, v) = &c.location {
                                                v.clone()
                                            } else {
                                                None
                                            }
                                        }));
                                    self.command_export_favorites_csv(
                                        backend, lang, true, version,
                                    );
                                }
                                ui.close();
                            }
                            if ui.button("导出收藏CSV源文件").clicked() {
                                let lang = LANGUAGE.get(ctx);
                                if let Some(backend) = self.backend.clone() {
                                    let version = self.backend.as_ref().and_then(|b| b.game_version())
                                        .and_then(|v| GameVersion::new(v).ok())
                                        .or_else(|| BACKEND_CONFIG.get(ctx).and_then(|c| {
                                            if let InstallLocation::Web(_, _, v) = &c.location {
                                                v.clone()
                                            } else {
                                                None
                                            }
                                        }));
                                    self.command_export_favorites_csv(
                                        backend, lang, false, version,
                                    );
                                }
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("导出全部音乐").clicked() {
                                if let Some(backend) = self.backend.clone() {
                                    self.command_export_music(&backend, false);
                                }
                                ui.close();
                            }
                            if ui.button("保存全部地图图片").clicked() {
                                if let Some(backend) = self.backend.clone() {
                                    self.command_export_map(&backend, false);
                                }
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("删除指定版本CSV文件").clicked() {
                                self.delete_csv_versions = crate::diff::find_version_folders();
                                // 默认选中除最新两个版本外的全部文件夹
                                self.delete_csv_selected = self.delete_csv_versions
                                    .iter()
                                    .take(self.delete_csv_versions.len().saturating_sub(2))
                                    .cloned()
                                    .collect();
                                self.delete_csv_confirming = false;
                                self.delete_csv_done_at = None;
                                self.delete_csv_open = true;
                                ui.close();
                            }
                        });
                    }

                    // ¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯
                    // Data source version display
                    if let Some(backend) = &self.backend {
                        if let Some(ver) = backend.game_version() {
                            ui.label(
                                egui::RichText::new(format!("数据版本: {ver}"))
                                    .color(egui::Color32::from_gray(160))
                                    .size(12.0)
                            );
                        }
                    }

                    let seg = egui::vec2(72.0, ui.spacing().interact_size.y);
                    let switcher_w = 3.0 * seg.x + 2.0 * ui.spacing().item_spacing.x;
                    let target_left = bar_left + bar_width / 2.0 - switcher_w / 2.0;
                    let space = target_left - ui.cursor().left();
                    if space > 0.0 {
                        ui.add_space(space);
                    }
                    if ui
                        .add_sized(seg, Button::selectable(!on_music && !on_maps, "数据列表"))
                        .clicked()
                    {
                        self.navigate("/sheet");
                    }
                    if ui
                        .add_sized(seg, Button::selectable(on_maps, "地图列表"))
                        .clicked()
                    {
                        self.navigate("/maps");
                    }
                    if ui
                        .add_sized(seg, Button::selectable(on_music, "音乐列表"))
                        .clicked()
                    {
                        self.navigate("/music");
                    }

                    add_links(ui, &mut self.about_open);
                });
            });
    }

fn draw_logger(&mut self, ctx: &egui::Context) {
        let logger_shown = LOGGER_SHOWN.get(ctx);
        let mut logger_shown_toggle = logger_shown;
        // Default level set via builder.log_levels() in main.rs
        egui::Window::new("日志")
            .open(&mut logger_shown_toggle)
            .show(ctx, |ui| {
                egui_logger::logger_ui().show(ui);
            });
        if logger_shown_toggle != logger_shown {
            LOGGER_SHOWN.set(ctx, logger_shown_toggle);
        }
    }
    fn poll_changed_schemas(&mut self, ctx: &egui::Context) -> PrChangedState {
        let key = match BACKEND_CONFIG.get(ctx) {
            Some(BackendConfig {
                schema: SchemaLocation::Github(location),
                ..
            }) => match &location.branch {
                GithubSchemaBranch::PullRequest { number, .. } => {
                    (location.owner.clone(), location.repo.clone(), *number)
                }
                _ => {
                    self.changed_schemas = None;
                    return PrChangedState::NotPr;
                }
            },
            _ => {
                self.changed_schemas = None;
                return PrChangedState::NotPr;
            }
        };

        if self.changed_schemas.as_ref().map(|(k, _)| k) != Some(&key) {
            let (owner, repo, number) = key.clone();
            self.changed_schemas = Some((
                key,
                ConvertiblePromise::new_promise(TrackedPromise::spawn_local(async move {
                    WebProvider::fetch_github_pull_request_files(&owner, &repo, number).await
                })),
            ));
        }

        let (_, promise) = self.changed_schemas.as_mut().unwrap();
        match promise.get(|result| match result {
            Ok(names) => Some(Rc::new(names.into_iter().collect())),
            Err(e) => {
                log::error!("Error fetching PR-changed schemas: {e}");
                None
            }
        }) {
            None => PrChangedState::Pending,
            Some(None) => PrChangedState::Failed,
            Some(Some(set)) => PrChangedState::Ready(set.clone()),
        }
    }

    fn draw_sheet_list(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        let pr_changed = self.poll_changed_schemas(ctx);
        CollapsibleSidePanel::new("sheet_list", Side::Left).show(ui, |ui, is_open| {
            if !is_open {
                return;
            }

            Panel::top("sheet_list_header").show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(egui::Align::Min), |ui| {
                        CollapsibleSidePanel::draw_arrow(ui, "sheet_list");
                        ui.vertical_centered_justified(|ui| ui.heading("数据列表"));
                    });
                });
                ui.add_space(4.0);
                ui.with_layout(Layout::right_to_left(egui::Align::Min), |ui| {
                    let mut sheets_filter = SHEETS_FILTER.get(ctx);
                    let resp = ui
                        .add_enabled(!sheets_filter.is_empty(), Button::new("↩"))
                        .on_hover_text("清除");
                    if resp.clicked() {
                        sheets_filter.clear();
                        SHEETS_FILTER.set(ctx, sheets_filter.clone());
                    }

                    let mut misc_sheets_shown = MISC_SHEETS_SHOWN.get(ctx);
                    if ui
                        .toggle_value(&mut misc_sheets_shown, "🗄")
                        .on_hover_text("显示杂项表格")
                        .changed()
                    {
                        MISC_SHEETS_SHOWN.set(ctx, misc_sheets_shown);
                    }

                    if !matches!(pr_changed, PrChangedState::NotPr) {
                        let mut changed_only = PR_CHANGED_ONLY.get(ctx);
                        let hover = match &pr_changed {
                            PrChangedState::Ready(_) => "Filter unchanged sheets",
                            PrChangedState::Pending => "Filter unchanged sheets (loading…)",
                            PrChangedState::Failed => "Filter unchanged sheets (failed to load)",
                            PrChangedState::NotPr => unreachable!(),
                        };
                        if ui
                            .toggle_value(&mut changed_only, "±")
                            .on_hover_text(hover)
                            .changed()
                        {
                            PR_CHANGED_ONLY.set(ctx, changed_only);
                        }
                    }

                    // New-only sheet toggle
                    let new_count = self.sheet_new_items.len();
                    let is_ready = self.sheet_list_state.starts_with("ready");
                    if is_ready && new_count > 0 {
                        ui.toggle_value(&mut self.show_new_sheets_only, "🔍")
                            .on_hover_text(format!("仅显示新增项（{new_count} 项）"));
                    } else if self.sheet_list_state == "no_changes" {
                        ui.label("✓")
                            .on_hover_text("当前列表无变动");
                    } else if self.sheet_list_state == "no_prev" {
                        ui.label("①")
                            .on_hover_text("首次运行，暂无旧版存档");
                    } else if self.sheet_list_state.starts_with("error") {
                        ui.label("⚠")
                            .on_hover_text(&self.sheet_list_state);
                    }

                    if ui
                        .add_sized(
                            Vec2::new(ui.available_width(), 0.0),
                            TextEdit::singleline(&mut sheets_filter).hint_text("筛选"),
                        )
                        .changed()
                    {
                        SHEETS_FILTER.set(ctx, sheets_filter);
                    }
                });
                ui.add_space(4.0);
            });

            let modified_schemas = self.get_modified_schemas();
            if !modified_schemas.is_empty() {
                let count = modified_schemas.len();
                let modified_tooltip = modified_schemas.iter().map(|(name, _)| name).join("\n");
                drop(modified_schemas);
                let save_label = if count > 1 { "保存全部" } else { "保存" };

                Panel::bottom("sheet_list_status").show(ui, |ui| {
                    let can_pr = pr_window::github_source(ctx).is_some();
                    ui.vertical_centered(|ui| {
                        ui.label(format!(
                            "{count} modified schema{}",
                            if count > 1 { "s" } else { "" }
                        ))
                        .on_hover_text(modified_tooltip);
                    });

                    let mut save = false;
                    let mut open_pr = false;
                    if can_pr {
                        ui.columns_const(|[c1, c2]| {
                            c1.vertical_centered_justified(|ui| {
                                if ui.button("创建PR").clicked() {
                                    open_pr = true;
                                }
                            });
                            c2.vertical_centered_justified(|ui| {
                                if ui.button(save_label).clicked() {
                                    save = true;
                                }
                            });
                        });
                    } else {
                        ui.vertical_centered_justified(|ui| {
                            if ui.button(save_label).clicked() {
                                save = true;
                            }
                        });
                    }
                    if save {
                        self.command_save_all_schemas();
                    }
                    if open_pr {
                        self.command_open_pr();
                    }
                });
            }

            let sheets_filter = SHEETS_FILTER.get(ctx);
            let misc_sheets_shown = MISC_SHEETS_SHOWN.get(ctx);
            
            let backend = self.backend.clone().unwrap();
            let sheets = self
                .sheet_filter_data
                .get_or_insert((sheets_filter.clone(), misc_sheets_shown), || {
                    let entries = backend.excel().get_entries();
                    let sample: Vec<&str> = entries.keys().take(3).map(String::as_str).collect();
                    log::debug!("excel.get_entries() sample: {:?}", sample);
                    let sheets = entries
                        .iter()
                        .filter(|(_, id)| misc_sheets_shown || **id >= 0)
                        .sorted_by_key(|(sheet, _)| *sheet)
                        .map(|(s, &id)| (s.clone(), id));
                    let sheets = self.sheet_matcher.match_list_indirect(
                        (!sheets_filter.is_empty()).then_some(&sheets_filter),
                        sheets,
                        |s| &s.0,
                    );
                    Rc::new(sheets)
                })
                .clone();


            let sheets = match &pr_changed {
                PrChangedState::Ready(changed) if PR_CHANGED_ONLY.get(ctx) => Rc::new(
                    sheets
                        .iter()
                        .filter(|(name, _)| changed.contains(name))
                        .cloned()
                        .collect::<Vec<_>>(),
                ),
                _ => sheets,
            };

            egui::CentralPanel::default().show(ui, |ui| {
                // Sort: favorited sheets first, then alphabetically
                let mut sorted_sheets: Vec<(String, i32)> = sheets.to_vec();
                if self.show_new_sheets_only && self.sheet_list_state.starts_with("ready") {
                    // Check: are the new items misc sheets (negative ID)?
                    if let Some(backend) = self.backend.as_ref() {
                        let entries = backend.excel().get_entries();
                        let first_new = self.sheet_new_items.first().cloned().unwrap_or_default();
                        if let Some(id) = entries.get(&first_new) {
                            log::debug!("first new item '{}' has id={} (misc={})", first_new, id, *id < 0);
                        }
                        // Build sorted_sheets from entries, not from cache
                        let misc_shown = MISC_SHEETS_SHOWN.get(ctx);
                        let new_set: std::collections::HashSet<&str> = self.sheet_new_items.iter().map(String::as_str).collect();
                        let all_filtered: Vec<(String, i32)> = entries.iter()
                            .filter(|(_, id)| misc_shown || **id >= 0)
                            .filter(|(name, _)| new_set.contains(name.as_str()))
                            .map(|(s, &id)| (s.clone(), id))
                            .sorted_by(|a, b| a.0.cmp(&b.0))
                            .collect();
                        log::debug!("new-only: {} matching sheets from entries (cached had {}), replacing sorted_sheets", 
                            all_filtered.len(), sorted_sheets.len());
                        sorted_sheets = all_filtered;
                    }
                }
                sorted_sheets.sort_by(|a, b| {
                    let a_fav = self.favorites.contains(&a.0);
                    let b_fav = self.favorites.contains(&b.0);
                    a_fav.cmp(&b_fav).reverse().then(a.0.cmp(&b.0))
                });

                let row_height = ui.text_style_height(&egui::TextStyle::Button);
                ScrollArea::both().auto_shrink(false).show_rows(
                    ui,
                    row_height,
                    sorted_sheets.len(),
                    |ui, range| {
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                            let mut current_sheet = SELECTED_SHEET.get(ctx);
                            for (sheet, id) in sorted_sheets
                                .iter()
                                .skip(range.start)
                                .take(range.end - range.start)
                            {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                                ui.horizontal(|ui| {
                                    let is_fav = self.favorites.contains(sheet);
                                    let star = if is_fav { "★ " } else { "☆ " };
                                    let resp = ui
                                        .add_sized(
                                            egui::vec2(18.0, row_height),
                                            egui::Label::new(
                                                egui::RichText::new(star)
                                                    .color(if is_fav {
                                                        egui::Color32::from_rgb(0xFF, 0xCC, 0x00)
                                                    } else {
                                                        egui::Color32::GRAY
                                                    }),
                                            )
                                            .sense(egui::Sense::click()),
                                        )
                                        .on_hover_cursor(egui::CursorIcon::Default)
                                        .on_hover_text("点击切换收藏");
                                    if resp.clicked() {
                                        self.toggle_favorite(sheet);
                                    }
                                    let resp = Button::selectable(
                                        current_sheet.as_ref() == Some(sheet),
                                        sheet.as_str(),
                                    )
                                    .ui(ui)
                                    .on_hover_text(format!("{sheet}\nId: {id}"));
                                    if resp.clicked() {
                                        current_sheet = Some(sheet.clone());
                                        SELECTED_SHEET.set(ctx, current_sheet.clone());
                                        self.navigate(format!("/sheet/{sheet}"));
                                    }
                                });
                            }
                        });
                    },
                );
            });
        });
    }

    fn draw_sheet_data(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        self.export_promise.take_if(|p| p.try_get().is_some());
        let mut export_request: Option<ExportAction> = None;

        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(&ctx.global_style()).inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 2,
                    bottom: 8,
                }),
            )
            .show(ui, |ui| {
                let backend = self.backend.as_ref().unwrap();
                let sheet_name = SELECTED_SHEET.get(ctx).unwrap();
                let language = LANGUAGE.get(ctx);

                let sheet_data =
                    self.sheet_data
                        .get_or_insert_mut_ref(&(language, sheet_name.clone()), || {
                            let sheet_name = sheet_name.clone();
                            let excel = backend.excel().clone();

                            ConvertiblePromise::new_promise(TrackedPromise::spawn_local(
                                async move { excel.get_sheet(&sheet_name, language).await },
                            ))
                        });

                let schema_data = self.schema_data.get_or_insert_mut_ref(&sheet_name, || {
                    let sheet_name = sheet_name.clone();
                    let is_sheet_miscellaneous = backend
                        .excel()
                        .get_entries()
                        .get(&sheet_name)
                        .copied()
                        .unwrap_or_default()
                        < 0;
                    let schema = backend.schema().clone();

                    ConvertiblePromise::new_promise(TrackedPromise::spawn_local(async move {
                        if !is_sheet_miscellaneous {
                            Some(schema.get_schema_text(&sheet_name).await)
                        } else {
                            None
                        }
                    }))
                });

                let schema_loading = !schema_data.should_swap();
                let sheet_loading = !sheet_data.should_swap();

                let combined_result = sheet_data.get_mut_with(schema_data, |sheet, schema| {
                    let editor = schema.either(
                        |schema| match schema {
                            Some(Ok(schema)) => Ok(EditableSchema::new(&sheet_name, schema)),
                            Some(Err(error)) => {
                                // Soft-fail on schema retrieval/parsing errors
                                log::error!("Failed to get schema: {error:?}");
                                let column_count = sheet.as_ref().either(
                                    |sheet| sheet.as_ref().map(|sheet| sheet.columns().len()),
                                    |sheet| {
                                        sheet
                                            .as_ref()
                                            .map(|sheet| sheet.context().sheet().columns().len())
                                    },
                                );
                                if let Ok(column_count) = column_count {
                                    EditableSchema::from_blank(&sheet_name, column_count)
                                } else {
                                    Err(anyhow::anyhow!(
                                        "Failed to load sheet to create blank schema"
                                    ))
                                }
                            }
                            None => EditableSchema::from_miscellaneous(&sheet_name),
                        },
                        |schema| schema,
                    );

                    let table = sheet.either(
                        |sheet| {
                            sheet.and_then(|sheet| {
                                let schema = editor.as_ref().map(|e| e.get_schema());
                                if let Ok(schema) = schema {
                                    Ok(SheetTable::with_layout(
                                        TableContext::new(
                                            GlobalContext::new(
                                                ui.ctx().clone(),
                                                backend.clone(),
                                                language,
                                                self.icon_manager.clone(),
                                            ),
                                            sheet,
                                            schema,
                                        ),
                                        ui,
                                        !self.table_layout_full,
                                    ))
                                } else {
                                    Err(anyhow::anyhow!("Failed to load schema to create table"))
                                }
                            })
                        },
                        |table| table,
                    );

                    (table, editor)
                });

                let (table, editor) = match combined_result {
                    None if schema_loading && sheet_loading => {
                        ui.label("正在加载数据表及数据结构定义...");
                        return;
                    }
                    None if schema_loading => {
                        ui.label("正在加载数据结构定义...");
                        return;
                    }
                    None if sheet_loading => {
                        ui.label("正在加载数据表...");
                        return;
                    }
                    None => {
                        ui.label("Preparing sheet and schema...");
                        return;
                    }
                    Some((Err(err), Err(err2))) => {
                        ui.label("加载数据表和数据结构失败");
                        ui.label(err.to_string());
                        ui.label(err2.to_string());
                        return;
                    }
                    Some((Err(err), _)) => {
                        ui.label("加载数据表失败");
                        ui.label(err.to_string());
                        return;
                    }
                    Some((_, Err(err))) => {
                        ui.label("加载数据结构定义失败");
                        ui.label(err.to_string());
                        return;
                    }
                    Some((Ok(table), Ok(editor))) => (table, editor),
                };

                Panel::top("sheet_data_header").show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if CollapsibleSidePanel::is_collapsed(ui.ctx(), "sheet_list") {
                            ui.with_layout(Layout::left_to_right(egui::Align::Min), |ui| {
                                CollapsibleSidePanel::draw_arrow(ui, "sheet_list");
                            });
                        }

                        // 返回按钮：返回跳转前的数据表（Link单元格跳转后）
                        if ui
                            .button("← 返回")
                            .on_hover_text("返回跳转前的数据表")
                            .clicked()
                        {
                            if let Err(e) = self.router.get().unwrap().back() {
                                log::debug!("无法返回（已在最前页）: {e}");
                            }
                        }

                        ui.vertical_centered_justified(|ui| ui.heading(sheet_name.clone()));
                    });
                    ui.add_space(4.0);
                    ui.with_layout(Layout::left_to_right(egui::Align::Min), |ui| {
                        let (mut filter_type, mut filter_text) = SHEET_FILTERS
                            .use_with(ui.ctx(), |map| {
                                map.entry(sheet_name.clone()).or_default().clone()
                            });

                        ui.spacing_mut().item_spacing.x /= 2.0;

                        let (button_resp, menu_resp) = MenuButton::from_button(
                            Button::new(filter_type.emoji())
                                .min_size(Vec2::splat(ui.spacing().interact_size.y)),
                        )
                        .ui(ui, |ui| {
                            let mut changed = false;
                            for value in &[
                                FilterInputType::Equals,
                                FilterInputType::Contains,
                                FilterInputType::Complex,
                            ] {
                                let resp =
                                    ui.selectable_value(&mut filter_type, *value, value.emoji());
                                if resp.changed() {
                                    changed = true;
                                }
                                resp.on_hover_text(value.to_string());
                            }
                            changed
                        });

                        button_resp.on_hover_text(format!("筛选类型:\n{filter_type}"));

                        let mut filter_dirty = menu_resp.is_some_and(|m| m.inner);

                        {
                            let MatchOptions {
                                mut case_insensitive,
                                mut use_display_field,
                            } = SHEET_FILTER_OPTIONS.get(ctx);

                            let mut is_dirty = ui
                                .toggle_value(&mut case_insensitive, "🔡")
                                .on_hover_text("不区分大小写")
                                .changed();
                            is_dirty |= ui
                                .toggle_value(&mut use_display_field, "📝")
                                .on_hover_text("启用可见列")
                                .changed();

                            if is_dirty {
                                SHEET_FILTER_OPTIONS.set(
                                    ctx,
                                    MatchOptions {
                                        case_insensitive,
                                        use_display_field,
                                    },
                                );
                            }
                        }

                        ui.with_layout(Layout::right_to_left(egui::Align::Min), |ui| {
                            let is_miscellaneous = backend
                                .excel()
                                .get_entries()
                                .get(&sheet_name)
                                .copied()
                                .unwrap_or_default()
                                < 0;

                            ui.add_enabled_ui(!is_miscellaneous, |ui| {
                                let mut visible = SCHEMA_EDITOR_VISIBLE.get(ui.ctx());
                                let resp = ui
                                    .toggle_value(&mut visible, "编辑数据结构定义")
                                    .on_hover_text("编辑此数据表的数据结构定义");
                                if resp.changed() {
                                    SCHEMA_EDITOR_VISIBLE.set(ui.ctx(), visible);
                                }
                            });

                            // 表格展示模式切换按钮：完整表格 / 个性化配置
                            {
                                // 当前表是否配置了个性化展示
                                let layout = crate::column_layout::load_column_layout();
                                let has_layout = crate::column_layout::get_sheet_columns(&layout, &sheet_name)
                                    .map(|cols| !cols.is_empty())
                                    .unwrap_or(false);
                                ui.add_enabled_ui(has_layout, |ui| {
                                    let label = if self.table_layout_full {
                                        "个性化配置"
                                    } else {
                                        "完整表格"
                                    };
                                    let hover = if self.table_layout_full {
                                        format!("当前展示：完整表格（{sheet_name} 未应用列布局配置）")
                                    } else {
                                        format!("当前展示：个性化配置（{sheet_name} 仅显示配置的列）")
                                    };
                                    if ui.button(label).on_hover_text(hover).clicked() {
                                        self.table_layout_confirm = true;
                                    }
                                });
                            }

                            if ui.button("版本Diff")
                                .on_hover_text("对比两个版本的数据差异")
                                .clicked()
                            {
                                let versions = crate::diff::find_version_folders();
                                let new_ver = versions.last().cloned().unwrap_or_default();
                                let old_ver = if versions.len() > 1 { versions[versions.len() - 2].clone() } else { new_ver.clone() };
                                self.diff_state = crate::diff::DiffState {
                                    active: true,
                                    old_version: old_ver,
                                    new_version: new_ver,
                                    status: "selecting".into(),
                                    diff_rows: Vec::new(),
                                    columns: Vec::new(),
                                    modal_icon_id: None,
                                    sheet: table.context().sheet().name().to_string(),
                                };
                            }

                            let exporting = self.export_promise.is_some();
                            if exporting {
                                ui.spinner();
                            }
                            ui.add_enabled_ui(!exporting, |ui| {
    ui.menu_button("导出", |ui| {
                                    if ui
                                        .button("导出CSV")
                                        .on_hover_text("链接导出为显示值")
                                        .clicked()
                                    {
                                        export_request = Some(ExportAction::Single(
                                            table.context().clone(),
                                            true,
                                        ));
                                        ui.close();
                                    }
                                    if ui
                                        .button("导出CSV源文件")
                                        .on_hover_text("链接导出为原始值")
                                        .clicked()
                                    {
                                        export_request = Some(ExportAction::Single(
                                            table.context().clone(),
                                            false,
                                        ));
                                        ui.close();
                                    }
                                });
                            });

                            let filter_error = table.get_filter_error();

                            let filter_resp = ui.add_sized(
                                Vec2::new(ui.available_width(), 0.0),
                                TextEdit::singleline(&mut filter_text)
                                    .hint_text("筛选")
                                    .background_color(if filter_error.is_some() {
                                        ui.visuals()
                                            .text_edit_bg_color()
                                            .blend(ui.visuals().error_fg_color.gamma_multiply(0.2))
                                    } else {
                                        ui.visuals().text_edit_bg_color()
                                    }),
                            );

                            filter_dirty |= filter_resp.changed();

                            if let Some(text) = filter_error {
                                filter_resp.on_hover_text(RichText::new(text).monospace());
                            }
                        });

                        if filter_dirty {
                            SHEET_FILTERS.use_with(ui.ctx(), |map| {
                                map.entry(sheet_name.clone())
                                    .insert_entry((filter_type, filter_text.clone()));
                            });

                            table.update_filter(ui.ctx());
                        }
                    });
                    ui.add_space(4.0);
                });

                let resp = editor.draw(ui, backend.schema());
                if resp.changed()
                    && let Some(schema) = editor.get_schema()
                    && let Err(e) = table.context().set_schema(Some(schema))
                {
                    log::error!("Failed to set schema: {e:?}");
                }

                // ── Version Diff ──
                
                // ── Download Status Window ──
                let download_msgs: Vec<(String, &std::sync::Arc<std::sync::Mutex<crate::downloader::DownloadStatus>>)> = {
                    let mut v = Vec::new();
                    let ds = self.download_exdschema_status.lock().unwrap().clone();
                    if ds != crate::downloader::DownloadStatus::Idle {
                        v.push(("EXDSchema".to_string(), &self.download_exdschema_status));
                    }
                    let dh = self.download_hca_status.lock().unwrap().clone();
                    if dh != crate::downloader::DownloadStatus::Idle {
                        v.push(("HCADecoder".to_string(), &self.download_hca_status));
                    }
                    v
                };
                if !download_msgs.is_empty() {
                    egui::Window::new("下载进度").id("download_progress".into()).show(ctx, |ui| {
                        for (name, status_arc) in &download_msgs {
                            let st = status_arc.lock().unwrap().clone();
                            let (msg, is_done) = match &st {
                                crate::downloader::DownloadStatus::Downloading(s) => (format!("{name}: {s}"), false),
                                crate::downloader::DownloadStatus::Done => (format!("{name} 下载完成"), true),
                                crate::downloader::DownloadStatus::Error(e) => (format!("{name}: {e}"), true),
                                _ => (String::new(), false),
                            };
                            if !msg.is_empty() {
                                ui.label(msg);
                            }
                            if is_done && ui.button("关闭").clicked() {
                                *status_arc.lock().unwrap() = crate::downloader::DownloadStatus::Idle;
                            }
                        }
                    });
                }

                // ── 删除CSV版本窗口 ──
                if self.delete_csv_open {
                    // 完成后5秒自动关闭
                    if let Some(t) = self.delete_csv_done_at {
                        if t.elapsed().as_secs_f32() >= 5.0 {
                            self.delete_csv_open = false;
                            self.delete_csv_done_at = None;
                            self.delete_csv_confirming = false;
                        }
                    }
                    let mut close_window = false;
                    let mut trigger_confirm = false;
                    let mut trigger_delete = false;
                    egui::Window::new("勾选需要删除的CSV文件版本号")
                        .id("delete_csv_window".into())
                        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                        .collapsible(false)
                        .resizable(false)
                        .default_size([460.0, 380.0])
                        .show(ctx, |ui| {
                            ui.label(
                                egui::RichText::new("注意：执行删除后无法恢复！")
                                    .color(egui::Color32::RED)
                                    .size(13.0),
                            );
                            ui.separator();
                            if let Some(t) = self.delete_csv_done_at {
                                ui.label(
                                    egui::RichText::new(
                                        format!("已完成删除，窗口5秒后自动关闭……（{:.0}秒）", 5.0 - t.elapsed().as_secs_f32().max(0.0)),
                                    ).color(egui::Color32::GREEN),
                                );
                                return;
                            }
                            egui::ScrollArea::vertical()
                                .id_salt("delete_csv_list")
                                .max_height(220.0)
                                .show(ui, |ui| {
                                    let versions = self.delete_csv_versions.clone();
                                    for v in &versions {
                                        let mut checked = self.delete_csv_selected.contains(v);
                                        if ui.checkbox(&mut checked, v).changed() {
                                            if checked {
                                                self.delete_csv_selected.insert(v.clone());
                                            } else {
                                                self.delete_csv_selected.remove(v);
                                            }
                                        }
                                    }
                                });
                            ui.separator();
                            ui.horizontal_wrapped(|ui| {
                                // 全选/取消全选
                                if ui.button("全选").clicked() {
                                    if self.delete_csv_selected.len() == self.delete_csv_versions.len()
                                        && !self.delete_csv_versions.is_empty()
                                    {
                                        self.delete_csv_selected.clear();
                                    } else {
                                        self.delete_csv_selected =
                                            self.delete_csv_versions.iter().cloned().collect();
                                    }
                                }
                                // 重置为默认选中（除最新两个版本外）
                                if ui.button("选中除最新两个版本外的CSV").clicked() {
                                    self.delete_csv_selected = self.delete_csv_versions
                                        .iter()
                                        .take(self.delete_csv_versions.len().saturating_sub(2))
                                        .cloned()
                                        .collect();
                                }
                                if ui.button("关闭").clicked() {
                                    close_window = true;
                                }
                            });
                            ui.separator();
                            if ui.button("确定删除选中版本的CSV文件").clicked() {
                                if self.delete_csv_selected.is_empty() {
                                    // 无选中则直接提示？这里用弹窗内容提示
                                    ui.colored_label(egui::Color32::RED, "请先勾选要删除的版本！");
                                } else {
                                    trigger_confirm = true;
                                }
                            }
                        });

                    // 二次确认框
                    if self.delete_csv_confirming {
                        egui::Modal::new(egui::Id::new("delete_csv_confirm_modal")).show(ctx, |ui| {
                            ui.heading("是否确定删除勾选版本的全部CSV文件");
                            ui.label(format!("将删除 {} 个版本文件夹中的全部CSV文件", self.delete_csv_selected.len()));
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if ui.button("确定").clicked() {
                                    trigger_delete = true;
                                }
                                if ui.button("取消").clicked() {
                                    self.delete_csv_confirming = false;
                                }
                            });
                        });
                    }

                    if trigger_confirm {
                        self.delete_csv_confirming = true;
                    }
                    if trigger_delete {
                        self.delete_csv_confirming = false;
                        let versions: Vec<String> = self.delete_csv_selected.iter().cloned().collect();
                        Self::delete_csv_versions_impl(versions);
                        self.delete_csv_done_at = Some(std::time::Instant::now());
                    }
                    if close_window {
                        self.delete_csv_open = false;
                        self.delete_csv_done_at = None;
                        self.delete_csv_confirming = false;
                    }
                }

                // ── 表格展示模式切换：二次确认窗口 ──
                if self.table_layout_confirm {
                    let target = if self.table_layout_full { "个性化配置" } else { "原始数据" };
                    egui::Modal::new(egui::Id::new("table_layout_confirm_modal")).show(ctx, |ui| {
                        ui.heading(format!("将切换为{target}表格展示"));
                        ui.label("重新加载表格数据需要时间，是否确认切换？");
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("确定").clicked() {
                                self.table_layout_full = !self.table_layout_full;
                                // 标记待重载（借用释放后在 draw_sheet_data 之前执行）
                                self.table_layout_reload_pending = true;
                                self.table_layout_confirm = false;
                                self.table_layout_switching = true;
                                self.table_layout_switch_start = Some(std::time::Instant::now());
                                ctx.request_repaint();
                            }
                            if ui.button("取消").clicked() {
                                self.table_layout_confirm = false;
                            }
                        });
                    });
                }

                // ── 表格展示模式切换：进行中提示（完成后自动关闭） ──
                if self.table_layout_switching {
                    egui::Modal::new(egui::Id::new("table_layout_switching_modal")).show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("正在切换表格展示数据中，请耐心等待……");
                        });
                    });
                    // 显示约600ms后自动关闭（切换已完成）
                    if self
                        .table_layout_switch_start
                        .map_or(true, |t| t.elapsed().as_secs_f32() >= 0.6)
                    {
                        self.table_layout_switching = false;
                        self.table_layout_switch_start = None;
                    } else {
                        ctx.request_repaint();
                    }
                }

                let sheet_name = table.context().sheet().name().to_string();
                // 切换数据表时自动取消Diff对比
                if self.diff_state.active && self.diff_state.sheet != sheet_name {
                    self.diff_state.active = false;
                    self.diff_state.status = "idle".into();
                    self.diff_state.diff_rows.clear();
                    self.diff_state.columns.clear();
                }
                if let Some(action) = crate::diff::draw_diff_window(
                    &mut self.diff_state, ui, &sheet_name, &self.diff_result,
                ) {
                    match action {
                        crate::diff::DiffAction::Compare { old, new, sheet } => {
                            self.diff_state.status = "comparing".into();
                            self.diff_result = crate::diff::start_background_diff(old, new, sheet);
                        }
                    }
                }
                let scroll_to = TEMP_SCROLL_TO.take(ctx);
                if let Some((row_pos, _)) = &scroll_to {
                    TEMP_HIGHLIGHTED_ROW.set(ctx, *row_pos);
                }

                let diff_state_owned = if self.diff_state.active && self.diff_state.status == "done" {
                    Some(self.diff_state.clone())
                } else {
                    None
                };
                let resp = if let Some(ref ds) = diff_state_owned {
                    crate::diff::draw_diff_table(ds, ui, table.context(), !self.table_layout_full)
                } else {
                    table.draw(ui, scroll_to)
                };
                match resp {
                    CellResponse::None => {}
                    CellResponse::Icon(..) => {}
                    CellResponse::Link((sheet_name, (row_id, subrow_id))) => {
                        self.navigate(format!(
                            "/sheet/{sheet_name}#R{row_id}{}",
                            if let Some(subrow_id) = subrow_id {
                                format!(".{subrow_id}")
                            } else {
                                String::new()
                            }
                        ));
                    }
                    CellResponse::Row((sheet_name, (row_id, subrow_id))) => {
                        self.navigate_replace(format!(
                            "/sheet/{sheet_name}#R{row_id}{}",
                            if let Some(subrow_id) = subrow_id {
                                format!(".{subrow_id}")
                            } else {
                                String::new()
                            }
                        ));
                        ui.ctx().copy_text(self.router.get().unwrap().full_url());
                    }
                }
            });

        if let Some(action) = export_request {
            match action {
                ExportAction::Single(context, resolve_display_field) => {
                    self.command_export_csv(context, resolve_display_field);
                }
                ExportAction::All(resolve_display_field) => {
                    let lang = LANGUAGE.get(ui.ctx());
                    if let Some(backend) = self.backend.clone() {
                        let version = self.backend.as_ref().and_then(|b| b.game_version())
                            .and_then(|v| GameVersion::new(v).ok())
                            .or_else(|| BACKEND_CONFIG.get(ui.ctx()).and_then(|c| {
                                if let InstallLocation::Web(_, _, v) = &c.location {
                                    v.clone()
                                } else {
                                    None
                                }
                            }));
                        self.command_export_all_csv(
                            backend,
                            lang,
                            resolve_display_field,
                            version,
                        );
                    }
                }
                ExportAction::Favorites(resolve_display_field) => {
                    let lang = LANGUAGE.get(ui.ctx());
                    if let Some(backend) = self.backend.clone() {
                        let version = self.backend.as_ref().and_then(|b| b.game_version())
                            .and_then(|v| GameVersion::new(v).ok())
                            .or_else(|| BACKEND_CONFIG.get(ui.ctx()).and_then(|c| {
                                if let InstallLocation::Web(_, _, v) = &c.location {
                                    v.clone()
                                } else {
                                    None
                                }
                            }));
                        self.command_export_favorites_csv(
                            backend,
                            lang,
                            resolve_display_field,
                            version,
                        );
                    }
                }
            }
        }

        // Handle icon save requests from context menu
        if let Some((icon_id, col_name, sheet_name, save_all, row_keys)) = ICON_SAVE_REQUEST.take(ui.ctx()) {
            if let Some(backend) = self.backend.clone() {
                let hires = ALWAYS_HIRES.get(ui.ctx());
                let lang = LANGUAGE.get(ui.ctx());
                self.command_save_icon(backend, icon_id, &col_name, &sheet_name, save_all, row_keys, hires, lang);
            }
        }
    }

    // ── Version Diff ────────────────────────────────────────────








    fn on_setup(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        _params: &Params<'_, '_>,
    ) -> RouteResponse {
        // If backend doesn't exist yet AND saved config exists, auto-initialize
        if self.backend.is_none() {
            // Try loading from settings.json first (more readable config file)
            let config = crate::config_file::load_backend_config()
                .or_else(|| BACKEND_CONFIG.try_get(ui.ctx()).flatten());
            if let Some(config) = config {
                self.setup_window = None;
                self.auto_init_promise = Some(UnsendPromise::new({
                    let config = config.clone();
                    async move {
                        let backend = Backend::new(config.clone()).await?;
                        Ok((backend, config))
                    }
                }));
                RouteResponse::Title("设置".to_string())
            } else {
                self.setup_window = Some(SetupWindow::from_blank(
                    path.query_pairs().contains_key("redirect"),
                ));
                RouteResponse::Title("设置".to_string())
            }
        } else {
            // Already have a backend: show setup page for reconfiguration
            self.setup_window = Some(SetupWindow::from_config(
                ui.ctx(),
                path.query_pairs().contains_key("redirect"),
            ));
            RouteResponse::Title("设置".to_string())
        }
    }

    fn draw_setup(&mut self, ui: &mut egui::Ui, path: &Path, _params: &Params<'_, '_>) {
        // Check if auto-init is running (from saved config on restart)
        if let Some(promise) = self.auto_init_promise.take() {
            if promise.ready() {
                match promise.block_and_take() {
                    Ok((backend, config)) => {
                        self.backend = Some(backend.clone());
                        self.load_favorites();
                        self.sheet_data.clear();
                        self.schema_data.clear();
                        self.sheet_languages.clear();
                        CURRENT_SHEET_LANGUAGES.remove(ui.ctx());
                        BACKEND_CONFIG.set(ui.ctx(), Some(config.clone()));
                        crate::config_file::save_backend_config(&config);
                        self.auto_init_promise = None;
                        self.init_list_tracker();
                        self.navigate("/sheet");
                    }
                    Err(e) => {
                        log::error!("Auto-init failed, falling back to setup page: {e}");
                        self.setup_window = Some(SetupWindow::from_blank(false));
                        self.auto_init_promise = None;
                    }
                }
            } else {
                // Still loading - put the promise back for next frame
                self.auto_init_promise = Some(promise);
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(100.0);
                        ui.heading("FF14 EXDViewer edit");
                        ui.add_space(10.0);
                        ui.label("正在加载保存的配置...");
                    });
                });
            }
            return;
        }

        if let Some(Some((backend, config))) = self.setup_window.as_mut().map(|w| w.draw(ui.ctx())) {
            self.backend = Some(backend);
            self.load_favorites();
            self.sheet_data.clear();
            self.schema_data.clear();
            self.sheet_languages.clear();
            CURRENT_SHEET_LANGUAGES.remove(ui.ctx());

            BACKEND_CONFIG.set(ui.ctx(), Some(config));

            crate::config_file::save_backend_config(
                &BACKEND_CONFIG.get(ui.ctx()).unwrap(),
            );
            if let Some(redirect_path) = path.query_pairs().get("redirect").map(|s| s.as_str()) {
                self.navigate_replace(redirect_path);
            } else {
                self.navigate("/sheet");
            }
            self.init_list_tracker();
        }
    }

    fn on_auth_callback(
        &mut self,
        _ui: &mut egui::Ui,
        _path: &Path,
        _params: &Params<'_, '_>,
    ) -> RouteResponse {
        RouteResponse::Title("Signing in…".to_string())
    }

    fn draw_auth_callback(&mut self, ui: &mut egui::Ui, _path: &Path, _params: &Params<'_, '_>) {
        pr_window::draw_auth_callback(ui);
    }

    fn ensure_backend(&self, path: &Path) -> Option<RouteResponse> {
        if self.backend.is_none() {
            return Some(RouteResponse::Redirect(Path::with_params(
                "/",
                &[("redirect", path.to_string())],
            )));
        }
        None
    }

    fn on_unnamed_sheet(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        _params: &Params<'_, '_>,
    ) -> RouteResponse {
        if let Some(r) = self.ensure_backend(path) {
            return r;
        }

        if let Some(sheet) = &SELECTED_SHEET.get(ui.ctx()) {
            return RouteResponse::Redirect(format!("/sheet/{sheet}").into());
        }
        RouteResponse::Title("Sheet List".to_string())
    }

    fn on_named_sheet(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        params: &Params<'_, '_>,
    ) -> RouteResponse {
        if let Some(r) = self.ensure_backend(path) {
            return r;
        }
        TEMP_HIGHLIGHTED_ROW.take(ui.ctx());

        if let Some(sheet) = params.get("name") {
            SELECTED_SHEET.set(ui.ctx(), Some(sheet.to_string()));
        } else {
            SELECTED_SHEET.set(ui.ctx(), None);
            return RouteResponse::Redirect("/sheet".into());
        }

        if let Some(mut fragment) = path.fragment() {
            let mut col_nr: Option<u16> = None;
            if let Some((rest, col_str)) = fragment.rsplit_once('C') {
                col_nr = col_str.parse::<u16>().ok();
                fragment = rest;
            }

            let mut row_pos: Option<(u32, Option<u16>)> = None;
            if let Some((_rest, row_str)) = fragment.rsplit_once('R') {
                if let Some((row_str, subrow_str)) = row_str.split_once('.') {
                    let row = row_str.parse::<u32>().ok();
                    let subrow = subrow_str.parse::<u16>().ok();
                    if let Some(row) = row {
                        row_pos = Some((row, subrow));
                    }
                } else if let Ok(row) = row_str.parse::<u32>() {
                    row_pos = Some((row, None));
                }
            }

            if let Some((row, subrow)) = row_pos {
                TEMP_SCROLL_TO.set(ui.ctx(), ((row, subrow), col_nr.unwrap_or_default()));
            }
        }
        RouteResponse::Title(params.get("name").unwrap().to_string())
    }

    fn draw_unnamed_sheet(&mut self, ui: &mut egui::Ui, _path: &Path, _params: &Params<'_, '_>) {
        self.draw_goto(ui.ctx());

        self.draw_sheet_list(ui);
    }

    fn draw_named_sheet(&mut self, ui: &mut egui::Ui, _path: &Path, _params: &Params<'_, '_>) {
        self.draw_goto(ui.ctx());

        // 处理表格展示模式切换后的缓存重载（借用已释放）
        if self.table_layout_reload_pending {
            self.sheet_data.clear();
            self.table_layout_reload_pending = false;
        }

        self.draw_sheet_list(ui);
        self.draw_sheet_data(ui);
    }

    fn on_music(
        &mut self,
        _ui: &mut egui::Ui,
        path: &Path,
        _params: &Params<'_, '_>,
    ) -> RouteResponse {
        if let Some(r) = self.ensure_backend(path) {
            return r;
        }
        RouteResponse::Title("Music".to_string())
    }

    fn on_music_track(
        &mut self,
        _ui: &mut egui::Ui,
        path: &Path,
        params: &Params<'_, '_>,
    ) -> RouteResponse {
        if let Some(r) = self.ensure_backend(path) {
            return r;
        }
        if let Some(id) = params.get("id").and_then(|id| id.parse::<u32>().ok()) {
            self.music.request(id);
        }
        RouteResponse::Title("Music".to_string())
    }

    fn draw_music(&mut self, ui: &mut egui::Ui, _path: &Path, _params: &Params<'_, '_>) {
        // Sync new-only list tracking to music player
        if self.music_list_state.starts_with("ready") {
            let set: std::collections::HashSet<String> = self.music_new_items.iter().cloned().collect();
            self.music.new_paths = set;
        }

        if let Some(backend) = self.backend.clone()
            && let Some(event) = self.music.ui(ui, &backend)
        {
            match event {
                music::MusicEvent::Select(row_id) => {
                    self.navigate(format!("/music/{row_id}"));
                }
                music::MusicEvent::ExportCurrent => {
                    self.command_export_music(&backend, true);
                }
                music::MusicEvent::ExportAll => {
                    self.command_export_music(&backend, false);
                }
            }
        }
    }

    fn on_maps(
        &mut self,
        _ui: &mut egui::Ui,
        path: &Path,
        _params: &Params<'_, '_>,
    ) -> RouteResponse {
        if let Some(r) = self.ensure_backend(path) {
            return r;
        }
        RouteResponse::Title("地图".to_string())
    }

    fn draw_maps(&mut self, ui: &mut egui::Ui, _path: &Path, _params: &Params<'_, '_>) {
        if let Some(backend) = self.backend.clone()
            && let Some(event) = self.map.ui(ui, &backend, LANGUAGE.get(ui.ctx()))
        {
            match event {
                crate::map::MapEvent::Select(_) => {}
                crate::map::MapEvent::ExportCurrent => {
                    self.command_export_map(&backend, true);
                }
                crate::map::MapEvent::ExportAll => {
                    self.command_export_map(&backend, false);
                }
            }
        }
    }

    fn command_open_pr(&mut self) {
        let names: Vec<String> = self
            .get_modified_schemas()
            .iter()
            .map(|(name, _)| (*name).clone())
            .collect();
        self.pr_window.open(&names);
    }

    fn draw_pr_window(&mut self, ctx: &egui::Context) {
        let location = pr_window::github_source(ctx);
        let modified: Vec<(String, Option<String>)> = self
            .get_modified_schemas()
            .iter()
            .map(|(name, schema)| ((*name).clone(), schema.invalid_reason()))
            .collect();
        if let Some(PrAction::Submit { title, body }) =
            self.pr_window.draw(ctx, location.as_ref(), &modified)
            && let Some(location) = &location
        {
            let files: Vec<(String, String)> = self
                .get_modified_schemas()
                .into_iter()
                .map(|(name, schema)| (format!("{name}.yml"), schema.get_text().clone()))
                .collect();
            self.pr_window.submit(location, title, body, files);
        }
    }

    fn get_modified_schemas(&self) -> Vec<(&String, &EditableSchema)> {
        self.schema_data
            .iter()
            .filter_map(|(name, schema)| schema.try_get().ok().map(|s| (name, s)))
            .filter_map(|(name, schema)| schema.as_ref().ok().map(|s| (name, s)))
            .filter(|(_, schema)| schema.is_modified())
            .collect()
    }

    // ── New-only list tracking ─────────────────────────────────────

    fn init_list_tracker(&mut self) {
        let Some(backend) = self.backend.as_ref() else {
            self.sheet_list_state = "error:后端未初始化".to_string();
            self.music_list_state = "error:后端未初始化".to_string();
            return;
        };

        let version = match backend.game_version() {
            Some(v) => v.to_string(),
            None => {
                // Web mode or no file access – feature not available
                self.sheet_list_state = "error:网络模式不支持".to_string();
                self.music_list_state = "error:网络模式不支持".to_string();
                return;
            }
        };

        // ── Sheet list ──────────────────────────────────────────
        let sheet_names: Vec<String> = backend
            .excel()
            .get_entries()
            .iter()
            .map(|(name, _)| name.clone())
            .sorted()
            .collect();

        let safe_ver = version.replace('.', "_");
        let cur_path = std::path::PathBuf::from("config")
            .join(format!("sheet_list_{safe_ver}.json"));
        let sheet_list = crate::list_tracker::VersionedList {
            version: version.clone(),
            items: sheet_names.clone(),
        };
        crate::list_tracker::save_list(&cur_path, &sheet_list);

        // Find previous version's list
        let prev_path = Self::find_prev_list("sheet_list_", &safe_ver);
        let prev_list = prev_path.as_ref().and_then(|p| crate::list_tracker::load_list(p));

        log::debug!("Sheet list: {} items, prev={:?}", sheet_names.len(), prev_path);
        self.sheet_list_state = match crate::list_tracker::compare_lists(
            &version, &sheet_list, prev_list.as_ref(),
        ) {
            crate::list_tracker::ComparisonResult::NoPrevious => {
                "no_prev".to_string()
            }
            crate::list_tracker::ComparisonResult::SameVersion => {
                "no_changes".to_string()
            }
            crate::list_tracker::ComparisonResult::NewItems(items) => {
                let count = items.len();
                log::debug!("New sheet items ({}): {:?}", count, &items[..count.min(5)]);
                // Cross-validate against actual excel entries
                let valid: std::collections::HashSet<&str> = backend.excel().get_entries().keys().map(String::as_str).collect();
                let validated: Vec<String> = items.into_iter().filter(|name| valid.contains(name.as_str())).collect();
                log::debug!("Validated new items: {} (of {} raw)", validated.len(), count);
                self.sheet_new_items = validated;
                format!("ready:{count}")
            }
            crate::list_tracker::ComparisonResult::Error(e) => format!("error:{e}"),
        };
        crate::list_tracker::cleanup_lists("sheet_list_", &version);

        // ── Music list (async) ───────────────────────────────────
        let excel = backend.excel().clone();
        let version_clone = version.clone();
        self.list_promise = Some(TrackedPromise::spawn_local(async move {
            let items = Self::load_music_list(excel).await;
            let safe_ver = version_clone.replace('.', "_");
            let cur_path = std::path::PathBuf::from("config")
                .join(format!("music_list_{safe_ver}.json"));
            let music_list = crate::list_tracker::VersionedList {
                version: version_clone.clone(),
                items,
            };
            crate::list_tracker::save_list(&cur_path, &music_list);
        }));
    }

    fn poll_list_promise(&mut self) {
            if let Some(promise) = self.list_promise.as_mut() {
                if promise.try_get().is_some() {
                    let _ = self.list_promise.take();
                    // Music list saved; now compare
                    if let Some(backend) = self.backend.as_ref() {
                        if let Some(version) = backend.game_version() {
                            let safe_ver = version.replace('.', "_");
                            let cur_path = std::path::PathBuf::from("config")
                                .join(format!("music_list_{safe_ver}.json"));
                            if let Some(list) = crate::list_tracker::load_list(&cur_path) {
                                let prev_path = Self::find_prev_list("music_list_", &safe_ver);
                                let prev = prev_path.as_ref().and_then(crate::list_tracker::load_list);
                                self.music_list_state = match crate::list_tracker::compare_lists(
                                    version, &list, prev.as_ref(),
                                ) {
                                    crate::list_tracker::ComparisonResult::NoPrevious => "no_prev".into(),
                                    crate::list_tracker::ComparisonResult::SameVersion => "no_changes".into(),
                                    crate::list_tracker::ComparisonResult::NewItems(items) => {
                                        let c = items.len();
                                        log::debug!("Music new items: {} raw", c);
                                        self.music_new_items = items;
                                        format!("ready:{c}")
                                    }
                                    crate::list_tracker::ComparisonResult::Error(e) => format!("error:{e}"),
                                };
                                crate::list_tracker::cleanup_lists("music_list_", version);
                            }
                        }
                    }
                }
            }
            }

    fn find_prev_list(prefix: &str, current_safe: &str) -> Option<std::path::PathBuf> {
        let dir = std::path::PathBuf::from("config");
        let Ok(entries) = std::fs::read_dir(&dir) else { return None };
        entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| name.starts_with(prefix) && !name.contains(current_safe))
            })
            .next()
    }

    async fn load_music_list(excel: crate::excel::base::CachedProvider) -> Vec<String> {
        use crate::excel::provider::ExcelSheet;
        let Ok(sheet) = excel.get_sheet("BGM", ironworks::excel::Language::None).await else {
            return Vec::new();
        };
        let offset = sheet.columns().first().map(|c| u32::from(c.offset())).unwrap_or(0);
        let mut items = Vec::new();
        for row_id in sheet.get_row_ids() {
            let Ok(row) = sheet.get_row(row_id) else { continue };
            let Ok(cell) = row.read_string(offset) else { continue };
            let path = String::from_utf8_lossy(cell.as_bytes()).into_owned();
            if path.ends_with(".scd") {
                items.push(path);
            }
        }
        items
    }

    fn favorites_path() -> std::path::PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("config").join("favorites.json")))
            .unwrap_or_else(|| std::path::PathBuf::from("config/favorites.json"))
    }

    fn load_favorites(&mut self) {
        let path = Self::favorites_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(names) = serde_json::from_str::<Vec<String>>(&content) {
                self.favorites = names.into_iter().collect();
                log::info!("已加载 {} 个收藏数据表", self.favorites.len());
            }
        }
    }

    fn save_favorites(&self) {
        let path = Self::favorites_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let names: Vec<&String> = self.favorites.iter().collect();
        if let Ok(content) = serde_json::to_string_pretty(&names) {
            let _ = std::fs::write(&path, &content);
        }
    }

    fn toggle_favorite(&mut self, sheet_name: &str) {
        if self.favorites.contains(sheet_name) {
            self.favorites.remove(sheet_name);
        } else {
            self.favorites.insert(sheet_name.to_string());
        }
        self.save_favorites();
    }

    fn command_export_favorites_csv(
        &mut self,
        backend: Backend,
        lang: Language,
        resolve_display_field: bool,
        version: Option<GameVersion>,
    ) {
        let export_base = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("export").join("data")))
            .unwrap_or_else(|| std::path::PathBuf::from("export/data"));

        let version_dir_name = version
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "local".to_string());
        let export_dir = if resolve_display_field {
            export_base.join(&version_dir_name)
        } else {
            export_base.join(&version_dir_name)
        };
        let _ = std::fs::create_dir_all(&export_dir);

        let favorite_names: Vec<String> = self.favorites.iter().cloned().collect();
        let total = favorite_names.len();
        if total == 0 {
            log::info!("没有收藏的数据表");
            return;
        }
        let excel = backend.excel().clone();

        let progress = self.export_progress.clone();
        *progress.lock().unwrap() = Some(ExportProgress {
            active: true,
            title: "导出收藏的CSV".into(),
            current: 0,
            total,
            current_name: String::new(),
            done: false,
            error: None,
            done_at: None,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        self.export_promise = Some(TrackedPromise::spawn_local(async move {
            for (i, sheet_name) in favorite_names.iter().enumerate() {
                // 检查中断导出请求
                if progress.lock().unwrap().as_ref().map_or(false, |p| {
                    p.cancel.load(std::sync::atomic::Ordering::Relaxed)
                }) {
                    log::info!("导出已中断");
                    if let Some(p) = progress.lock().unwrap().as_mut() {
                        p.done = true;
                        p.active = false;
                        p.error = Some("已中断导出".into());
                    }
                    break;
                }
                let file_name = format!("{}.csv", sheet_name.replace('/', "_"));
                {
                    if let Some(p) = progress.lock().unwrap().as_mut() {
                        p.current = i + 1;
                        p.current_name = sheet_name.clone();
                    }
                }
                let out_path = export_dir.join(&file_name);

                match excel.get_sheet(sheet_name, lang).await {
                    Ok(sheet) => {
                        let editable = crate::config_file::load_schema_for_export(
                            &backend,
                            &sheet_name,
                        ).await;
                        let schema = editable.as_ref().and_then(|e| e.get_schema());
                        let context = TableContext::new(
                            crate::sheet::GlobalContext::new(
                                egui::Context::default(),
                                backend.clone(),
                                lang,
                                IconManager::new(),
                            ),
                            sheet,
                            schema,
                        );
                        match export_csv(context, resolve_display_field).await {
                            Ok(data) => {
                                if let Err(e) = std::fs::write(&out_path, &data) {
                                    log::error!("导出收藏 {sheet_name} 失败: {e}");
                                } else {
                                    log::info!(
                                        "已导出 ({}): {}",
                                        i + 1,
                                        out_path.display()
                                    );
                                }
                            }
                            Err(e) => log::error!("生成CSV收藏 {sheet_name} 失败: {e}"),
                        }
                    }
                    Err(e) => log::error!("读取收藏数据表 {sheet_name} 失败: {e}"),
                }
            }
            log::info!("收藏CSV导出完成，共 {total} 张数据表");
            if let Some(p) = progress.lock().unwrap().as_mut() {
                p.done = true;
                p.active = false;
            }
        }));
    }

    fn command_export_csv(&mut self, context: TableContext, resolve_display_field: bool) {
        let file_name = format!("{}.csv", context.sheet().name().replace('/', "_"));
        let export_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("export").join("data")))
            .unwrap_or_else(|| std::path::PathBuf::from("export/data"));
        let _ = std::fs::create_dir_all(&export_dir);

        self.export_promise = Some(TrackedPromise::spawn_local(async move {
            let data = match export_csv(context, resolve_display_field).await {
                Ok(data) => data,
                Err(e) => {
                    log::error!("Failed to export CSV: {e:?}");
                    return;
                }
            };

            if let Some(file) = rfd::AsyncFileDialog::new()
                .set_title("导出CSV")
                .set_directory(&export_dir)
                .set_file_name(file_name)
                .save_file()
                .await
            {
                if let Err(e) = file.write(&data).await {
                    log::error!("写入CSV文件失败: {e}");
                } else {
                    log::info!("CSV导出成功");
                }
            }
        }));
    }

    fn command_save_icon(
        &mut self,
        backend: Backend,
        icon_id: u32,
        col_name: &str,
        sheet_name: &str,
        save_all: bool,
        row_keys: Option<Vec<String>>,
        hires: bool,
        lang: Language,
    ) {
        let export_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("export").join("img")))
            .unwrap_or_else(|| std::path::PathBuf::from("export/img"));
        let _ = std::fs::create_dir_all(&export_dir);

        let excel = backend.excel().clone();
        let col_name = col_name.to_string();
        let safe_col = col_name.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_");
        let sheet_name = sheet_name.to_string();
        let row_keys = row_keys.map(|v| v.into_iter().filter_map(|k| k.parse::<u32>().ok()).collect::<Vec<u32>>());

        // 导出此列全部图片：显示进度窗口（单张保存不显示）
        let progress = self.export_progress.clone();
        if save_all {
            *progress.lock().unwrap() = Some(ExportProgress {
                active: true,
                title: format!("导出图标：{sheet_name}-{col_name}"),
                current: 0,
                total: 0,
                current_name: String::new(),
                done: false,
                error: None,
                done_at: None,
                cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            });
        }

        self.export_promise = Some(TrackedPromise::spawn_local(async move {
            if save_all {
                // Save all icons from the target column
                // 通过 schema（yml）按列名定位列：exh 列本身没有名字
                use crate::sheet::TableContext;
                match excel.get_sheet(&sheet_name, lang).await {
                    Ok(sheet) => {
                        // 加载真实 schema（yml），按列名定位列
                        let editable = crate::config_file::load_schema_for_export(
                            &backend, &sheet_name,
                        )
                        .await;
                        let schema = editable.as_ref().and_then(|e| e.get_schema());
                        let context = TableContext::new(
                            crate::sheet::GlobalContext::new(
                                egui::Context::default(),
                                backend.clone(),
                                lang,
                                IconManager::new(),
                            ),
                            sheet,
                            schema,
                        );
                        // 按列名找到目标图标列
                        let col_count = context.column_count();
                        let mut target = None;
                        for ci in 0..col_count {
                            if let Ok(((sc, shc), _)) = context.get_column_by_index(ci as u32) {
                                if sc.name() == col_name {
                                    target = Some((shc.offset() as u32, shc.kind()));
                                    break;
                                }
                            }
                        }
                        let Some((col_offset, col_kind)) = target else {
                            log::warn!("批量导出图标失败：未找到列 {col_name}（{sheet_name}）");
                            if let Some(p) = progress.lock().unwrap().as_mut() {
                                p.done = true;
                                p.active = false;
                                p.error = Some(format!("未找到列 {col_name}"));
                            }
                            return;
                        };
                        // 差异行过滤：仅导出指定 row_ids 对应的行（diff表格新增行）
                        let row_ids: Vec<u32> = if let Some(keys) = &row_keys {
                            keys.clone()
                        } else {
                            context.sheet().get_row_ids().collect()
                        };
                        let total = row_ids.len();
                        if let Some(p) = progress.lock().unwrap().as_mut() {
                            p.total = total;
                        }
                        log::info!("开始批量导出 {sheet_name} 的图标（列 {col_name}），共 {total} 行");
                        let mut saved = 0usize;
                        for (ri, row_id) in row_ids.iter().enumerate() {
                            // 检查中断导出请求
                            if progress.lock().unwrap().as_ref().map_or(false, |p| {
                                p.cancel.load(std::sync::atomic::Ordering::Relaxed)
                            }) {
                                log::info!("图标导出已中断");
                                if let Some(p) = progress.lock().unwrap().as_mut() {
                                    p.done = true;
                                    p.active = false;
                                    p.error = Some("已中断导出".into());
                                }
                                return;
                            }
                            if let Some(p) = progress.lock().unwrap().as_mut() {
                                p.current = ri + 1;
                                p.current_name = format!("Row {row_id}");
                            }
                            if ri % 50 == 0 {
                                crate::utils::yield_to_ui().await;
                            }
                            let Ok(row) = context.sheet().get_row(*row_id) else { continue };
                            let val: i64 = crate::sheet::cell::read_integer(
                                row, col_offset, col_kind,
                            ).unwrap_or(0);
                            if val <= 0 || val > u32::MAX as i64 { continue; }
                            let id = val as u32;
                            match excel.get_icon(id, hires).await {
                                Ok(either::Either::Right(img)) => {
                                    let mut buf = std::io::Cursor::new(Vec::new());
                                    if img.write_to(&mut buf, image::ImageFormat::Png).is_ok() {
                                        let fname = format!("{sheet_name}-{safe_col}-{id}.png");
                                        if std::fs::write(export_dir.join(&fname), buf.into_inner()).is_ok() {
                                            saved += 1;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        if let Some(p) = progress.lock().unwrap().as_mut() {
                            p.done = true;
                            p.active = false;
                        }
                        log::info!("图标批量导出完成: {sheet_name}，共保存 {saved} 个（目录: {}）", export_dir.display());
                    }
                    Err(e) => {
                        log::error!("读取数据表 {sheet_name} 失败: {e}");
                        if let Some(p) = progress.lock().unwrap().as_mut() {
                            p.done = true;
                            p.active = false;
                            p.error = Some(e.to_string());
                        }
                    }
                }
                return;
            }

            // Single icon save
            match excel.get_icon(icon_id, hires).await {
                Ok(either) => {
                    match either {
                        either::Either::Left(url) => {
                            log::warn!("图标来自URL，暂不支持保存: {url}");
                            return;
                        }
                        either::Either::Right(image) => {
                            let mut buf = std::io::Cursor::new(Vec::new());
                            if image.write_to(&mut buf, image::ImageFormat::Png).is_err() {
                                log::error!("编码PNG失败");
                                return;
                            }
                            let bytes = buf.into_inner();

                            // Single save: show file dialog
                            let default_name = if !col_name.is_empty() {
                                format!("{sheet_name}-{safe_col}-{icon_id}.png")
                            } else {
                                format!("{sheet_name}-{icon_id}.png")
                            };
                            if let Some(file) = rfd::AsyncFileDialog::new()
                                    .set_title("保存图片")
                                    .set_directory(&export_dir)
                                    .set_file_name(&default_name)
                                    .save_file()
                                    .await
                                {
                                    if let Err(e) = file.write(&bytes).await {
                                        log::error!("保存图标失败: {e}");
                                    } else {
                                        log::info!("图标已保存: {}", file.file_name());
                                    }
                                }
                        }
                    }
                }
                Err(e) => log::error!("读取图标 {icon_id} 失败: {e}"),
            }
        }));
    }


    fn execute_delete_csv_versions(&mut self) {
        let versions: Vec<String> = self.delete_csv_selected.iter().cloned().collect();
        self.delete_csv_selected.clear();
        Self::delete_csv_versions_impl(versions);
    }

    fn delete_csv_versions_impl(versions: Vec<String>) {
        let export_base = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("export").join("data")))
            .unwrap_or_else(|| std::path::PathBuf::from("export/data"));
        let mut deleted_dirs = 0usize;
        for v in &versions {
            let dir = export_base.join(v);
            if !dir.is_dir() {
                log::warn!("目录不存在，跳过: {}", dir.display());
                continue;
            }
            let mut removed_files = 0usize;
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file()
                        && path.extension().map(|e| e.eq_ignore_ascii_case("csv")).unwrap_or(false)
                    {
                        match std::fs::remove_file(&path) {
                            Ok(_) => removed_files += 1,
                            Err(e) => log::error!("删除文件失败 {}: {e}", path.display()),
                        }
                    }
                }
            }
            // 若目录已空则删除目录本身
            let is_empty = std::fs::read_dir(&dir)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false);
            if is_empty {
                match std::fs::remove_dir(&dir) {
                    Ok(_) => deleted_dirs += 1,
                    Err(e) => log::warn!("删除目录失败 {}: {e}", dir.display()),
                }
            }
            log::info!("已删除版本 {v} 的 {removed_files} 个CSV文件");
        }
        log::info!("CSV删除完成，共删除 {deleted_dirs} 个空目录");
    }

    /// 保存地图图片：export_current=true 保存当前选中的一张，false 保存全部。
    /// 带进度窗口（与其他批量导出一致）。
    fn command_export_map(&mut self, backend: &Backend, export_current: bool) {
        let export_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("export").join("map")))
            .unwrap_or_else(|| std::path::PathBuf::from("export/map"));
        let _ = std::fs::create_dir_all(&export_dir);

        let maps: Vec<crate::map::MapRow> = if export_current {
            self.map
                .selected
                .and_then(|i| self.map.rows.get(i).cloned())
                .into_iter()
                .collect()
        } else {
            self.map.rows.clone()
        };
        if maps.is_empty() {
            log::info!("没有可导出的地图");
            return;
        }
        let total = maps.len();
        log::info!("开始导出地图（{}），共 {total} 个", if export_current { "当前" } else { "全部" });

        let progress = self.export_progress.clone();
        *progress.lock().unwrap() = Some(ExportProgress {
            active: true,
            title: if export_current { "保存地图".into() } else { "保存全部地图".into() },
            current: 0,
            total,
            current_name: String::new(),
            done: false,
            error: None,
            done_at: None,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        let files = backend.files().clone();
        self.export_promise = Some(TrackedPromise::spawn_local(async move {
            let mut saved = 0usize;
            for (i, m) in maps.iter().enumerate() {
                // 检查中断导出请求
                if progress.lock().unwrap().as_ref().map_or(false, |p| {
                    p.cancel.load(std::sync::atomic::Ordering::Relaxed)
                }) {
                    log::info!("地图导出已中断");
                    if let Some(p) = progress.lock().unwrap().as_mut() {
                        p.done = true;
                        p.active = false;
                        p.error = Some("已中断导出".into());
                    }
                    return;
                }
                if let Some(p) = progress.lock().unwrap().as_mut() {
                    p.current = i + 1;
                    p.current_name = m.code.clone();
                }
                match crate::map::load_map_texture(&*files, &m.code).await {
                    Ok(img) => {
                        let fname = crate::map::map_file_name(m);
                        let out_path = export_dir.join(&fname);
                        if image::DynamicImage::ImageRgba8(img)
                            .save(&out_path)
                            .is_ok()
                        {
                            saved += 1;
                        } else {
                            log::error!("保存地图 {fname} 失败");
                        }
                    }
                    Err(e) => {
                        log::error!("读取地图 {} 失败: {e}", m.code);
                    }
                }
                if i % 5 == 0 {
                    crate::utils::yield_to_ui().await;
                }
            }
            if let Some(p) = progress.lock().unwrap().as_mut() {
                p.done = true;
                p.active = false;
            }
            log::info!("地图导出完成：共保存 {saved} 个（目录: {}）", export_dir.display());
        }));
    }

    fn command_export_music(&mut self, backend: &Backend, export_current: bool) {
        use ironworks::file::scd::{Codec, SoundContainer};
        use std::io::Cursor;

        let export_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("export").join("music")))
            .unwrap_or_else(|| std::path::PathBuf::from("export/music"));
        let _ = std::fs::create_dir_all(&export_dir);

        let files = backend.files().clone();

        // Extract data before spawning async (avoid borrowing self in the future)
        let tracks_for_export: Vec<(u32, String)> = if export_current {
            self.music
                .now_playing
                .as_ref()
                .map(|n| vec![(n.row_id, n.path.clone())])
                .unwrap_or_default()
        } else {
            self.music
                .rows
                .iter()
                .filter(|row| row.available)
                .map(|row| (row.row_id, row.path.clone()))
                .collect()
        };

        let progress = self.export_progress.clone();
        if !export_current {
            *progress.lock().unwrap() = Some(ExportProgress {
                active: true,
                title: "导出全部音乐".into(),
                current: 0,
                total: tracks_for_export.len(),
                current_name: String::new(),
                done: false,
                error: None,
                done_at: None,
                cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            });
        }

        self.export_promise = Some(TrackedPromise::spawn_local(async move {
            let total = tracks_for_export.len();
            for (i, (_, track_path)) in tracks_for_export.iter().enumerate() {
                // 检查中断导出请求
                if progress.lock().unwrap().as_ref().map_or(false, |p| {
                    p.cancel.load(std::sync::atomic::Ordering::Relaxed)
                }) {
                    log::info!("导出已中断");
                    if let Some(p) = progress.lock().unwrap().as_mut() {
                        p.done = true;
                        p.active = false;
                        p.error = Some("已中断导出".into());
                    }
                    break;
                }
                // Yield to keep UI responsive during batch export
                crate::utils::yield_to_ui().await;
                let stem = std::path::Path::new(&track_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                {
                    if let Some(p) = progress.lock().unwrap().as_mut() {
                        p.current = i + 1;
                        p.current_name = stem.to_string();
                    }
                }

                // Read SCD file and extract the actual audio data with correct extension
                let result = async {
                    let raw_bytes = files.file::<Vec<u8>>(track_path).await?;
                    let container = SoundContainer::read(Cursor::new(raw_bytes))?;
                    let entry = container
                        .sound(0)
                        .ok_or_else(|| anyhow::anyhow!("no audio entry in SCD"))?;
                    let ext = match entry.format() {
                        Codec::OggVorbis => "ogg",
                        Codec::Hca => "hca",
                        Codec::Mp3 => "mp3",
                        Codec::MsAdpcm => "wav",
                        Codec::Atrac9 => "at9",
                        Codec::Pcm => "wav",
                        Codec::Empty | Codec::Unknown(_) => "bin",
                    };
                    let file_name = format!("{stem}.{ext}");
                    anyhow::Ok((file_name, entry.data().to_vec()))
                }
                .await;

                match result {
                    Ok((file_name, audio_data)) => {
                        if total == 1 {
                            // Single track: use file save dialog
                            if let Some(file) = rfd::AsyncFileDialog::new()
                                .set_title("导出音频")
                                .set_directory(&export_dir)
                                .set_file_name(file_name)
                                .save_file()
                                .await
                            {
                                let saved_path = file.path().to_path_buf();
                                if let Err(e) = file.write(&audio_data).await {
                                    log::error!("导出音频 {stem} 失败: {e}");
                                } else {
                                    log::info!("音频导出成功: {stem}");
                                    convert_hca_to_wav(&saved_path);
                                }
                            }
                        } else {
                            // Batch export: write directly to export directory
                            let out_path = export_dir.join(&file_name);
                            if let Err(e) = std::fs::write(&out_path, &audio_data) {
                                log::error!("导出音频 {stem} 失败: {e}");
                            } else {
                                log::info!("已导出 ({}/{total}): {}", i + 1, out_path.display());
                            }
                        }
                    }
                    Err(e) => log::error!("读取音频 {track_path} 失败: {e}"),
                }
            }
            if total > 1 {
                log::info!("音乐导出完成，共 {total} 首");
            }
            if let Some(p) = progress.lock().unwrap().as_mut() {
                p.done = true;
                p.active = false;
            }
        }));
    }

    fn command_export_all_csv(
        &mut self,
        backend: Backend,
        lang: Language,
        resolve_display_field: bool,
        version: Option<GameVersion>,
    ) {
        let export_base = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("export").join("data")))
            .unwrap_or_else(|| std::path::PathBuf::from("export/data"));

        let version_dir_name = version
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "local".to_string());
        let export_dir = if resolve_display_field {
            export_base.join(&version_dir_name)
        } else {
            export_base.join(&version_dir_name).join("raw")
        };
        let _ = std::fs::create_dir_all(&export_dir);

        let sheets: Vec<String> = backend
            .excel()
            .get_entries()
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let total = sheets.len();
        let excel = backend.excel().clone();

        let progress = self.export_progress.clone();
        *progress.lock().unwrap() = Some(ExportProgress {
            active: true,
            title: "导出全部CSV".into(),
            current: 0,
            total,
            current_name: String::new(),
            done: false,
            error: None,
            done_at: None,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        self.export_promise = Some(TrackedPromise::spawn_local(async move {
            for (i, sheet_name) in sheets.iter().enumerate() {
                // 检查中断导出请求
                if progress.lock().unwrap().as_ref().map_or(false, |p| {
                    p.cancel.load(std::sync::atomic::Ordering::Relaxed)
                }) {
                    log::info!("导出已中断");
                    if let Some(p) = progress.lock().unwrap().as_mut() {
                        p.done = true;
                        p.active = false;
                        p.error = Some("已中断导出".into());
                    }
                    break;
                }
                let file_name = format!("{}.csv", sheet_name.replace('/', "_"));
                {
                    if let Some(p) = progress.lock().unwrap().as_mut() {
                        p.current = i + 1;
                        p.current_name = sheet_name.clone();
                    }
                }
                let out_path = export_dir.join(&file_name);

                match excel.get_sheet(sheet_name, lang).await {
                    Ok(sheet) => {
                        let editable =
                            crate::editable_schema::EditableSchema::from_miscellaneous(
                            sheet_name,
                        )
                            .ok();
                        let schema = editable.as_ref().and_then(|e| e.get_schema());
                        let context = TableContext::new(
                            crate::sheet::GlobalContext::new(
                                egui::Context::default(),
                                backend.clone(),
                                lang,
                                IconManager::new(),
                            ),
                            sheet,
                            schema,
                        );
                        match export_csv(context, resolve_display_field).await {
                            Ok(data) => {
                                if let Err(e) = std::fs::write(&out_path, &data) {
                                    log::error!("导出 {sheet_name} 失败: {e}");
                                } else {
                                    log::info!(
                                        "已导出 ({}/{total}): {}",
                                        i + 1,
                                        out_path.display()
                                    );
                                }
                            }
                            Err(e) => log::error!("生成CSV {sheet_name} 失败: {e}"),
                        }
                    }
                    Err(e) => log::error!("读取数据表 {sheet_name} 失败: {e}"),
                }
            }
            log::info!("全部CSV导出完成，共 {total} 张数据表");
            if let Some(p) = progress.lock().unwrap().as_mut() {
                p.done = true;
                p.active = false;
            }
        }));
    }

    fn command_save_all_schemas(&mut self) {
        let backend = self.backend.as_ref().unwrap();
        let modified_schemas = self.get_modified_schemas();

        if modified_schemas.is_empty() {
            log::info!("No modified schemas to save.");
            return;
        }

        let provider = backend.schema();
        let start_dir = provider
            .can_save_schemas()
            .then(|| provider.save_schema_start_dir())
            .flatten();

        if provider.can_save_schemas() {
            for (_, schema) in modified_schemas {
                schema.command_save(provider);
            }
        } else if let Ok((_, schema)) = modified_schemas.iter().exactly_one() {
            schema.command_save_as(provider);
        } else {
            let create_archive = || -> Result<Vec<u8>> {
                let mut archive = ZipWriter::new(std::io::Cursor::new(Vec::new()));
                for (sheet_name, schema) in modified_schemas {
                    archive
                        .start_file(format!("{sheet_name}.yml"), SimpleFileOptions::default())?;
                    archive.write_all(schema.get_text().as_bytes())?;
                }
                Ok(archive.finish()?.into_inner())
            };

            let archive = match create_archive() {
                Ok(archive) => archive,
                Err(e) => {
                    log::error!("Failed to create schema archive: {e}");
                    return;
                }
            };

            self.save_promise = Some(TrackedPromise::spawn_local(async move {
                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_title("另存数据结构定义为")
                    .set_file_name("schemas.zip");
                if let Some(start_dir) = start_dir {
                    dialog = dialog.set_directory(start_dir);
                }
                if let Some(file) = dialog.save_file().await {
                    if let Err(e) = file.write(&archive).await {
                        log::error!("Failed to save schemas: {e}");
                    } else {
                        log::info!("Saved all saved successfully");
                    }
                }
            }));
        }
    }
}

impl App {
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::column_layout::ensure_default_layout_file();
        install_image_loaders(&cc.egui_ctx);
        Self::apply_fonts(&cc.egui_ctx, None);
        Self::setup_theme(&cc.egui_ctx);

        Self {
            router: Rc::new(OnceCell::new()),
            icon_manager: IconManager::new(),
            setup_window: None,
            backend: None,
            favorites: std::collections::HashSet::new(),
            sheet_data: LruCache::new(NonZero::new(32).unwrap()),
            schema_data: LruCache::unbounded(),
            sheet_languages: LruCache::unbounded(),
            sheet_matcher: FuzzyMatcher::new(),
            sheet_filter_data: LruCache::new(NonZero::new(8).unwrap()),
            changed_schemas: None,
            save_promise: None,
            export_promise: None,
            export_progress: std::sync::Arc::new(std::sync::Mutex::new(None)),
            list_promise: None,
            diff_result: std::sync::Arc::new(std::sync::Mutex::new(None)),
            auto_init_promise: None,
            pr_window: PrWindow::default(),
            goto_window: None,
            about_open: false,
            music: music::MusicPlayer::default(),
            map: crate::map::MapViewer::default(),
            last_system_theme: None,
            loaded_cjk: None,
            #[cfg(target_arch = "wasm32")]
            font_promise: None,

            show_new_sheets_only: false,
            show_new_music_only: false,
            sheet_new_items: Vec::new(),
            music_new_items: Vec::new(),
            sheet_list_state: "loading".to_string(),
            music_list_state: "loading".to_string(),
            diff_state: crate::diff::DiffState::new(),
            download_exdschema_status: std::sync::Arc::new(std::sync::Mutex::new(crate::downloader::DownloadStatus::Idle)),
            download_hca_status: std::sync::Arc::new(std::sync::Mutex::new(crate::downloader::DownloadStatus::Idle)),

            // ── 表格展示模式切换 ──
            table_layout_full: false,
            table_layout_confirm: false,
            table_layout_switching: false,
            table_layout_switch_start: None,
            table_layout_reload_pending: false,

            // ── 删除CSV版本窗口 ──
            delete_csv_open: false,
            delete_csv_versions: Vec::new(),
            delete_csv_selected: std::collections::HashSet::new(),
            delete_csv_confirming: false,
            delete_csv_done_at: None,
        }
    }

    fn apply_fonts(ctx: &egui::Context, cjk: Option<(String, Arc<FontData>)>) {
        let mut fonts = FontDefinitions::default();

        fonts.font_data.insert(
            "FFXIV-PrivateUseIcons".to_owned(),
            Arc::new(FontData::from_static(include_bytes!(
                "../assets/FFXIV_Lodestone_SSF.ttf"
            ))),
        );
        let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
        proportional.push("FFXIV-PrivateUseIcons".to_owned());

        if let Some((name, data)) = cjk {
            fonts.font_data.insert(name.clone(), data);
            // 中文字体同时注册到 Proportional 与 Monospace 字体族，
            // 否则日志窗口(egui_logger用monospace渲染)中文显示为"口"
            let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
            proportional.push(name.clone());
            let monospace = fonts.families.entry(FontFamily::Monospace).or_default();
            monospace.push(name);
        }

        ctx.set_fonts(fonts);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn update_fonts(&mut self, ctx: &egui::Context) {
        let wanted = CjkFont::for_language(LANGUAGE.get(ctx));
        if wanted == self.loaded_cjk {
            return;
        }
        let cjk = wanted.map(|font| {
            (
                font.family_name().to_owned(),
                Arc::new(FontData::from_static(font.embedded_bytes())),
            )
        });
        Self::apply_fonts(ctx, cjk);
        self.loaded_cjk = wanted;
    }

    #[cfg(target_arch = "wasm32")]
    fn update_fonts(&mut self, ctx: &egui::Context) {
        let wanted = CjkFont::for_language(LANGUAGE.get(ctx));
        if wanted == self.loaded_cjk {
            return;
        }

        let Some(font) = wanted else {
            Self::apply_fonts(ctx, None);
            self.loaded_cjk = None;
            self.font_promise = None;
            return;
        };

        if self.font_promise.as_ref().is_some_and(|(f, _)| *f == font) {
            if !self.font_promise.as_ref().unwrap().1.ready() {
                return;
            }
            let (_, promise) = self.font_promise.take().unwrap();
            match promise.block_and_take() {
                Ok(bytes) => Self::apply_fonts(
                    ctx,
                    Some((
                        font.family_name().to_owned(),
                        Arc::new(FontData::from_owned(bytes)),
                    )),
                ),
                Err(error) => log::error!("Failed to fetch font {}: {error}", font.asset_file()),
            }
            self.loaded_cjk = Some(font);
            return;
        }

        let file = font.asset_file().to_owned();
        self.font_promise = Some((
            font,
            UnsendPromise::new(async move { crate::utils::fetch_url(file).await }),
        ));
    }

    fn setup_theme(ctx: &egui::Context) {
        // First launch: use Macchiato as default color theme
        if COLOR_THEME.try_get(ctx).is_none() {
            COLOR_THEME.set(ctx, ColorTheme::Macchiato);
        }
        COLOR_THEME.get(ctx).apply(ctx);
        let solid_scrollbar = SOLID_SCROLLBAR.get(ctx);
        ctx.all_styles_mut(|s| {
            s.spacing.scroll = if solid_scrollbar {
                ScrollStyle::solid()
            } else {
                ScrollStyle::default()
            };
        });
    }

    fn follow_system_theme(&mut self, ctx: &egui::Context) {
        if COLOR_THEME.get(ctx) != ColorTheme::System {
            return;
        }
        let system = ctx.system_theme();
        if system != self.last_system_theme {
            self.last_system_theme = system;
            ColorTheme::System.apply(ctx);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.follow_system_theme(ui.ctx());
        self.draw(ui);
        tick_promises(ui.ctx());
    }
}


/// After a music file is exported, if it's HCA format, convert to WAV using hca.exe.
fn convert_hca_to_wav(file_path: &std::path::Path) {
    let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");
    if ext.to_ascii_lowercase() != "hca" { return; }
    let hca_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("tools").join("hca.exe")))
        .filter(|p| p.exists())
        .or_else(|| {
            let dev = std::path::PathBuf::from("tools").join("hca.exe");
            dev.exists().then_some(dev)
        });
    let Some(hca_path) = hca_exe else {
        log::warn!("hca.exe 未找到，跳过HCA转WAV转码");
        return;
    };
    log::info!("转码 HCA→WAV: {}", file_path.display());
    let _ = std::process::Command::new(&hca_path)
        .arg("-a").arg("E0748978")
        .arg("-b").arg("CF222F1F")
        .arg(file_path.as_os_str())
        .spawn();
    log::info!("已启动 hca.exe 转码任务");
}

fn add_links(ui: &mut egui::Ui, open_about: &mut bool) {
    ui.with_layout(Layout::right_to_left(ui.layout().vertical_align()), |ui| {
        if ui
            .link(format!("FF14 EXDViewer edit v{}", crate::build::PKG_VERSION))
            .clicked()
        {
            *open_about = true;
        }
        ui.label("/");
        ui.add(
            egui::Hyperlink::from_label_and_url(
                format!("在 {} 为我点Star", egui::special_emojis::GITHUB),
                crate::REPO_URL,
            )
            .open_in_new_tab(true),
        );
        egui::warn_if_debug_build(ui);
    });
}
