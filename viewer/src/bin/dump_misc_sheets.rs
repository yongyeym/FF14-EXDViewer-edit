/// 临时调试工具：读取游戏 sqpack 中的 exd/root.exl，导出全部数据表ID记录（含杂项/普通表分类）。
/// 用法：dump_misc_sheets [sqpack路径] [输出txt路径] ，默认读默认游戏目录，输出到当前目录 misc_sheets.txt
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sqpack_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| r"G:\Game\FINAL FANTASY XIV\game\sqpack".to_string());
    let out_path = args
        .get(2)
        .cloned()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap().join("misc_sheets.txt"));

    match ff14_exdviewer_edit::misc_sheets::export_misc_sheets(&sqpack_path, &out_path) {
        Ok((normal, misc)) => {
            println!("已写出: {}", out_path.display());
            println!("普通表 {normal} 个, 杂项表 {misc} 个");
        }
        Err(e) => {
            eprintln!("导出失败: {e}");
            std::process::exit(1);
        }
    }
}
