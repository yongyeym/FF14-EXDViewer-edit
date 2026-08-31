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
        run_python(EXCEL_SCRIPT, &json_path, &out_path)?;
    } else {
        run_python(IMAGE_SCRIPT, &json_path, &out_path)?;
    }

    let _ = std::fs::remove_file(&json_path);
    Ok((out_path, total_pages))
}

/// 把脚本写到临时文件并调用系统 python（matplotlib/openpyxl 为第三方开源库）。
#[cfg(not(target_arch = "wasm32"))]
fn run_python(script: &str, json_path: &Path, out_path: &Path) -> anyhow::Result<()> {
    let script_path = std::env::temp_dir().join("exdviewer_summary_gen.py");
    std::fs::write(&script_path, script)?;
    let status = std::process::Command::new("python")
        .arg(&script_path)
        .arg(json_path)
        .arg(out_path)
        .status()?;
    if !status.success() {
        anyhow::bail!("Python 生成失败（exit {:?}），请确认已安装 openpyxl/matplotlib", status.code());
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn run_python(_script: &str, _json_path: &Path, _out_path: &Path) -> anyhow::Result<()> {
    anyhow::bail!("web 端不支持生成 Excel/图片总结表");
}

/// 生成 Excel 的 Python 脚本（openpyxl 第三方开源库）。
const EXCEL_SCRIPT: &str = r#"
import json, sys, os
from openpyxl import Workbook
from openpyxl.drawing.image import Image as XLImage
from openpyxl.utils import get_column_letter

def main():
    data = json.load(open(sys.argv[1], encoding='utf-8'))
    out = sys.argv[2]
    wb = Workbook(); ws = wb.active; ws.title = data['sheet_name'][:31]
    cols = data['columns']
    header = ['Row'] + [c['name'] for c in cols]
    ws.append(header)
    # 列宽
    for i, c in enumerate(cols, start=2):
        ws.column_dimensions[get_column_letter(i)].width = max(8, min(60, c['width'] / 4))
    ws.column_dimensions['A'].width = 10
    # 行数据
    for row in data['rows']:
        rec = [row['row']]
        for cell in row['cells']:
            if isinstance(cell, dict) and 'value' in cell:
                rec.append(cell['value'])
            else:
                rec.append('')
        ws.append(rec)
    # 图片列：在数据下方叠加缩略图
    # 找到图片列索引（1-based: 第1列是Row）
    icon_cols = [(i + 2) for i, c in enumerate(cols) if c.get('is_icon')]
    if icon_cols:
        for r, row in enumerate(data['rows'], start=2):
            for ci, cell in enumerate(row['cells']):
                if isinstance(cell, dict) and 'icon_path' in cell and os.path.exists(cell['icon_path']):
                    col_letter = get_column_letter(ci + 2)
                    try:
                        img = XLImage(cell['icon_path'])
                        img.width = 30; img.height = 30
                        anchor = f"{col_letter}{r}"
                        ws.add_image(img, anchor)
                        # 该单元格写 id 便于对照
                        ws[f"{col_letter}{r}"] = cell['id']
                    except Exception:
                        ws[f"{col_letter}{r}"] = cell['id']
    wb.save(out)
    print("saved", out)

main()
"#;

/// 生成图片的 Python 脚本（matplotlib 第三方开源库）。
const IMAGE_SCRIPT: &str = r#"
import json, sys, os
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from matplotlib import font_manager

def setup_font():
    for name in ['Microsoft YaHei', 'SimHei', 'Noto Sans CJK SC', 'Noto Sans SC', 'Songti SC']:
        try:
            font_manager.findfont(name, fallback_to_default=False)
            plt.rcParams['font.family'] = [name]
            break
        except Exception:
            continue
    plt.rcParams['axes.unicode_minus'] = False

def main():
    setup_font()
    data = json.load(open(sys.argv[1], encoding='utf-8'))
    out = sys.argv[2]
    cols = [c['name'] for c in data['columns']]
    rows = data['rows']
    # 单元格文本
    cells = []
    for row in rows:
        line = [str(row['row'])]
        for cell in row['cells']:
            if isinstance(cell, dict) and 'value' in cell:
                line.append(str(cell['value']))
            else:
                line.append(cell.get('id', ''))
        cells.append(line)
    header = ['Row'] + cols
    fig_w = max(10, len(header) * 2.2)
    fig_h = max(3, (len(cells) + 1) * 0.45 + 1.2)
    fig, ax = plt.subplots(figsize=(fig_w, fig_h))
    ax.axis('off')
    title = data['sheet_name'] + (f"  (第 {data['page']+1}/{data['total_pages']} 页)" if data['total_pages'] > 1 else "")
    ax.set_title(title, fontsize=12, pad=8)
    tbl = ax.table(cellText=cells, colLabels=header, loc='center', cellLoc='left')
    tbl.auto_set_font_size(False)
    tbl.set_fontsize(8)
    tbl.scale(1, 1.2)
    try:
        fig.savefig(out, dpi=150, bbox_inches='tight')
        print("saved", out)
    except Exception as e:
        print("ERR", e, file=sys.stderr)
        sys.exit(1)
    plt.close(fig)

main()
"#;

// 引用避免未使用告警
#[allow(unused_imports)]
use crate::excel::provider::{ExcelRow, ExcelSheet as _ExcelSheet};
#[allow(unused_imports)]
use crate::sheet::cell::CellValue;
