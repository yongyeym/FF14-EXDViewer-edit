//! 个性化表格列布局配置（config/column_layout.json）
//!
//! 用户可手动编辑的 JSON 文件，按数据表名配置要展示的列及其顺序。
//! 示例：
//! ```json
//! {
//!   "Item": ["Singular", "Name", "Icon", "LevelItem"],
//!   "Action": ["Name", "Description", "CastTimeSeconds"]
//! }
//! ```
//! - 列名使用该表对应 yml 数据结构定义文件中的 `- name:` 名称；
//! - 未在配置中的列将被隐藏；
//! - 未配置的数据表按默认方式渲染。

use std::collections::HashMap;
use std::path::PathBuf;

/// 配置内容：表名 -> 有序列名列表（顺序即展示顺序）
pub type ColumnLayout = HashMap<String, Vec<String>>;

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
                log::info!("已加载列布局配置: {}", path.display());
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
    let example = "{\n  \"_说明\": \"按数据表名配置表格中要展示的列，列名对应yml定义中的name，顺序即展示顺序。未配置的表按默认方式渲染。\",\n  \"_示例\": {\n    \"Item\": [\"Singular\", \"Name\", \"Icon\", \"LevelItem\"],\n    \"Action\": [\"Name\", \"Description\", \"CastTimeSeconds\"]\n  }\n}\n";
    if let Err(e) = std::fs::write(&path, example) {
        log::warn!("写入列布局配置失败 '{}': {e}", path.display());
    } else {
        log::info!("已生成列布局配置文件: {}", path.display());
    }
}

/// 查询某数据表的列布局；未配置时返回 None。
pub fn get_sheet_columns(layout: &ColumnLayout, sheet_name: &str) -> Option<Vec<String>> {
    layout
        .get(sheet_name)
        .map(|cols| cols.iter().filter(|c| !c.starts_with('_')).cloned().collect())
        .filter(|cols: &Vec<String>| !cols.is_empty())
}
