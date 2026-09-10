//! 导出数据表内容Diff总结表：收集当前表数据（含个性化配置/图片列），
//! 用第三方开源工具生成 Excel(.xlsx, openpyxl) 或图片(.png, matplotlib)。

use crate::backend::Backend;
use crate::sheet::{GlobalContext, TableContext, export_csv};
use ironworks::excel::Language;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// 生成方式
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SummaryMode { Image, Excel }

/// 个性化配置
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SummaryLayout { Full, Personalized }

#[derive(Serialize, Clone)]
pub struct SummaryData {
    pub sheet_name: String,
    pub columns: Vec<SummaryColumn>,
    pub rows: Vec<SummaryRow>,
    /// Image 模式：当前页索引（0-based）
    pub page: usize,
    pub total_pages: usize,
}

#[derive(Serialize, Clone)]
pub struct SummaryColumn {
    pub name: String,
    pub width: f32,
    pub is_icon: bool,
}

#[derive(Serialize, Clone)]
pub struct SummaryRow {
    pub row: u32,
    pub subrow: Option<u16>,
    pub cells: Vec<SummaryCell>,
}

#[derive(Serialize, Clone)]
#[serde(untagged)]
pub enum SummaryCell {
    Text { value: String },
    Icon { id: String, icon_path: String },
}

