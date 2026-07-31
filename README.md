# FF14 EXDViewer edit

> 基于 [EXDViewer](https://github.com/WorkingRobot/EXDViewer) 二次开发的中文本地化桌面工具，用于浏览与对比《最终幻想14》的游戏数据表（Excel 文件）。

## 项目简介

FF14 EXDViewer edit 是一个使用 **Rust + egui** 构建的桌面应用程序，帮助 FF14 玩家、Mod 制作者与数据挖掘爱好者快速浏览游戏内部数据表（如 `Item`、`Action`、`Quest` 等），并提供了原生 EXDViewer 没有的增强能力：

- **数据版本对比（Diff）**：对比两个游戏版本导出 CSV 的数据差异，精确到行与单元格，并支持图标渲染。
- **版本追踪**：自动记录各游戏版本的数据表/音乐列表，一键筛选"仅显示新增项"。
- **音乐转码**：内置 HCA 解码工具，导出音频后自动转换为 WAV。
- **一键下载**：从 GitHub 下载 EXDSchema 与 HCADecoder 工具，无需手动配置。

## 功能使用说明

### 1. 数据表浏览

- 通过左侧面板选择数据表（支持筛选与收藏）。
- 顶部工具栏支持：筛选（等于/包含/复杂表达式）、不区分大小写、启用可见列。
- 图片列（Icon 类型）以缩略图显示，点击图片可打开大图预览；右键菜单支持「复制原始值」「复制图片」「保存此图片」。
- 单元格右键可复制文本内容。

### 2. 版本 Diff 对比

1. 先通过「导出」菜单导出 CSV 文件（默认保存到 `export/data/{版本号}/` 目录）。
2. 点击工具栏「版本Diff」按钮，打开对比窗口。
3. 选择旧版本与新版本（默认自动选中最新的两个版本），点击「开始对比」。
4. 对比在后台线程执行，完成后以表格形式展示差异行：第一列为 `+/-` 标记，随后是全部数据列（含图片渲染）。

> 提示：Diff 对比会自动忽略大小写差异与数值科学计数法格式差异（如 `1.84684e+11` 与 `184683661463` 视为相等）。

### 3. 仅显示新增项

- 数据表页面与音乐页面均提供「仅显示新增项」开关（🔍）。
- 程序会自动记录每个游戏版本的表单/音乐列表，跨版本对比后标记新增内容。

### 4. 音频导出与 HCA 转码

- 音乐页面支持导出所选音频（HCA 格式）。
- 导出后程序会自动调用 `tools/hca.exe` 在相同目录生成 `.wav` 文件。
- 若 `tools/hca.exe` 不存在，可通过「下载」菜单的「下载HCADecoder」自动获取。

### 5. 下载功能

主菜单栏的「下载」菜单提供：

| 菜单项 | 说明 | 保存位置 |
|--------|------|----------|
| **下载EXDSchema** | 下载 `xivdev/EXDSchema` 仓库 `schemas/latest` 的全部 yaml 文件 | `tools/EXDSchema/` |
| **下载HCADecoder** | 下载 `Nyagamon/HCADecoder` 最新 Release 压缩包，解压出 `hca.exe` | `tools/` |

- 下载 URL 保存在 `config/settings.json` 中（字段 `exdschema_url` / `hca_url`），可手动修改；未配置时使用代码内置默认值。
- 下载在后台线程执行，窗口显示进度与结果。

### 6. 日志与配置

- 「视图设置」菜单可打开 Log 日志窗口，支持等级过滤（ERROR/WARN/INFO/DEBUG）、正则搜索、复制日志。
- 所有配置文件均存放于 exe 同目录下的 `config/` 文件夹（JSON 格式，可读可改）。

## 目录结构

```
FF14_EXDViewer_edit/
├── Cargo.toml              # 工作区配置
├── Dockerfile              # Web 版 Docker 部署
├── deps/                   # 本地依赖（ironworks 等）
├── tools/
│   └── hca.exe             # HCA 音频解码工具（下载后获得）
├── viewer/                 # 桌面客户端主程序
│   ├── Cargo.toml          # viewer crate 配置（包名 ff14-exdviewer-edit）
│   └── src/
│       ├── main.rs         # 程序入口，日志初始化
│       ├── lib.rs          # 模块注册与全局常量
│       ├── app.rs          # 主界面逻辑（路由、菜单、导出、Diff、下载）
│       ├── backend.rs      # 后端数据提供者（游戏版本等）
│       ├── downloader.rs   # EXDSchema / HCADecoder 下载模块
│       ├── diff.rs         # 版本对比模块（CSV 解析、差异计算、表格渲染）
│       ├── list_tracker.rs # 版本化列表存储与新增项对比
│       ├── music.rs        # 音乐播放器（含新增项筛选）
│       ├── sheet/          # 数据表渲染（表格、单元格、图标）
│       ├── excel/          # 游戏数据访问（SqPack 解析）
│       ├── settings.rs     # 设置项与配置文件结构
│       ├── config_file.rs  # settings.json 读写
│       └── utils/          # 图标管理、Promise 工具等
└── README.md               # 本文件
```

## 技术栈与参考项目

### 编程语言

- **Rust**（≥ 1.97）
- 少量 HTML/JS（Web 版部署）

### 技术架构

- **UI 框架**：[egui](https://github.com/emilk/egui)（即时模式 GUI）+ [eframe](https://github.com/emilk/egui/tree/master/crates/eframe)
- **表格渲染**：`egui_table`（虚拟化表格）、`egui_extras`
- **游戏数据读取**：[ironworks](https://github.com/ackwell/ironworks)（本地 SqPack 解析）
- **CSV 处理**：`csv`
- **网络请求**：`reqwest`（下载功能）
- **压缩解压**：`zip`

### 参考项目

| 项目 | 地址 | 用途 |
|------|------|------|
| EXDViewer | https://github.com/WorkingRobot/EXDViewer | 本项目的基础（V1.7.0 版本） |
| EXDSchema | https://github.com/xivdev/EXDSchema | 数据结构定义（YAML），支持动态编辑 |
| ironworks | https://github.com/ackwell/ironworks | Rust 游戏数据解析库 |
| Lumina | https://github.com/NotAdam/Lumina | C# 游戏数据解析库（社区参考） |
| XIVAPI | https://xivapi.com/ | REST API 数据服务 |

### 第三方工具

| 工具 | 来源 | 用途 |
|------|------|------|
| HCADecoder | https://github.com/Nyagamon/HCADecoder | HCA → WAV 音频解码 |

## 从源码构建

```bash
# 原生桌面版（Release）
cargo build --release --bin ff14-exdviewer-edit
# 输出：target/release/ff14-exdviewer-edit.exe
```

构建依赖：Rust 工具链（MSVC）、NASM、Windows 10+。

---

<details>
<summary><b>以下内容为EXDViewer项目V1.7.0版本原始ReadMe.md文档（文本已经AI翻译）</b></summary>

# FF14_EXDViewer_edit

<img align="right" src="https://github.com/WorkingRobot/EXDViewer/blob/main/viewer/assets/icon.png?raw=true" width="20%">

[![Native Build](https://img.shields.io/github/actions/workflow/status/WorkingRobot/EXDViewer/build-native.yml?style=for-the-badge&label=Native%20Build
)](https://github.com/WorkingRobot/EXDViewer/releases)
[![Web Build](https://img.shields.io/github/actions/workflow/status/WorkingRobot/EXDViewer/build-web.yml?style=for-the-badge&label=Web%20Build
)](https://github.com/WorkingRobot/EXDViewer/pkgs/container/exdviewer-web)
[![License](https://img.shields.io/github/license/WorkingRobot/EXDViewer?style=for-the-badge&)](/LICENSE)
[![FFXIV Version](https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fexd.camora.dev%2Fapi%2F4e9a232b%2Fversions&query=latest&style=for-the-badge&label=Latest%20XIV%20Version
)](https://thaliak.xiv.dev/repository/4e9a232b)

EXDViewer 是一个现代化、快速且用户友好的工具，用于浏览《最终幻想14》的 [Excel 文件](https://xiv.dev/game-data/file-formats/excel)。Excel 文件是结构化的数据表，存储了各种游戏内信息，例如物品属性、NPC 数据等。

## 功能特性

- **Web 版和原生桌面版：** 立即在 [exd.camora.dev](https://exd.camora.dev) 使用 Web 版，或下载[原生桌面版](https://github.com/WorkingRobot/EXDViewer/releases)。
- **轻松部署：** 通过 Docker 自行托管 Web 实例。
- **高性能：** 高效处理所有数据表，即使是 `Item`、`Action` 或 `Quest` 这样的大型表也能流畅运行。
- **EXDSchema 支持：** 与 [EXDSchema](https://github.com/xivdev/EXDSchema) 深度集成，支持增强的数据浏览和动态编辑器内数据结构定义编辑。
- **高级筛选：** 支持简单、模糊和复杂的筛选方式，快速定位特定数据。

## 快速开始

### 在线使用

访问 [exd.camora.dev](https://exd.camora.dev) 即可在浏览器中直接使用最新版本。支持本地游戏安装、schema 文件（仅限 [Chromium 内核浏览器](https://developer.mozilla.org/en-US/docs/Web/API/Window/showDirectoryPicker#browser_compatibility)）以及全部四个区域（台湾已列出，但在 [Thaliak](https://thaliak.xiv.dev/) 上线之前暂不可用）。

### 本地运行

在 [Releases 页面](https://github.com/WorkingRobot/EXDViewer/releases) 下载对应平台的预编译二进制文件。

### 使用 Docker 自托管

使用 Docker 自行部署网站：

```bash
docker pull ghcr.io/workingrobot/exdviewer-web:main
docker run -p 8080:80 ghcr.io/workingrobot/exdviewer-web:main
```

然后在浏览器中打开 [http://localhost:8080](http://localhost:8080)。等待几秒钟加载最新游戏版本，之后在设置中将 API URL 设为 `http://localhost:8080/api`。

## 什么是 EXD 文件？

在 SqPack 中，类别 0A（0a0000.win32... 文件）由 Excel 数据表组成，这些表以专有二进制格式序列化供游戏读取。Excel 文件（其中 .exd 文件包含实际数据）是《最终幻想14》数据存储的核心部分，包含任务、物品等表格信息。FFXIV 社区经常使用这些文件进行数据挖掘和开发社区工具。通常通过 [Lumina](https://github.com/NotAdam/Lumina) (C#)、[ironworks](https://github.com/ackwell/ironworks) (Rust) 或 [XIVAPI](https://xivapi.com/) (REST API) 进行程序化访问。

更多信息请参见[此处](https://xiv.dev/game-data/file-formats/excel)。

## 什么是 EXDSchema？

FFXIV 的内部开发流程会为每个数据表生成头文件，这些文件随后被编译到游戏中，因此在游戏编译后，客户端侧会丢失所有结构信息。本仓库旨在将各方努力整合到一个与语言无关的 schema 中，任何语言都可以轻松解析该 schema，从而精确描述客户端收到的 EXH 文件的结构。

更多信息请参见[此处](https://github.com/xivdev/EXDSchema?tab=readme-ov-file#exdschema)。

## 从源码构建

1. 克隆仓库：
    ```bash
    git clone https://github.com/WorkingRobot/EXDViewer.git
    cd EXDViewer
    ```

### 原生桌面版

2. 构建项目：
    ```bash
    cargo build --bin viewer --release
    ```

### Web 版

2. 安装 trunk：
    ```bash
    cargo install --locked trunk
    ```
    或参考[安装说明](https://trunkrs.dev/guide/getting-started/installation.html)。在继续之前，请确保 `trunk` 已安装且位于 PATH 环境变量中。

3. 如果不需要 API 服务器，可以只构建 viewer 二进制文件以节省时间：
    ```bash
    trunk serve --release --config viewer
    ```

4. 如果需要 API 服务器，构建 web 二进制文件（这将同时在内部构建 viewer 二进制文件）：
    ```bash
    cargo run --bin web --release
    ```

## 贡献

欢迎提交贡献、Bug 报告和功能请求！请提交 [issue](https://github.com/WorkingRobot/EXDViewer/issues) 或 [pull request](https://github.com/WorkingRobot/EXDViewer/pulls)。

</details>
