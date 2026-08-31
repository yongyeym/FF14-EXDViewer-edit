# 数据表内容Diff总结表 生成工具

「导出数据表内容Diff总结表」功能依赖两个独立工具 exe（用 PyInstaller 打包，
**不依赖本地机器安装 Python / openpyxl / matplotlib**）。

## 工具文件
| 工具 | 功能 | 体积 |
|------|------|------|
| `gen_excel_tool.exe` | 生成 Excel(.xlsx)，图片列嵌入缩略图 | ~27MB |
| `gen_image_tool.exe` | 生成图片(.png)，matplotlib 表格，分页 | ~37MB |

## 部署位置
把两个 `*.exe` 放到**程序 exe 同目录的 `tools/` 文件夹**下：

```
程序目录/
├── ff14-exdviewer-edit.exe
└── tools/
    ├── gen_excel_tool.exe
    └── gen_image_tool.exe
```

程序运行时会在 `tools/` 下查找对应工具；找不到会在进度窗口提示"请将 xx 放入程序目录 tools/ 文件夹下"。

## 重新打包（可选）
工具源码在 `tools/scripts/`。若要修改后重新打包：

```bash
pip install pyinstaller openpyxl matplotlib
cd tools/scripts
pyinstaller --onefile --name gen_excel_tool --distpath ../dist --workpath ../build --specpath ../ --clean gen_excel.py
pyinstaller --onefile --name gen_image_tool --distpath ../dist --workpath ../build --specpath ../ --clean gen_image.py
# 将 ../dist/*.exe 复制到 程序目录/tools/
```
