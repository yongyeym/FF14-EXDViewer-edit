use std::path::PathBuf;

use crate::editable_schema::EditableSchema;
use crate::schema::provider::SchemaProvider;
use crate::settings::BackendConfig;
use crate::backend::Backend;

/// Path to the user-facing settings file.
fn settings_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("config").join("settings.json")))
        .unwrap_or_else(|| PathBuf::from("config/settings.json"))
}

/// Save backend config to the JSON settings file.
pub fn save_backend_config(config: &BackendConfig) {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(config) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, &json) {
                log::error!("保存配置文件失败 '{}': {e}", path.display());
            } else {
                log::info!("配置文件已保存: {}", path.display());
            }
        }
        Err(e) => log::error!("序列化配置失败: {e}"),
    }
}

/// Load backend config from the JSON settings file.
pub fn load_backend_config() -> Option<BackendConfig> {
    let path = settings_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<BackendConfig>(&content) {
            Ok(config) => {
                log::info!("已加载配置文件: {}", path.display());
                Some(config)
            }
            Err(e) => {
                log::warn!("解析配置文件失败 '{}': {e}", path.display());
                None
            }
        },
        Err(_) => {
            // File doesn't exist yet – first run.
            None
        }
    }
}

/// Remove settings.json from disk (e.g. during reset).
pub fn clear_settings() {
    let path = settings_path();
    if path.exists() {
        let _ = std::fs::remove_file(&path);
        log::info!("配置文件已删除: {}", path.display());
    }
}

/// Try to load a real schema for a sheet during batch export.
/// Returns `None` if schema loading fails.
pub async fn load_schema_for_export(
    backend: &Backend,
    sheet_name: &str,
) -> Option<EditableSchema> {
    let schema_text = match backend.schema().get_schema_text(sheet_name).await {
        Ok(text) => text,
        Err(_) => return None,
    };
    let editable = EditableSchema::new(sheet_name, schema_text);
    Some(editable)
}
