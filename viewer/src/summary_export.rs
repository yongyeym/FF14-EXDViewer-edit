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

    // Image 模式分页：每页 MAX_ROWS_PER_PAGE 行
    const MAX_ROWS_PER_PAGE: usize = 60;
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

    let mut rows = Vec::new();
    let diff_set: std::collections::HashSet<&str> = filter_keys
        .map(|v| v.iter().map(String::as_str).collect())
        .unwrap_or_default();
    for (k, (row_id, subrow_id)) in row_keys.iter().enumerate() {
        if !(start..end).contains(&k) {
            continue;
        }
        // Diff 过滤：只保留变更数据表配置的差异行
        if !diff_set.is_empty() && !diff_set.contains(row_id.to_string().as_str()) {
            continue;
        }
        let row = context_sheet
            .get_subrow(*row_id, subrow_id.unwrap_or(0))
            .map_err(|e| anyhow::anyhow!("读取行 {row_id} 失败: {e}"))?;
        let mut cells = Vec::new();
        for (_, is_icon_col, col_idx) in &cols {
            let cell = context.cell_by_index(row, *col_idx as u32)?;
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
                cells.push(SummaryCell::Text { value: value.coerce_string().to_string() });
            } else {
                cells.push(SummaryCell::Text { value: value.coerce_string().to_string() });
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
        run_tool("gen_excel_tool.exe", &json_path, &out_path, "Excel")?;
    } else {
        run_tool("gen_image_tool.exe", &json_path, &out_path, "图片")?;
    }

    let _ = std::fs::remove_file(&json_path);
    Ok((out_path, total_pages))
}

/// 调用打包的独立工具 exe（程序目录/tools/ 下，PyInstaller 打包，不依赖本地 python）。
#[cfg(not(target_arch = "wasm32"))]
fn run_tool(tool_name: &str, json_path: &Path, out_path: &Path, kind: &str) -> anyhow::Result<()> {
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
    let status = std::process::Command::new(&tool)
        .arg(json_path)
        .arg(out_path)
        .status()?;
    if !status.success() {
        anyhow::bail!("生成{kind}失败（exit {:?}），请查看工具报错", status.code());
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn run_tool(_tool_name: &str, _json_path: &Path, _out_path: &Path, _kind: &str) -> anyhow::Result<()> {
    anyhow::bail!("web 端不支持生成 Excel/图片总结表");
}



// 引用避免未使用告警
#[allow(unused_imports)]
use crate::excel::provider::{ExcelRow, ExcelSheet as _ExcelSheet};
#[allow(unused_imports)]
use crate::sheet::cell::CellValue;
