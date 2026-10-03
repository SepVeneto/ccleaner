# C 盘清理助手 · DiskSweeper

用 **Tauri 2 + React + TypeScript + Rust** 写的 Windows 磁盘清理工具，核心是两个能力：

1. **可视化分析**：扫描磁盘后用矩形树图（squarified treemap）展示每个目录/文件占用的空间，
   面积越大占用越多，交互方式类似 SpaceSniffer —— 双击色块逐层进入，滚轮/面包屑回退，
   悬停查看大小与占比。
2. **安全清理**：列出可以被清理、且**不影响系统正常使用**的内容，每一项都标注了
   「它是什么、有什么用」以及「为什么删掉没关系」，并按风险等级分类，勾选后一键清理。

> 当前开发环境是 Linux，因此系统相关的清理目标会退回到跨平台的 `~/.cache`、npm/pip 缓存等；
> **在 Windows 上运行时会自动切换到 C 盘的目标**（临时文件、Windows 更新缓存、系统日志、
> 浏览器缓存等）。

---

## 功能一览

### 1. 可视化分析（矩形树图）

- 并行扫描（顶层目录用 Rayon 并行），扫描过程实时汇报进度、可随时取消。
- 矩形树图按**面积 = 占用大小**布局：目录是容器，文件是叶子，标签随色块大小自适应。
- 颜色编码：目录按顶层色相区分、层级越深越暗；文件按类型着色（视频/图片/压缩包/程序…）。
- 双击目录下钻，面包屑与「上一级」回退；被深度限制截断的目录用**虚线边框**标出。
- 悬停浮层显示名称、大小、占当前视图比例、文件数、完整路径。
- 选中色块后可「进入目录 / 在资源管理器中定位 / 删除到回收站 / 永久删除」。
- 侧栏还有**大文件排行**（Top 200）和**概览**（磁盘使用率、文件数、耗时、跳过数）。

### 2. 清理建议（带功能说明）

每一项清理目标都包含四类信息：

| 字段 | 含义 |
| --- | --- |
| `description` | 这个目录**是什么、由谁写入、有什么用** |
| `safety` | **为什么清理它不会影响正常使用** |
| `risk` | 风险等级：`安全` / `基本安全` / `需确认` / `高风险` |
| `note` | 额外注意事项（如需管理员权限、建议先关闭程序、不可撤销等） |

清理面板支持：按分类整体勾选、测量每项实际占用大小、一键清理选中项、
清理后展示释放空间与未能删除的文件（通常是正在被占用的文件，跳过即可）。

---

## Windows 上的清理目标

| 分类 | 目标 | 风险 | 功能说明 |
| --- | --- | --- | --- |
| 临时文件 | 用户临时文件 `%TEMP%` | 安全 | 程序运行时释放的中间数据，用完即弃 |
| 临时文件 | 系统临时文件 `C:\Windows\Temp` | 安全 | 系统服务与安装程序（MSI/EXE）的临时目录 |
| Windows 更新 | 更新下载缓存 `SoftwareDistribution\Download` | 基本安全 | 已安装更新的安装包，系统需要时会重新下载 |
| Windows 更新 | 传递优化缓存 | 基本安全 | 更新用的 P2P 分发缓存 |
| 系统缓存 | 预读取 `C:\Windows\Prefetch` | 基本安全 | 加快启动的加载记录，会自动重建 |
| 系统缓存 | 缩略图缓存 `thumbcache_*.db` | 安全 | 纯缓存，可修复缩略图显示异常 |
| 系统缓存 | 图标缓存 `iconcache_*.db` | 安全 | 纯缓存，可修复图标错乱 |
| 系统缓存 | 字体缓存 FontCache | 基本安全 | 系统字体渲染缓存，自动重建 |
| 日志与诊断 | Windows 错误报告 WER | 安全 | 崩溃反馈数据，删除不影响程序 |
| 日志与诊断 | 程序崩溃转储 `CrashDumps` | 安全 | 崩溃时的内存转储，单个可达上百 MB |
| 日志与诊断 | 系统内存转储 `memory.dmp` / `Minidump` | 安全 | 蓝屏转储，仅在分析蓝屏时需要 |
| 日志与诊断 | 系统日志 `C:\Windows\Logs` | 安全 | 安装/更新/组件的历史文本日志 |
| 日志与诊断 | Defender 扫描历史 | 需确认 | 会一并清掉隔离区记录 |
| 回收站 | `$Recycle.Bin` | 安全 | 清空即永久删除，**不可撤销** |
| 系统升级残留 | `C:\Windows.old` | 需确认 | 旧系统备份，删除后无法回退版本 |
| 系统升级残留 | `$WINDOWS.~BT` / `$WINDOWS.~WS` / `$GetCurrent` | 需确认 | 升级过程遗留的临时目录 |
| 显卡缓存 | DirectX `D3DSCache` | 安全 | 着色器缓存，首次运行会重新编译 |
| 显卡缓存 | NVIDIA `DXCache`/`GLCache`/`ComputeCache` | 安全 | 驱动自动重建 |
| 显卡缓存 | AMD `DxCache`/`GLCache` | 安全 | 驱动自动重建 |
| 浏览器缓存 | Chrome / Edge / Firefox 缓存 | 安全 | **只清缓存**，书签、密码、登录状态不受影响 |
| 开发工具缓存 | npm / Yarn / pip / NuGet / VS Code 缓存 | 安全 | 可重新下载，不影响已安装的项目 |
| 开发工具缓存 | pnpm 内容寻址存储 | 需确认 | 硬链接共享，清理后其它项目需重装依赖 |
| 应用缓存 | Teams / Discord 缓存 | 安全 | 只清图片与网页资源缓存 |
| 系统功能 | 休眠文件 `hiberfil.sys` | 高风险 | 执行 `powercfg -h off`，释放≈内存容量，代价是无法休眠/快速启动 |

