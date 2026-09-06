# FF14 EXDViewer edit

基于 [WorkingRobot/EXDViewer（V1.7.0 版本）](https://github.com/WorkingRobot/EXDViewer) ，使用AI进行二次开发的Windows桌面工具，用于浏览、对比和导出游戏《最终幻想14（FF14）》的游戏数据资源。

## 项目简介

FF14 EXDViewer edit 是使用 **Rust + egui** 构建的Windows桌面应用程序，根据个人需求，提供了部分原版EXDViewer没有的功能，并进行全面中文本地化。
全程使用AI大模型进行功能开发和代码修改，主要使用Hermes Agent + DeepSeek V4 Flash模型。

* **主要功能**：提供 数据表、图片、地图、音乐、资源（3D模型） 页面，浏览、导出对应资源文件，数据表Diff对比等。
* **与原项目区别**：更好的中文支持，增加便于查看不同版本资源更新的相关功能，数据表页面增加个性化表格展示功能，基于EXDSchema的英文结构定义，由本人手工完成了对全部数据表结构的中文翻译和校正，按中文习惯对列排序并隐藏不需要的列，方便快速浏览表数据。

> **请注意**：由于增加的功能大多涉及本地文件读写，程序使用需要指定本地FF14游戏文件目录，并建议使用本地EXDSchema结构定义表，测试Debug环境全程使用本地相关文件，不保证使用网络资源时程序正常运行。

## 主要功能说明

### 1. 数据表列表

* 左侧列表展示全部数据表，支持筛选、仅显示新增项、显示杂项表格功能。
* 显示杂项表格功能默认不启用，选择启用后，左侧列表将显示全部数据表，表数量会从1000+变为8000左右，不建议开启，杂项表格为过场动画、任务剧情文本等配置表。
* 图片单元格以缩略图显示，点击图片可打开独立预览窗口。
* 单元格右键菜单可复制原始值（单元格原始数据文本，不含关系表引用内容）、复制（复制到系统剪贴板，支持文本和图片）、保存当前图片（仅图片单元格）。
* 表格区域右上角工具栏区域可导出当前表CSV文件，也可使用顶部菜单 「导出」下的各项导出表格功能保存指定数据表文件，保存位置为`export/data/{游戏版本号}/`目录下。
* 数据表默认命名格式为`游戏数据表原始文件名.csv`。

### 2. 图片列表

* 此功能来自原项目V1.9.0版本代码，本项目进行了部分修改优化和功能增强，支持筛选、仅显示新增项功能。
* 浏览游戏全部图片文件，包含图标、UI、地图、大型图片。
* 默认以 40×40px 网格展示（点击 +/- 可调整缩放档），点击图片可在右侧预览界面显示大图和相关信息，右侧预览图再次点击可打开独立预览窗口预览更大尺寸图片。
* 点击右上角「点击加载反向引用」按钮，将读取EXDSchema数据结构定义，在左侧按照数据表提供分类筛选，并在右侧预览界面显示所有引用该图片的数据表与对应数据行号。
* 支持复制、保存当前单张图片，也可使用 顶部菜单「导出」-「导出全部图片资源」菜单保存全部图片文件，保存位置为`export/img/`目录下。
* 图片默认命名格式`ui_{图片ID序号}.png`。

### 3. 地图列表

* 左侧列表展示全部地图列表，支持筛选、仅显示新增项功能。
* 右侧预览区显示地图名称、基本信息（编号/类型/限制职能/等级/装等……）与地图图片。
* 右侧预览区的地图基本信息数据通过读取游戏文件Map、ContentFinderCondition、ClassJobCategory等数据表获取，其中部分数据会写入到 `config/map_links_{游戏版本号}.json`持久化保存。游戏版本更新后，启动本程序进入地图列表页面时，自动重新生成新版本数据存档json，并删除旧版本。
* 支持复制、保存当前单张地图图片，也可使用 顶部菜单「导出」-「导出全部地图图片」菜单保存全部地图图片文件，保存位置为`export/map/`目录下。
* 地图图片默认命名格式`{区域短编号}-{地名}-{二级地名}.png`。

### 4. 音乐列表

* 左侧列表展示全部音乐列表，支持筛选、仅显示新增项、切换显示原始文件名/实际歌曲名功能。
* 筛选支持同时匹配原始文件名和API获取的歌曲实际名称。
* 显示歌曲名/文件名：点击左侧列表上方工具栏开关（🎼）切换显示模式，默认为歌曲名模式，可切换为原始文件名显示。仅影响左侧列表名称显示方式，不影响右侧预览窗口歌曲标题、导出音乐功能默认文件名。
* 音乐标题和获取途径等基本信息数据来自[FF14第三方公共API服务(https://exd.camora.dev/api/songs/zh/)](https://exd.camora.dev/api/songs/zh/)，访问失败时将只能显示原始文件名，无法显示歌曲名和相关信息，API中国服数据不完整，部分歌曲为英文名。
* `*.hca`加密音频在导出后，程序会自动调用HCA音频解码程序 `tools/hca.exe` 在同目录解码生成未加密的`*.wav`文件。
* 若 `tools/hca.exe` 不存在则不会进行HCA音频解码，不影响导出功能，游戏绝大部分音乐均为未加密的`*.ogg`文件。
* HCA音频解码程序可通过顶部菜单「下载」-「HCADecoder」功能自动下载，也可手动下载并移动至`tools/hca.exe`。
* 可保存当前单个音乐文件，也可使用 顶部菜单「导出」-「导出全部音乐」菜单保存全部音乐文件，保存位置为`export/music/`目录下。
* 音乐默认命名格式为`游戏音乐原始文件名`。

### 5. 资源列表

* 此功能来自原项目V1.9.0版本代码，本项目未进行任何修改，仅将原项目相关实现代码复制到本项目。
* 浏览游戏的全部资源文件（模型/材质/着色器/特效/音频/UI布局等），支持3D渲染模型。
* 搜索支持模糊、正则、包含三种模式，可按扩展名筛选。
* 点击文件后自动识别格式并提供对应查看器：3D 模型、纹理、着色器代码、动画等；无法识别时展示文件原始字节。
* 支持复制文件路径 / crc32 / 索引哈希，查看文件依赖关系。

## 次要功能说明

### 1. 仅显示新增项

* 数据表、地图、图片、音乐页面左侧列表上方均可使用「仅显示新增项」开关按钮（🔍），鼠标悬停在按钮上会提示有多少个新增项。
* 程序启动并切换到对应列表页面后，会自动记录当前游戏版本的数据列表；若最近两个版本数据无变化（如 热修补丁导致版本号变动），会自动对比更早版本数据列表存档，直到找到有变动的版本，标记新增内容。
* 若当前没有旧版本的数据列表存档json文件（即只有当前版本存档）或全部版本列表存档均无差异，则此按钮会被禁用。
* 当前版本与对比有变动版本的相关数据列表存档文件位于`config/{img_list,map_list,music_list,sheet_list}{游戏版本号}.json`，始终至多保留两个版本。
* 不再使用的旧版本数据列表会归档到 `config/bak/{img_list,map_list,music_list,sheet_list}{游戏版本号}.json`，可自行删除。

### 2. 数据表：版本 Diff 对比

> Diff对比仅支持对比CSV文件，需要先导出至少两个版本的数据表文件到本地才能使用。
> CSV文件需存放到程序导出CSV文件的默认路径`export/data/{游戏版本号}/*.csv`才能使用此功能。

1. 点击数据表右侧右上角区域工具栏「版本Diff」按钮，打开对比窗口。
2. 选择旧版本与新版本（默认自动选中最新的两个版本），并可选择是否启用「仅筛选关键列变更」、「仅显示修改后的数据」功能，最后点击「开始对比」。
3. 对比完成后以表格形式展示差异行：第一列为 `+/-` 标记新旧版本列，随后是数据列，对比结果支持切换「完整表格」/「个性化配置」展示方式。
4. 改动变化大的表格对比完成后渲染表格需要时间，程序可能会卡顿无响应，请耐心等待。

* 仅筛选关键列变更：默认不启用。只对比EXDSchema的yml文件中定义的数据表主展示列的数据是否有变更，此列名后会有黄色五角星⭐标记，yml文件中由配置项`displayField:`定义，此功能主要用于物品/装备/副本等数据表大规模修改了可用职业范围的情况（新增职业）。
* 仅显示修改后的数据：默认启用。只显示新版本CSV文件中的数据行内容，不展示旧版本数据。
* 点击「取消对比」后将还原为默认数据表格展示，Diff对比表格展示期间从左侧列表切换到其他数据表也会自动取消对比。
* Diff对比会忽略大小写、科学计数法的数据差异。

### 3. 数据表：展示完整 / 个性化配置表格

* 通过数据表页面的表格区域右上角功能按钮「完整表格」/「个性化配置」切换展示方式。
* 完整表格模式：使用xivdev/EXDSchema结构定义表配置的表格数据展示方式，提供英文列名、表关系链接、全部数据列展示，列排序按照游戏数据表原本顺序。
* 个性化配置表格模式：在使用xivdev/EXDSchema结构定义表的基础上，同时加载`config/column_layout.json`配置，提供中文列名、仅展示重要数据列，列排序按照json配置文件定义的顺序重新排序。
* 个性化配置表格模式适合日常使用，可以更方便的查看重要的游戏数据，隐藏了未知作用、不太重要、中国服环境不需要的列，并按照中文习惯翻译并校正了EXDSchema原本提供的列名。

## 下载功能

顶部菜单栏的「下载」菜单提供：

|下载项目|配置项|功能说明|文件保存位置|
|-|-|-|-|
|**[EXDSchema](https://github.com/xivdev/EXDSchema)**|exdschema_url|逐个下载 `xivdev/EXDSchema` 的子模块仓库 `schemas/latest` 的全部 yml 文件|`tools/EXDSchema/*.yml`|
|**[HCADecoder](https://github.com/Nyagamon/HCADecoder)**|hca_url|下载 `Nyagamon/HCADecoder` 最新 Release 压缩包，解压出 `hca.exe`|`tools/hca.exe`|

* URL配置项在 `config/settings.json`中，可手动修改；未配置或配置有误时会自动重置为默认值。
* 下载在后台异步执行，窗口显示进度与结果，下载完成5秒后进度窗口自动关闭。
* 由于EXDSchema文件众多（1000+），程序提供的下载功能需要按文件依次下载，且无断点续传等功能，频繁请求github容易中途下载失败，推荐使用git克隆仓库，若无git环境可考虑zip包方式手动下载，github release提供的可能不是最新数据，建议直接下载仓库schemas/latest子模块。
  ```
  # xivdev/EXDSchema仓库使用了子模块提供多个版本的EXDSchema结构定义表文件，克隆需要使用额外参数。
  git clone --recurse-submodules https://github.com/xivdev/EXDSchema.git

  # git pull无法直接更新子模块仓库，可使用递归更新或子模块更新命令。
  git pull --recurse-submodules
  git submodule update

  # 若后续想使用git pull时不带额外参数即可默认递归拉取子模块仓库更新，可设置Git总是以 --recurse-submodules 拉取（clone命令仍需指定额外参数）。
  # 若有大量其他含有子模块的仓库项目，请谨慎设置此项！
  git config --global submodule.recurse true
  ```

## 数据功能

顶部菜单栏的「数据」菜单提供：

|数据项目|功能说明|文件保存位置|
|-|-|-|
|**导出数据表（exd/root.exl）**|导出游戏文件exd/root.exl中记录的数据表ID数据为txt文档|`export/result/misc_sheets_{游戏版本号}.txt`|
|**找出版本变更的数据表**|选择两个版本文件夹，对比其中的CSV文件，找出有差异的文件并将结果存为txt文档|`export/result/diff_sheets_{旧版本号}_{新版本号}.txt`|
|**导出当前数据表内容Diff总结表**|以图片/excel表格形式输出当前选择的数据表内容|`export/result/png/{数据表名}_{当前日期时间}_p{页码}.png`<br>`export/result/xlsx/{数据表名}_{当前日期时间}.xlsx`|

* 功能执行完成后会自动打开生成的文件，调用Windows资源管理器使用系统默认打开方式打开文件。
* 导出数据表（exd/root.exl）：读取此游戏文件并格式化导出，此文件记录了全部数据表ID序号，其中ID为-1的为杂项表格，ID>0的为普通表格，杂项表格有6000+个，普通表格1000+个。此功能仅输出表格ID序号表，不会真正导出游戏数据表文件，如需导出数据表，请使用「导出」菜单相关功能。
* 找出版本变更的数据表：需要至少导出过两个版本的不同CSV数据表文件，输出txt文档结果会按有变更文件、新版本新增文件、新版本删除文件三项分别列出对应的CSV文件名。
* 导出当前数据表内容Diff总结表：以图片/excel表格形式输出当前选择的数据表内容，可选择使用完整表格/个性化表格输出，如果启用了Diff表格对比，则输出结果为Diff结果的表格。输出图片时固定200行数据分割，分多张图片输出。

  > **导出当前数据表内容Diff总结表**：请注意！此功能需要额外程序支持。
  >
  > 额外程序由AI编程，使用python编写并打包为exe单文件程序，相关源码也保存在此项目中，存放在`tools/`目录下。
  >> `tools/gen_image_tool.exe`：输出图片时调用。
  >> `tools/gen_excel_tool.exe`：输出Excel表格时调用。

## 导出功能

顶部菜单栏的「导出」菜单提供：

|导出项目|功能说明|文件保存位置|
|-|-|-|
|**导出全部CSV（不含杂项表）**|导出当前游戏全部数据表CSV文件，不包括杂项表格|`export/data/{游戏版本号}/*.csv`|
|**导出全部CSV源文件（不含杂项表）**|导出当前游戏全部数据表CSV源文件，不包括杂项表格|`export/data/{游戏版本号}/*.csv`|
|**导出全部CSV**|导出当前游戏全部数据表CSV文件，包括杂项表格|`export/data/{游戏版本号}/*.csv`|
|**导出全部CSV源文件**|导出当前游戏全部数据表CSV文件，包括杂项表格|`export/data/{游戏版本号}/*.csv`|
|**导出收藏CSV**|导出当前游戏收藏的数据表CSV文件|`export/data/{游戏版本号}/*.csv`|
|**导出收藏CSV源文件**|导出当前游戏收藏的数据表CSV源文件|`export/data/{游戏版本号}/*.csv`|
|**删除指定版本CSV文件**|打开删除窗口，选择指定版本号的文件夹将其删除。若文件夹中有非CSV文件，则保留这些文件；若文件夹中没有非CSV文件，则会同时删除此文件夹|`export/data/*/`|
|**导出全部音乐**|导出当前游戏全部音乐文件，并转码导出的*.hca加密音乐文件|`export/music/*.{ogg,wav,hca…}`|
|**导出全部地图图片**|导出当前游戏全部地图图片文件|`export/img/{区域短编号}-{地名}-{二级地名}.png`|
|**导出全部图片资源**|导出当前游戏全部图片文件|`export/img/ui_{图片ID序号}.png`|

* 导出在后台异步执行，窗口显示进度与结果，导出完成5秒后进度窗口自动关闭。
* CSV源文件：即表格原始的数据内容，导出时不会将引用其他数据表的内容填入其中。

## 设置与其他

* 顶部菜单栏的「应用」菜单：可打开游戏/文件路径设置。
* 顶部菜单栏的「跳转」菜单：可在数据列表页面使用，跳转到指定行/表，功能来自原项目，未作修改。
* 顶部菜单栏的「数据语言」菜单：可设置数据表展示语言（默认简体中文）。
* 顶部菜单栏的「视图设置」菜单：可设置程序相关展示设置及打开日志窗口。
* 所有配置文件均存放于`config/`目录下，所有导出的游戏资源文件均存放于`export/`目录下。

## config目录文件用途

* `bak/*`：保存旧的文件存档，可删除。
* `app.ron`：程序核心配置文件，保存当前数据表浏览的位置、程序框体尺寸等信息。
* `settings.json`：程序核心配置文件，保存游戏文件/EXDSchema本地或网络位置，下载功能URL地址。
* `favorites.json`：记录收藏的数据表列表。
* `column_layout.json`：数据表页面「个性化配置」表格展示功能的配置文件，记录了全部数据表与EXDSchema对应列名中文翻译、显示的列、列排序的配置。
* `map_links_{游戏版本号}.json`：地图列表页面基础信息数据存档，游戏版本更新后会自动生成新版本文件，并删除旧版本。
* `sheet_list_{游戏版本号}.json`：数据表页面表列表存档。
* `music_list_{游戏版本号}.json`：引用页面表列表存档。
* `map_list_{游戏版本号}.json`：地图页面表列表存档。
* `img_list_{游戏版本号}.json`：图片页面表列表存档。

> `*_list_{游戏版本号}.json`列表存档文件：用于「仅显示新增项」功能，固定保留有差异的最近两个游戏版本的数据列表，其他不再使用的列表移动到`bak/`目录下，可自行删除。

## 项目目录结构

```
FF14\_EXDViewer\_edit/
├── Cargo.toml              # 工作区配置
├── Dockerfile              # Web 版 Docker 部署
├── deps/                   # 本地依赖（ironworks 等）
├── deps/                   # 本地依赖（ironworks、d3dasm 等）
├── glyphnames/             # 字形名列表 crate（资源查看用）
├── luadec/                 # Lua 字节码反编译 crate（资源查看用）
├── pathlist/               # FFXIV 路径列表编码 crate
├── shaders/                # 着色器名列表 crate
├── shadermerge/            # 着色器合并 crate（shpk 查看用）
├── viewer/                 # 桌面客户端主程序
│   ├── Cargo.toml          # viewer crate 配置（包名 ff14-exdviewer-edit）
│   └── src/
│       ├── main.rs         # 程序入口，日志初始化
│       ├── lib.rs          # 模块注册与全局常量
│       ├── app.rs          # 主界面逻辑（路由、菜单、导出、Diff、下载）
│       ├── backend.rs      # 后端数据提供者（游戏版本等）
│       ├── downloader.rs   # EXDSchema / HCADecoder 下载模块
│       ├── diff.rs         # 版本对比模块（CSV 解析、差异计算、表格渲染）
│       ├── list\_tracker.rs # 版本化列表存储与新增项对比/归档
│       ├── map.rs          # 地图列表（左侧列表 + 右侧预览 + 图片保存）
│       ├── music.rs        # 音乐播放器（含新增项筛选）
│       ├── icons/          # 图标列表（分类、网格、反向引用）
│       ├── assets/         # 资源列表（搜索、文件树、3D 查看器）
│       ├── sheet/          # 数据表渲染（表格、单元格、图标）
│       ├── excel/          # 游戏数据访问（SqPack 解析）
│       ├── goto.rs         # 跳转窗口（含 Palette 调色板/列表导航）
│       ├── settings.rs     # 设置项与配置文件结构
│       ├── config\_file.rs  # settings.json 读写
│       └── utils/          # 图标管理、Promise 工具、纹理解码等
└── README.md               # 本文档
```

## 技术栈与参考项目

### 编程语言

* Rust（≥ 1.97）
* Python（独立额外工具）
* HTML/JS（Web版支持，原项目自带，未修改）

### 技术架构

* **UI 框架**：[egui](https://github.com/emilk/egui)+ [eframe](https://github.com/emilk/egui/tree/master/crates/eframe)
* **表格渲染**：`egui\_table`、`egui\_extras`
* **本地游戏文件解析**：[ironworks](https://github.com/ackwell/ironworks)
* **CSV 处理**：`csv`
* **网络请求**：`reqwest`
* **压缩解压**：`zip`

### 参考项目

|项目|地址|用途|
|-|-|-|
|EXDViewer|https://github.com/WorkingRobot/EXDViewer|基础项目|
|EXDSchema|https://github.com/xivdev/EXDSchema|数据结构定义（YAML）|
|ironworks|https://github.com/ackwell/ironworks|Rust 游戏数据解析库|
|Lumina|https://github.com/NotAdam/Lumina|C# 游戏数据解析库|
|XIVAPI|https://xivapi.com/|FF14第三方 API 服务|

### 第三方工具

|项目|地址|用途|
|-|-|-|
|HCADecoder|https://github.com/Nyagamon/HCADecoder|HCA 加密音频解码|

## 从源码构建

```bash
# 原生桌面版（Release）
cargo build --release --bin ff14-exdviewer-edit
# 输出：target/release/ff14-exdviewer-edit.exe
```

构建依赖：Rust 工具链（MSVC）、NASM、Windows 10+。

\---

<details>
<summary><b>以下为原项目WorkingRobot/EXDViewer的V1.7.0版本ReadMe.md文档内容（AI翻译版本，无人工校对）</b></summary>

# EXDViewer

<img align="right" src="https://github.com/WorkingRobot/EXDViewer/blob/main/viewer/assets/icon.png?raw=true" width="20%">

[!\[Native Build](https://img.shields.io/github/actions/workflow/status/WorkingRobot/EXDViewer/build-native.yml?style=for-the-badge\&label=Native%20Build)](https://github.com/WorkingRobot/EXDViewer/releases)
[!\[Web Build](https://img.shields.io/github/actions/workflow/status/WorkingRobot/EXDViewer/build-web.yml?style=for-the-badge\&label=Web%20Build)](https://github.com/WorkingRobot/EXDViewer/pkgs/container/exdviewer-web)
[!\[License](https://img.shields.io/github/license/WorkingRobot/EXDViewer?style=for-the-badge\&)](/LICENSE)
[!\[FFXIV Version](https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fexd.camora.dev%2Fapi%2F4e9a232b%2Fversions\&query=latest\&style=for-the-badge\&label=Latest%20XIV%20Version)](https://thaliak.xiv.dev/repository/4e9a232b)

EXDViewer 是一个现代化、快速且用户友好的工具，用于浏览《最终幻想14》的 [Excel 文件](https://xiv.dev/game-data/file-formats/excel)。Excel 文件是结构化的数据表，存储了各种游戏内信息，例如物品属性、NPC 数据等。

## 功能特性

* **Web 版和原生桌面版：** 立即在 [exd.camora.dev](https://exd.camora.dev) 使用 Web 版，或下载[原生桌面版](https://github.com/WorkingRobot/EXDViewer/releases)。
* **轻松部署：** 通过 Docker 自行托管 Web 实例。
* **高性能：** 高效处理所有数据表，即使是 `Item`、`Action` 或 `Quest` 这样的大型表也能流畅运行。
* **EXDSchema 支持：** 与 [EXDSchema](https://github.com/xivdev/EXDSchema) 深度集成，支持增强的数据浏览和动态编辑器内数据结构定义编辑。
* **高级筛选：** 支持简单、模糊和复杂的筛选方式，快速定位特定数据。

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

&#x20;   ```bash
    git clone https://github.com/WorkingRobot/EXDViewer.git
    cd EXDViewer
    ```

### 原生桌面版

2. 构建项目：

&#x20;   ```bash
    cargo build --bin viewer --release
    ```

### Web 版

2. 安装 trunk：

&#x20;   ```bash
    cargo install --locked trunk
    ```

   或参考[安装说明](https://trunkrs.dev/guide/getting-started/installation.html)。在继续之前，请确保 `trunk` 已安装且位于 PATH 环境变量中。

3. 如果不需要 API 服务器，可以只构建 viewer 二进制文件以节省时间：

&#x20;   ```bash
    trunk serve --release --config viewer
    ```

4. 如果需要 API 服务器，构建 web 二进制文件（这将同时在内部构建 viewer 二进制文件）：

&#x20;   ```bash
    cargo run --bin web --release
    ```

## 贡献

欢迎提交贡献、Bug 报告和功能请求！请提交 [issue](https://github.com/WorkingRobot/EXDViewer/issues) 或 [pull request](https://github.com/WorkingRobot/EXDViewer/pulls)。

</details>

