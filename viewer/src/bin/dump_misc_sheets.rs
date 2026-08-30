use ironworks::{
    Ironworks,
    file::exl::ExcelList,
    sqpack::{Install, SqPack},
};
use std::path::PathBuf;
use std::str::FromStr;

/// 临时调试工具：读取游戏 sqpack 中的 exd/root.exl，列出所有杂项表（id<0）。
/// 用法：dump_misc_sheets [sqpack路径] ，默认 G:\Game\FINAL FANTASY XIV\game\sqpack
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sqpack_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| r"G:\Game\FINAL FANTASY XIV\game\sqpack".to_string());

    let resource = Install::at_sqpack(PathBuf::from_str(&sqpack_path).unwrap());
    let iw = Ironworks::new().with_resource(SqPack::new(resource));

    let list = match iw.file::<ExcelList>("exd/root.exl") {
        Ok(l) => l,
        Err(e) => {
            eprintln!("读取 root.exl 失败: {e}");
            return;
        }
    };

    let mut misc: Vec<(&String, &i32)> = list.0.iter().filter(|(_, id)| **id < 0).collect();
    let mut normal: Vec<(&String, &i32)> = list.0.iter().filter(|(_, id)| **id >= 0).collect();
    misc.sort_by(|a, b| a.1.cmp(b.1));
    normal.sort_by(|a, b| a.1.cmp(b.1));

    let ver = iw.version("exd/ffxiv").ok().map(|v| v.trim().to_string());

    let mut out = String::new();
    out.push_str("FF14 EXDViewer 杂项表(Miscellaneous, id<0)清单\n");
    out.push_str(&format!("sqpack路径: {sqpack_path}\n"));
    if let Some(v) = &ver {
        out.push_str(&format!("游戏版本: {v}\n"));
    }
    out.push_str(&format!("总表数: {}\n\n", list.0.len()));

    out.push_str("===== 杂项表 (id < 0) =====\n");
    out.push_str(&format!("共 {} 个\n\n", misc.len()));
    for (name, id) in &misc {
        out.push_str(&format!("{id}\t{name}\n"));
    }

    out.push_str("\n===== 普通表 (id >= 0) =====\n");
    out.push_str(&format!("共 {} 个\n", normal.len()));
    for (name, id) in &normal {
        out.push_str(&format!("{id}\t{name}\n"));
    }

    // 输出到 exe 目录 + 当前目录双保险（找得到即可）
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("misc_sheets.txt")));
    let cwd = std::env::current_dir().unwrap().join("misc_sheets.txt");
    let out_path = exe_dir.unwrap_or_else(|| cwd.clone());
    std::fs::write(&out_path, &out).expect("写入失败");
    println!("已写出: {}", out_path.display());
    println!("杂项表 {} 个, 普通表 {} 个", misc.len(), normal.len());
}
