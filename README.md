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