/// 收集一页数据并生成文件，返回生成的文件路径。
/// `page` 为 Image 模式的分页索引（Excel 模式恒为 0）。
pub async fn generate_page(
    backend: Backend,
    sheet_name: &str,
    lang: Language,
    hires: bool,
    resolve_display_field: bool,
    mode: SummaryMode,
    layout: SummaryLayout,
    output_dir: &Path,
    page: usize,
    // Diff 过滤：仅生成这些 row_key（row_id 字符串）对应的行；None 表示全量
    filter_keys: Option<&Vec<String>>,
) -> anyhow::Result<(PathBuf, usize)> {
    use crate::excel::provider::{ExcelHeader, ExcelProvider, ExcelSheet};

    let excel = backend.excel().clone();
    let sheet = excel.get_sheet(sheet_name, lang).await?;
    let editable = crate::config_file::load_schema_for_export(&backend, sheet_name).await;
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

    let all_cols = context.columns()?;
    let is_icon = |sc: &crate::sheet::SchemaColumn| -> bool {
        format!("{:?}", sc.meta()) == "Icon"
    };

    // 根据个性化配置决定列（显示标题, 是否图片列, 列索引）
    let layout_cols: Option<Vec<(String, bool, usize)>> = if layout == SummaryLayout::Personalized {
        crate::column_layout::get_sheet_columns(&crate::column_layout::load_column_layout(), sheet_name)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|(name, title)| {
                        all_cols
                            .iter()
                            .position(|(sc, _)| sc.name() == name)
                            .map(|i| (title.clone(), is_icon(&all_cols[i].0), i))
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty())
    } else {
        None
    };

    let cols: Vec<(String, bool, usize)> = if let Some(lc) = &layout_cols {
        lc.clone()
    } else {
        (0..all_cols.len())
            .map(|i| (all_cols[i].0.name().to_string(), is_icon(&all_cols[i].0), i))
            .collect()
    };

    // 遍历行（含 subrow）
    let context_sheet = context.sheet();
    let mut row_keys: Vec<(u32, Option<u16>)> = Vec::new();
    if context_sheet.has_subrows() {
        for (rid, sid) in context_sheet.get_subrow_ids() {
            row_keys.push((rid, Some(sid)));
        }
    } else {
        for rid in context_sheet.get_row_ids() {
            row_keys.push((rid, None));
        }
    }

    // Diff 过滤：先得到过滤后的行（再据其分页，避免按全量行分页导致 diff 行只落在最后一页）
    let diff_set: std::collections::HashSet<String> = filter_keys
        .map(|v| v.iter().cloned().collect())
        .unwrap_or_default();
    if !diff_set.is_empty() {
        row_keys.retain(|(rid, _)| diff_set.contains(&rid.to_string()));
    }
    if row_keys.is_empty() {
        anyhow::bail!("数据表 {} 没有可导出的行（可能是无匹配的Diff差异行）", sheet_name);
    }

    // Image 模式分页：每页 MAX_ROWS_PER_PAGE 行（按过滤后的行数）
    const MAX_ROWS_PER_PAGE: usize = 200;
    let total_pages = if mode == SummaryMode::Image {
        row_keys.len().div_ceil(MAX_ROWS_PER_PAGE).max(1)
    } else {
        1
    };
    let page = if mode == SummaryMode::Image {
        page.min(total_pages - 1)
    } else {
        0
    };

    let start = page * MAX_ROWS_PER_PAGE;
    let end = if mode == SummaryMode::Image {
        ((page + 1) * MAX_ROWS_PER_PAGE).min(row_keys.len())
    } else {
        row_keys.len()
    };

    // 图片临时目录
    let temp_base = std::env::temp_dir().join("exdviewer_summary");
    let _ = std::fs::create_dir_all(&temp_base);

    // 预热引用表：导出用的 TableContext 是新建的，其引用表（Link / ConditionalLink）的
    // promise 尚未完成。若直接读取，链接列会返回 CellValue::InProgressLink(row_id)，
    // 最终导出成原始 row_id（而非表格中展示的解析值）。此处与 CSV 导出（csv::warm_links）
    // 保持一致：多轮触发解析，并等待所有引用表加载完成后再正式取值。
    #[cfg(not(target_arch = "wasm32"))]
    {
        use crate::utils::yield_to_ui;
        const MAX_LINK_PASSES: usize = 16;
        for _ in 0..MAX_LINK_PASSES {
            let mut pending = false;
            for (k, (row_id, subrow_id)) in row_keys.iter().enumerate() {
                if !(start..end).contains(&k) {
                    continue;
                }
                let Ok(row) = context_sheet.get_subrow(*row_id, subrow_id.unwrap_or(0)) else {
                    continue;
                };
                for (_, _, col_idx) in &cols {
                    let Ok(cell) = context.cell_by_offset(row, *col_idx as u32) else {
                        continue;
                    };
                    if let Ok(value) = cell.read(resolve_display_field) {
                        if is_pending_value(&value) {
                            pending = true;
                        }
                    }
                }
            }
            if !pending {
                break;
            }
            while context.has_pending_references() {
                yield_to_ui().await;
            }
        }
    }

    // 字体：使用程序内嵌的同款 CJK 字体（Noto Sans），写出到临时目录供生成工具渲染，
    // 从而让导出的图片文本与程序界面字体一致。
    #[cfg(not(target_arch = "wasm32"))]
    let font_path: Option<PathBuf> = {
        let (file_name, bytes) = crate::app::export_font(lang);
        let fonts_dir = temp_base.join("fonts");
        let _ = std::fs::create_dir_all(&fonts_dir);
        let p = fonts_dir.join(file_name);
        if !p.exists() {
            let _ = std::fs::write(&p, bytes);
        }
        p.exists().then_some(p)
    };
    #[cfg(target_arch = "wasm32")]
    let font_path: Option<PathBuf> = None;

    let mut rows = Vec::new();
    for (k, (row_id, subrow_id)) in row_keys.iter().enumerate() {
        if !(start..end).contains(&k) {
            continue;
        }
        let row = context_sheet
            .get_subrow(*row_id, subrow_id.unwrap_or(0))
            .map_err(|e| anyhow::anyhow!("读取行 {row_id} 失败: {e}"))?;
        let mut cells = Vec::new();
        for (_, is_icon_col, col_idx) in &cols {
            // columns() 按 offset 索引返回，因此用 cell_by_offset（而非依赖列顺序索引的 cell_by_index），
            // 否则会产生列错位（图片列被填入下一列数据）。
            let cell = context.cell_by_offset(row, *col_idx as u32)?;
            let value = cell.read(resolve_display_field)?;
            if *is_icon_col {
                if let crate::sheet::cell::CellValue::Icon(icon_id) = value {
                    let icon_id = icon_id as u32;
                    // 读图标 → 临时 PNG
                    let png_path = temp_base.join(format!("icon_{icon_id}.png"));
                    if !png_path.exists() {
                        match backend.files().get_icon(icon_id, hires).await {
                            Ok(either::Either::Right(img)) => {
                                let mut buf = std::io::Cursor::new(Vec::new());
                                if img.write_to(&mut buf, image::ImageFormat::Png).is_ok() {
                                    let _ = std::fs::write(&png_path, buf.into_inner());
                                }
                            }
                            _ => {}
                        }
                    }
                    if png_path.exists() {
                        cells.push(SummaryCell::Icon {
                            id: icon_id.to_string(),
                            icon_path: png_path.display().to_string(),
                        });
                    } else {
                        cells.push(SummaryCell::Text { value: icon_id.to_string() });
                    }
                    continue;
                }
                cells.push(SummaryCell::Text { value: display_string(&value) });
            } else {
                cells.push(SummaryCell::Text { value: display_string(&value) });
            }
        }
        rows.push(SummaryRow {
            row: *row_id,
            subrow: *subrow_id,
            cells,
        });
    }

    let data = SummaryData {
        sheet_name: sheet_name.to_string(),
        columns: cols
            .iter()
            .map(|(name, is_icon, _)| SummaryColumn {
                name: name.clone(),
                width: 140.0,
                is_icon: *is_icon,
            })
            .collect(),
        rows,
        page,
        total_pages,
    };

    // 写 JSON 临时文件 → 调 Python 生成
    let json_path = temp_base.join(format!("summary_{}_{}.json", sheet_name.replace('/', "_"), page));
    let json = serde_json::to_string(&data)?;
    std::fs::write(&json_path, &json)?;

    let out_base = output_dir.join(if mode == SummaryMode::Excel { "xlsx" } else { "png" });
    let _ = std::fs::create_dir_all(&out_base);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let out_name = if mode == SummaryMode::Excel {
        format!("{}_{}.xlsx", sheet_name.replace('/', "_"), ts)
    } else {
        format!("{}_{}_p{}.png", sheet_name.replace('/', "_"), ts, page + 1)
    };
    let out_path = out_base.join(&out_name);

    if mode == SummaryMode::Excel {
        run_tool("gen_excel_tool.exe", &json_path, &out_path, "Excel", None)?;
    } else {
        run_tool(
            "gen_image_tool.exe",
            &json_path,
            &out_path,
            "图片",
            font_path.as_deref(),
        )?;
    }

    let _ = std::fs::remove_file(&json_path);
    Ok((out_path, total_pages))
}

