# 第三方声明 Third-Party Notices

本项目（FF14 EXDViewer edit）基于 MIT 许可分发，其中包含或依赖以下第三方软件与资源。
各第三方组件的许可以其各自发行版本中声明的为准。完整依赖清单见 `Cargo.lock`。

## 原始项目

**WorkingRobot/EXDViewer**（现名 XIViewer）
- 许可证: MIT License
- 版权: Copyright (c) 2025 Asriel Camora
- 说明: 本项目基于其 V1.7.0 版本代码二次开发，并移植了其 V1.9.0 的 icons（图片）/assets（资源）页面代码。
- 仓库: https://github.com/WorkingRobot/EXDViewer

## 第三方 Rust 库（直接依赖，均为其各自许可）

以下库大多采用 MIT 或 MIT OR Apache-2.0 双许可，具体以对应 crate 自带许可为准：

| 库 | 用途 | 许可 |
|----|------|------|
| [ironworks](https://github.com/ackwell/ironworks) | 游戏数据（SqPack/Excel/SCD）解析 | MIT |
| [egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui/tree/main/crates/eframe) | GUI 框架 / 窗口 | MIT OR Apache-2.0 |
| egui_glow / egui_extras / ehttp | egui 相关扩展 / 网络 | MIT OR Apache-2.0 |
| ironworks 相关 crate（pathlist、shaders、shadermerge、glyphnames、luadec、dxbc、hlsl） | 本地/原项目工具库 | MIT（源自原项目） |
| [csv](https://github.com/BurntSushi/rust-csv) | CSV 读写 | MIT OR Apache-2.0 |
| [reqwest](https://github.com/seanmonstar/reqwest) | HTTP 请求 | MIT OR Apache-2.0 |
| [zip](https://github.com/zip-rs/zip2) / [image](https://github.com/image-rs/image) / image_dds | 压缩 / 图像 | MIT OR Apache-2.0 |
| [rfd](https://github.com/PolyMeilex/rfd) | 原生文件选择器 | MIT OR Apache-2.0 |
| [arboard](https://github.com/1Password/arboard) | 系统剪贴板（含图片） | MIT OR Apache-2.0 |
| [serde](https://github.com/serde-rs/serde) / serde_json | 序列化 | MIT OR Apache-2.0 |
| [symphonia](https://github.com/pdeljanov/Symphonia) / rodio | 音频解码 / 播放 | MIT OR Apache-2.0 |
| [syntect](https://github.com/trishume/syntect) | 语法高亮 | MIT |
| catppuccin-egui | 主题配色 | MIT |
| 其他传递依赖 | — | 见 Cargo.lock |

## 内置字体

**Noto Sans**（SC / TC / JP / KR，`viewer/assets/NotoSans*-Regular.ttf`）
- 许可证: SIL Open Font License 1.1（SIL OFL 1.1）
- 版权: © Google Inc.
- 说明: 依据 OFL 1.1 可随软件自由分发、内嵌；不得单独出售字体，且不得使用字体名称命名本软件。

**字体文件原始许可文件**：https://fonts.google.com/noto （OFL 1.1 全文见 https://openfontlicense.org）

**FFXIV_Lodestone_SSF.ttf**（`viewer/assets/FFXIV_Lodestone_SSF.ttf`）
- 来源: 原始项目 WorkingRobot/EXDViewer（XIViewer）自带并公开分发的资源。
- 版权: © SQUARE ENIX CO., LTD. —— 此为《最终幻想14》官方站点（Lodestone）字体。
- 说明: 程序运行时**未加载/引用**该字体文件（仅为原项目附带资源）；随本仓库再分发与原项目分发行为一致。请悉知该字体版权归 SQUARE ENIX，仅供个人浏览学习用途，请勿用于商业用途。

## 其他资源

- 程序图标等 `viewer/assets/*.png/svg/ico` 来自原始项目（WorkingRobot/EXDViewer，MIT）。
- `tools/` 下的 Python 工具源码与打包脚本为本项目自行编写（MIT，同本项目许可）。

## 说明

- 本程序运行时读取的 FF14 游戏数据文件、音乐、地图等资源版权归 © SQUARE ENIX CO., LTD. 所有；本项目仅作本地浏览/导出用途，不包含游戏资源，请勿用于商业分发。
- 歌曲名映射数据来自公开 API（exd.camora.dev），供个人浏览使用。
