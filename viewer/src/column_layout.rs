//! 个性化表格列布局配置（config/column_layout.json）
//!
//! 用户可手动编辑的 JSON 文件，按数据表名配置要展示的列、顺序及中文列标题。
//! 每个元素为二元组：`[yml中的列名, 要显示的中文列标题]`，顺序即展示顺序。
//! 示例：
//! ```json
//! {
//!   "Item": [
//!     ["Icon", "图标"],
//!     ["Name", "物品名称"],
//!     ["LevelItem", "品级"]
//!   ]
//! }
//! ```
//! - 第一项必须使用该表对应 yml 数据结构定义文件中的 `- name:` 名称（用于匹配列）；
//! - 第二项为渲染表格时列标题显示的中文名；
//! - 未在配置中的列将被隐藏；
//! - 未配置的数据表按默认方式渲染。

use std::collections::HashMap;
use std::path::PathBuf;

/// 配置内容：表名 -> [(yml列名, 中文显示标题)]，数组顺序即展示顺序
/// 内部以 serde_json::Value 存储（允许 `_说明`/`_示例` 等辅助键）
pub type ColumnLayout = HashMap<String, serde_json::Value>;

/// config/column_layout.json 路径（exe 目录下）
pub fn layout_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("config").join("column_layout.json")))
        .unwrap_or_else(|| PathBuf::from("config/column_layout.json"))
}

/// 读取列布局配置；文件不存在或解析失败时返回空配置（不影响默认渲染）。
pub fn load_column_layout() -> ColumnLayout {
    let path = layout_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<ColumnLayout>(&content) {
            Ok(layout) => {
                // 每帧都会调用，只输出一次避免刷屏
                static LOGGED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
                let _ = LOGGED.get_or_init(|| {
                    log::info!("已加载列布局配置: {}", path.display());
                });
                layout
            }
            Err(e) => {
                log::warn!("解析列布局配置失败 '{}': {e}", path.display());
                HashMap::new()
            }
        },
        Err(_) => HashMap::new(),
    }
}

/// 首次运行时写入一个带说明的示例配置文件（若不存在）。
pub fn ensure_default_layout_file() {
    let path = layout_path();
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let example = "{\n  \"_说明\": \"按数据表名配置表格中要展示的列。每列是一个二元组：[yml中的列名, 中文显示标题]，数组顺序即展示顺序。未配置的表按默认方式渲染。\",\n  \"_示例\": {\n    \"Item\": [\n      [\"Icon\", \"图标\"],\n      [\"Name\", \"物品名称\"],\n      [\"LevelItem\", \"品级\"]\n    ]\n  }\n}\n";
    if let Err(e) = std::fs::write(&path, example) {
        log::warn!("写入列布局配置失败 '{}': {e}", path.display());
    } else {
        log::info!("已生成列布局配置文件: {}", path.display());
    }
}

/// 查询某数据表的列布局；未配置或格式非法时返回 None。
/// 返回 [(yml列名, 中文显示标题)]，顺序即展示顺序。
pub fn get_sheet_columns(layout: &ColumnLayout, sheet_name: &str) -> Option<Vec<(String, String)>> {
    let value = layout.get(sheet_name)?;
    let pairs = value.as_array()?;
    let cols: Vec<(String, String)> = pairs
        .iter()
        .filter_map(|item| {
            let arr = item.as_array()?;
            if arr.len() < 2 {
                return None;
            }
            let name = arr[0].as_str()?.to_string();
            if name.starts_with('_') {
                return None;
            }
            let title = arr[1].as_str().unwrap_or(&name).to_string();
            Some((name, title))
        })
        .collect();
    if cols.is_empty() {
        None
    } else {
        Some(cols)
    }
}