/// 单元格值是否仍在等待引用表解析（用于预热引用表）。
#[cfg(not(target_arch = "wasm32"))]
fn is_pending_value(value: &CellValue) -> bool {
    match value {
        CellValue::InProgressLink(_) => true,
        CellValue::ValidLink {
            value: Some(value), ..
        } => is_pending_value(value),
        _ => false,
    }
}

/// 生成与表格单元格展示一致的文本（详见 sheet::cell 的 CellValue::show）：
/// 链接列解析出显示字段时展示其文本，否则展示 `表名#行号`，与界面所见保持一致。
fn display_string(value: &CellValue) -> String {
    match value {
        CellValue::ValidLink {
            sheet_name,
            row_id,
            value,
        } => match value {
            Some(value) => display_string(value),
            None => format!("{sheet_name}#{row_id}"),
        },
        CellValue::InProgressLink(id) => format!("...#{id}"),
        CellValue::InvalidLink(id) => format!("???#{id}"),
        other => other.coerce_string().to_string(),
    }
}

/// 调用打包的独立工具 exe（程序目录/tools/ 下，PyInstaller 打包，不依赖本地 python）。
#[cfg(not(target_arch = "wasm32"))]
fn run_tool(
    tool_name: &str,
    json_path: &Path,
    out_path: &Path,
    kind: &str,
    font_path: Option<&Path>,
) -> anyhow::Result<()> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let tool = exe_dir.join("tools").join(tool_name);
    if !tool.exists() {
        anyhow::bail!(
            "找不到生成{kind}的工具: {}。请将 {tool_name} 放入程序目录 tools/ 文件夹下。",
            tool.display()
        );
    }
    let mut cmd = std::process::Command::new(&tool);
    cmd.arg(json_path).arg(out_path);
    if let Some(font_path) = font_path {
        cmd.arg(font_path);
    }
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("生成{kind}失败（exit {:?}），请查看工具报错", status.code());
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn run_tool(
    _tool_name: &str,
    _json_path: &Path,
    _out_path: &Path,
    _kind: &str,
    _font_path: Option<&Path>,
) -> anyhow::Result<()> {
    anyhow::bail!("web 端不支持生成 Excel/图片总结表");
}



// 引用避免未使用告警
#[allow(unused_imports)]
use crate::excel::provider::{ExcelRow, ExcelSheet as _ExcelSheet};
#[allow(unused_imports)]
use crate::sheet::cell::CellValue;
