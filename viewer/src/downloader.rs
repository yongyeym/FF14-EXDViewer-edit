use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Default download URLs (used when config has no URL for that tool)
pub const DEFAULT_EXDSCHEMA_URL: &str =
    "https://github.com/xivdev/EXDSchema";
pub const DEFAULT_HCA_DECODER_URL: &str =
    "https://github.com/Nyagamon/HCADecoder";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadStatus {
    Idle,
    Downloading(String),
    Done,
    Error(String),
}

pub type SharedStatus = Arc<Mutex<DownloadStatus>>;

/// Returns the tools directory (exe_dir/tools)
pub fn tools_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("tools")))
        .unwrap_or_else(|| PathBuf::from("tools"))
}

/// Start downloading EXDSchema yaml files in a background thread.
/// Progress is reported via `status`.
pub fn start_download_exdschema(
    url: &str,
    status: SharedStatus,
) {
    let url = url.to_string();
    let dest = tools_dir().join("EXDSchema");
    std::thread::spawn(move || {
        if let Err(e) = download_exdschema_impl(&url, &dest, &status) {
            let mut s = status.lock().unwrap();
            *s = DownloadStatus::Error(format!("EXDSchema下载失败: {e}"));
        }
    });
}

fn download_exdschema_impl(
    _base_url: &str,
    dest_dir: &PathBuf,
    status: &SharedStatus,
) -> Result<(), String> {
    // Fetch the directory listing from GitHub Contents API
    // The schemas/latest path is a submodule; we resolve it via the tree SHA
    let client = reqwest::blocking::Client::builder()
        .user_agent("FF14-EXDViewer-edit/1.0")
        .build()
        .map_err(|e| format!("创建HTTP客户端失败: {e}"))?;

    // Step 1: Get the submodule info to find the commit SHA
    let api_url = format!(
        "https://api.github.com/repos/xivdev/EXDSchema/contents/schemas/latest"
    );
    let resp = client
        .get(&api_url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .map_err(|e| format!("请求子模块信息失败: {e}"))?;
    let resp_status = resp.status();
    let body_text = resp.text().map_err(|e| format!("读取响应体失败: {e}"))?;
    if !resp_status.is_success() {
        return Err(format!(
            "GitHub API返回状态码: {} (访问 {} 可能需要token)\n响应: {body_text}", resp_status, api_url
        ));
    }
    let submodule: serde_json::Value = serde_json::from_str(&body_text)
        .map_err(|e| format!("解析子模块JSON失败: {e}\n响应: {}", &body_text[..body_text.len().min(500)]))?;
    let is_submodule = submodule["type"].as_str() == Some("submodule");

    let sha = if is_submodule { submodule["sha"].as_str().unwrap_or("").to_string() } else { String::new() };
    let (entries, _source_desc): (Vec<serde_json::Value>, String) = if is_submodule {
        // Submodule: use Git Trees API with recursive=1 to get all files at that commit
        let sha = submodule["sha"].as_str().unwrap_or("");
        let tree_url = format!(
            "https://api.github.com/repos/xivdev/EXDSchema/git/trees/{sha}?recursive=1"
        );
        let tresp = client
            .get(&tree_url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .map_err(|e| format!("获取文件树失败: {e}"))?;
        let tstatus = tresp.status();
        let tbody = tresp.text().map_err(|e| format!("读取文件树响应失败: {e}"))?;
        if !tstatus.is_success() {
            return Err(format!(
                "GitHub Trees API返回状态码: {}\n响应: {}",
                tstatus, &tbody[..tbody.len().min(500)]
            ));
        }
        let tree: serde_json::Value = serde_json::from_str(&tbody)
            .map_err(|e| format!("解析文件树JSON失败: {e}\n响应: {}", &tbody[..tbody.len().min(500)]))?;
        let items = tree["tree"].as_array().cloned().unwrap_or_default();
        // Filter to only .yml files (blob type)
        let yml_files: Vec<serde_json::Value> = items.into_iter()
            .filter(|item| {
                item["type"].as_str() == Some("blob")
                && item["path"].as_str().map_or(false, |p| p.ends_with(".yml"))
            })
            .collect();
        (yml_files, format!("(文件树, SHA={sha})"))
    } else {
        // Regular directory
        let items: Vec<serde_json::Value> = serde_json::from_str(&body_text)
            .map_err(|e| format!("解析目录JSON失败: {e}"))?;
        (items, String::new())
    };

    if entries.is_empty() {
        return Err("目录中没有找到任何.yml文件".to_string());
    }

    // Ensure destination directory exists
    std::fs::create_dir_all(dest_dir)
        .map_err(|e| format!("创建目录失败: {e}"))?;

    let total = entries.len();
    for (i, entry) in entries.iter().enumerate() {
        let name = entry["name"].as_str()
            .or_else(|| entry["path"].as_str())
            .unwrap_or("unknown");
        let typ = entry["type"].as_str().unwrap_or("");

        {
            let mut s = status.lock().unwrap();
            *s = DownloadStatus::Downloading(format!("EXDSchema [{}/{}] {}", i + 1, total, name));
        }

        // Tree API entries have 'blob' type; Contents API has 'file'
        if (typ == "file" || typ == "blob") && name.ends_with(".yml") {
            // Construct raw download URL from GitHub
            let download_url = entry["download_url"].as_str().map(|s| s.to_string())
                .or_else(|| {
                    entry["path"].as_str().map(|path| {
                        format!("https://raw.githubusercontent.com/xivdev/EXDSchema/{}/{path}", sha)
                    })
                })
                .ok_or_else(|| format!("{name} 缺少下载URL"))?;

            let file_resp = client
                .get(download_url)
                .send()
                .map_err(|e| format!("下载 {name} 失败: {e}"))?;

            let bytes = file_resp
                .bytes()
                .map_err(|e| format!("读取 {name} 失败: {e}"))?;

            // Use the entry's path (relative path like '.github/columns.yml') or name as fallback
            let save_name = entry["path"].as_str().unwrap_or(name);
            let file_path = dest_dir.join(save_name);
            if let Some(parent) = file_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&file_path, &bytes)
                .map_err(|e| format!("保存 {name} 失败: {e}"))?;
        }
    }

    let mut s = status.lock().unwrap();
    *s = DownloadStatus::Done;
    Ok(())
}

/// Start downloading HCADecoder in a background thread.
pub fn start_download_hca(
    url: &str,
    status: SharedStatus,
) {
    let url = url.to_string();
    let dest = tools_dir();
    std::thread::spawn(move || {
        if let Err(e) = download_hca_impl(&url, &dest, &status) {
            let mut s = status.lock().unwrap();
            *s = DownloadStatus::Error(format!("HCADecoder下载失败: {e}"));
        }
    });
}

fn download_hca_impl(
    repo_url: &str,
    dest_dir: &PathBuf,
    status: &SharedStatus,
) -> Result<(), String> {
    // Extract owner/repo from the URL
    let repo_path = repo_url
        .trim_end_matches('/')
        .trim_start_matches("https://github.com/");
    let api_releases = format!(
        "https://api.github.com/repos/{repo_path}/releases/latest"
    );

    let client = reqwest::blocking::Client::builder()
        .user_agent("FF14-EXDViewer-edit/1.0")
        .build()
        .map_err(|e| format!("创建HTTP客户端失败: {e}"))?;

    {
        let mut s = status.lock().unwrap();
        *s = DownloadStatus::Downloading("获取最新Release信息...".into());
    }

    let resp = client
        .get(&api_releases)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .map_err(|e| format!("获取Release信息失败: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "GitHub API返回状态码: {} (可能需要 GitHub Token)",
            resp.status()
        ));
    }

    let release: serde_json::Value = resp
        .json()
        .map_err(|e| format!("解析Release JSON失败: {e}"))?;

    // Find the first zip/tar.gz asset
    let assets = release["assets"]
        .as_array()
        .ok_or("Release中没有assets字段")?;

    let asset = assets
        .iter()
        .find(|a| {
            let name = a["name"].as_str().unwrap_or("");
            name.ends_with(".zip") || name.ends_with(".tar.gz") || name.ends_with(".tgz")
        })
        .ok_or("未找到压缩包格式的Release资源 (需要 .zip 或 .tar.gz)")?;

    let asset_name = asset["name"].as_str().unwrap_or("archive.zip");
    let download_url = asset["browser_download_url"]
        .as_str()
        .ok_or("Release资源缺少下载URL")?;

    {
        let mut s = status.lock().unwrap();
        *s = DownloadStatus::Downloading(format!("下载 {} ...", asset_name));
    }

    let archive_resp = client
        .get(download_url)
        .send()
        .map_err(|e| format!("下载压缩包失败: {e}"))?;

    let archive_bytes = archive_resp
        .bytes()
        .map_err(|e| format!("读取压缩包失败: {e}"))?;

    // Extract hca.exe from the archive
    {
        let mut s = status.lock().unwrap();
        *s = DownloadStatus::Downloading("解压中...".into());
    }

    if asset_name.ends_with(".zip") {
        extract_hca_from_zip(&archive_bytes, dest_dir)?;
    } else {
        return Err("仅支持 .zip 格式的自动解压".to_string());
    }

    let mut s = status.lock().unwrap();
    *s = DownloadStatus::Done;
    Ok(())
}

fn extract_hca_from_zip(data: &[u8], dest: &PathBuf) -> Result<(), String> {
    let reader =
        std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| format!("打开ZIP文件失败: {e}"))?;

    let mut found = false;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("读取ZIP条目失败: {e}"))?;

        let out_path = file.name().to_string();
        // Only extract hca.exe
        if out_path.ends_with("hca.exe") || out_path.eq_ignore_ascii_case("hca.exe") {
            let target_path = dest.join("hca.exe");
            let mut outfile = std::fs::File::create(&target_path)
                .map_err(|e| format!("创建 hca.exe 失败: {e}"))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| format!("写入 hca.exe 失败: {e}"))?;
            found = true;
            break;
        }
    }

    if !found {
        return Err("ZIP中未找到 hca.exe".to_string());
    }
    Ok(())
}
