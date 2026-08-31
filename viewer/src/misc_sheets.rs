//! 从本地游戏 sqpack 的 exd/root.exl 导出数据表 ID 记录（含杂项/普通表分类）。
//! 由独立工具 bin(dump_misc_sheets) 与主程序"导出数据表ID"功能共用。

use ironworks::{
    Ironworks,
    file::exl::ExcelList,
    sqpack::{Install, SqPack},
};
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// 导出 exd/root.exl 全部数据表ID记录到 `out_path`。
/// 返回 `(普通表数, 杂项表数)`。
pub fn export_misc_sheets(sqpack_dir: &str, out_path: &Path) -> anyhow::Result<(usize, usize)> {
    let resource = Install::at_sqpack(PathBuf::from_str(sqpack_dir).map_err(anyhow::Error::msg)?);
    let iw = Ironworks::new().with_resource(SqPack::new(resource));
    let list = iw.file::<ExcelList>("exd/root.exl")?;
    let version = iw.version("exd/ffxiv").ok().map(|v| v.trim().to_string());

    let mut all: Vec<(&String, &i32)> = list.0.iter().collect();
    all.sort_by(|a, b| a.1.cmp(b.1));
    let mut misc: Vec<(&String, &i32)> = list.0.iter().filter(|(_, id)| **id < 0).collect();
    misc.sort_by(|a, b| a.1.cmp(b.1));
    let mut normal: Vec<(&String, &i32)> = list.0.iter().filter(|(_, id)| **id >= 0).collect();
    normal.sort_by(|a, b| a.1.cmp(b.1));

    let mut out = String::new();
    out.push_str("FF14 EXDViewer 游戏数据表ID记录（exd/root.exl）\n");
    out.push_str(&format!("sqpack路径: {sqpack_dir}\n"));
    if let Some(v) = &version {
        out.push_str(&format!("游戏版本: {v}\n"));
    }
    out.push_str(&format!("总表数: {}\n\n", list.0.len()));

    out.push_str("===== 全部表 =====\n");
    for (name, id) in &all {
        out.push_str(&format!("{id}\t{name}\n"));
    }

    out.push_str(&format!("\n===== 杂项表 (id < 0) =====\n共 {} 个\n", misc.len()));
    for (name, id) in &misc {
        out.push_str(&format!("{id}\t{name}\n"));
    }

    out.push_str(&format!("\n===== 普通表 (id >= 0) =====\n共 {} 个\n", normal.len()));
    for (name, id) in &normal {
        out.push_str(&format!("{id}\t{name}\n"));
    }

    std::fs::write(out_path, &out)?;
    Ok((normal.len(), misc.len()))
}