> 「需管理员」标记的目标需要以管理员身份运行本程序才能完整清理；普通权限下会跳过无权限的
> 文件并在结果中列出，不会中断其它清理项。

---

## 技术实现

```
ccleaner/
├─ src/                     前端（React + TypeScript）
│  ├─ App.tsx               主界面：工具栏、面包屑、树图、侧栏
│  ├─ components/
│  │  ├─ Treemap.tsx        SVG 矩形树图渲染 + 悬停/选中/下钻
│  │  ├─ CleanupPanel.tsx   清理目标列表（分类、说明、风险、测量、清理）
│  │  └─ BigFiles.tsx       大文件排行
│  └─ lib/
│     ├─ treemap.ts         squarified treemap 布局算法
│     ├─ palette.ts         目录色相 / 文件类型配色
│     └─ api.ts             Tauri 命令与事件封装
├─ scripts/treemap-check.ts 树图布局自检脚本
└─ src-tauri/               后端（Rust）
   └─ src/
      ├─ scan.rs            并行扫描、目录树构建、进度事件、取消、最大文件堆
      ├─ cleanup.rs         清理目标目录库 + 清理执行
      ├─ fsutil.rs          环境变量/通配符展开、长路径、体积统计、删除
      ├─ disk.rs            卷信息、默认扫描根目录、在文件管理器中定位
      └─ lib.rs             Tauri 命令注册 + 无界面 CLI
```

要点：

- 扫描时跳过符号链接与目录联结点（Windows junction），避免死循环；对 Windows 超长路径
  自动加 `\\?\` 前缀（界面里仍显示正常路径）。
- 删除只读文件失败时会清除只读属性重试一次；被占用的文件记录为错误并继续，不中断整体清理。
- 「删除到回收站」在 Windows 上走系统回收站 API，在其它平台退回为直接删除。

---

## 开发与运行

需要 Rust（≥1.77）、Node.js（≥18）与对应平台的 Tauri 依赖。

```bash
npm install          # 安装前端依赖
npm run tauri:dev    # 启动开发模式（Vite + Tauri 窗口）
npm run build        # 仅构建前端（tsc 类型检查 + vite）
npm run check:treemap # 矩形树图布局自检（无需浏览器）
```

### 打包 Windows 安装包

统一入口，会自动判断当前系统该走哪条路径：

```bash
npm run build:win            # Windows 本机 → nsis + msi；Linux/macOS → cargo-xwin 交叉编译出 nsis
npm run build:win -- --arch arm64        # 构建 Windows on ARM
npm run build:win -- --bundles nsis      # 只出 .exe 安装包
npm run build:win:check      # 只检查环境是否齐全，不构建
npm run build:win -- --dry-run           # 只打印将要执行的命令
```

产物位于 `src-tauri/target/<target>/release/bundle/`：

- `nsis/*.exe` —— NSIS 安装包
- `msi/*.msi` —— MSI 安装包（**只能在 Windows 上打包**，依赖仅支持 Windows 的 WiX）

想直接用 Tauri 原生命令也可以：`npm run build:win:nsis`、`npm run build:win:msi`。

#### 在 Windows 上构建

装好 Rust（MSVC 工具链）与 Node 后直接跑 `npm run build:win` 即可，
WebView2 运行时与 NSIS/WiX 由 Tauri 自动处理。

#### 在 Linux / macOS 上交叉编译

通过 [cargo-xwin](https://github.com/rust-cross/cargo-xwin) 生成 Windows 安装包
（只能出 NSIS `.exe`）。需要先准备：

```bash
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc

# LLVM 提供 clang / lld / llvm-rc，NSIS 负责打包
sudo apt install llvm clang lld nsis        # Ubuntu / Debian
brew install llvm nsis                       # macOS（还需把 llvm/bin 加入 PATH）
```

然后执行 `npm run build:win`；`npm run build:win:check` 会逐项告诉你缺什么。
如果工具链装在非标准位置，可以用 `--skip-preflight` 跳过检查自行接管。

### 单元测试

```bash
cd src-tauri && cargo test
```

覆盖：目录大小/文件数统计、显示阈值过滤、取消扫描、清空目录保留目录本身。

---

## 无界面模式（便于脚本 / CI 验证）

```bash
cd src-tauri && cargo build
./target/debug/disk-sweeper --scan "C:\"       # 打印各目录占用与最大文件
./target/debug/disk-sweeper --cleanup-list     # 列出所有清理目标、功能说明与实际大小
./target/debug/disk-sweeper --clean user_temp,windows_update_download --yes
```

不带 `--yes` 时只演示将要清理的目标，不会真正删除。

---

## 安全提示

- 本工具会**真正删除文件**。回收站、`Windows.old`、休眠文件等标记为「需确认 / 高风险」的
  目标请阅读说明后再勾选。
- 默认勾选的只有明确「安全」的缓存类目标；风险项默认不勾选。
- 清理被占用的文件会失败并被记录，属于正常现象，不需要强制删除正在运行程序的临时文件。
