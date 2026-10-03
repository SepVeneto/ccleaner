//! 清理目标目录库 + 清理执行。
//!
//! 每个目标都带有：它存放什么（`description`）、为什么删掉不影响正常使用
//! （`safety`）、以及对普通用户的风险等级（`risk`）。

use crate::fsutil;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 清空目录里的内容，保留目录本身
    DirContents,
    /// 删除通配符匹配到的文件
    Glob,
    /// 回收站
    RecycleBin,
    /// 执行一条系统命令（例如关闭休眠以删除 hiberfil.sys）
    #[allow(dead_code)]
    Command,
}

pub struct Spec {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    /// 这里存放的是什么 / 它的作用
    pub description: &'static str,
    /// 为什么清理它是安全的
    pub safety: &'static str,
    pub risk: &'static str,
    pub admin: bool,
    pub default_selected: bool,
    pub kind: Kind,
    pub templates: &'static [&'static str],
    /// 系统命令（kind = Command 时使用）
    pub command: &'static [&'static str],
    /// 额外的注意事项
    pub note: &'static str,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CleanupTarget {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub safety: String,
    pub risk: String,
    pub admin: bool,
    pub default_selected: bool,
    pub kind: String,
    pub templates: Vec<String>,
    pub resolved: Vec<String>,
    pub exists: bool,
    pub size: Option<u64>,
    pub note: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TargetResult {
    pub id: String,
    pub name: String,
    pub freed_bytes: u64,
    pub deleted_files: u64,
    pub errors: Vec<String>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct CleanResult {
    pub freed_bytes: u64,
    pub deleted_files: u64,
    pub errors: Vec<String>,
    pub per_target: Vec<TargetResult>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct EstimateProgress {
    done: usize,
    total: usize,
    current: String,
}

// ---------------------------------------------------------------------------
// 目标目录库
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub fn catalog() -> Vec<Spec> {
    vec![
        Spec {
            id: "user_temp",
            name: "用户临时文件",
            category: "临时文件",
            description: "位于 C:\\Users\\<你>\\AppData\\Local\\Temp。程序运行时把中间数据、解压内容、安装包释放到这里。",
            safety: "临时文件本就是用完即弃，绝大多数程序不会依赖它。正在运行的程序占用的个别文件会被自动跳过。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["%TEMP%"],
            command: &[],
            note: "",
        },
        Spec {
            id: "system_temp",
            name: "系统临时文件",
            category: "临时文件",
            description: "位于 C:\\Windows\\Temp。系统服务与安装程序（MSI/EXE）使用的临时目录。",
            safety: "安装结束后这些文件不再被使用；被占用的文件会跳过，不影响系统。",
            risk: "safe",
            admin: true,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["%SYSTEMROOT%\\Temp"],
            command: &[],
            note: "部分文件需要管理员权限才能删除。",
        },
        Spec {
            id: "windows_update_download",
            name: "Windows 更新下载缓存",
            category: "Windows 更新",
            description: "C:\\Windows\\SoftwareDistribution\\Download 保存已下载但已安装的更新安装包（.cab/.msu）。",
            safety: "更新装好后安装包就没用了；系统在需要时会重新下载。可释放数 GB。",
            risk: "low",
            admin: true,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["%SYSTEMROOT%\\SoftwareDistribution\\Download"],
            command: &[],
            note: "建议先停止 Windows Update 服务；正在被占用的文件会自动跳过。",
        },
        Spec {
            id: "delivery_optimization",
            name: "传递优化缓存",
            category: "Windows 更新",
            description: "Windows 更新用的 P2P 分发缓存（Delivery Optimization）。",
            safety: "只是从其它电脑下载到的更新片段，删除后系统会按需重新获取。",
            risk: "low",
            admin: true,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &[
                "%SYSTEMROOT%\\SoftwareDistribution\\DeliveryOptimization",
                "%SYSTEMROOT%\\ServiceProfiles\\NetworkService\\AppData\\Local\\Microsoft\\Windows\\DeliveryOptimization\\Cache",
            ],
            command: &[],
            note: "",
        },
        Spec {
            id: "prefetch",
            name: "预读取缓存 (Prefetch)",
            category: "系统缓存",
            description: "C:\\Windows\\Prefetch 保存常用程序的加载记录，用于加快启动速度。",
            safety: "删除后 Windows 会在使用过程中自动重建，只是接下来几次启动稍慢一点。",
            risk: "low",
            admin: true,
            default_selected: false,
            kind: Kind::Glob,
            templates: &["%SYSTEMROOT%\\Prefetch\\*.pf"],
            command: &[],
            note: "追求启动速度时可不清理。",
        },
        Spec {
            id: "thumbnail_cache",
            name: "缩略图缓存",
            category: "系统缓存",
            description: "资源管理器为图片/视频生成的缩略图数据库 thumbcache_*.db。",
            safety: "纯缓存，删除后浏览图片时会自动重新生成。是图标/缩略图显示异常的常见修复手段。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::Glob,
            templates: &["%LOCALAPPDATA%\\Microsoft\\Windows\\Explorer\\thumbcache_*.db"],
            command: &[],
            note: "被资源管理器占用时可能失败，可先重启资源管理器。",
        },
        Spec {
            id: "icon_cache",
            name: "图标缓存",
            category: "系统缓存",
            description: "资源管理器的图标缓存数据库 iconcache_*.db。",
            safety: "纯缓存，会自动重建；删除可修复图标错乱的问题。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::Glob,
            templates: &["%LOCALAPPDATA%\\Microsoft\\Windows\\Explorer\\iconcache_*.db"],
            command: &[],
            note: "",
        },
        Spec {
            id: "font_cache",
            name: "字体缓存",
            category: "系统缓存",
            description: "系统字体渲染缓存（FontCache），删除后会自动重建。",
            safety: "由系统自动重建，仅在重建的瞬间略有卡顿。",
            risk: "low",
            admin: true,
            default_selected: false,
            kind: Kind::DirContents,
            templates: &["%SYSTEMROOT%\\ServiceProfiles\\LocalService\\AppData\\Local\\FontCache"],
            command: &[],
            note: "需要 SYSTEM 权限，通常要在管理员模式下才能清理。",
        },
        Spec {
            id: "wer_reports",
            name: "Windows 错误报告",
            category: "日志与诊断",
            description: "程序崩溃后生成的错误报告（WER），用于向微软反馈问题。",
            safety: "只是历史诊断数据，删除后不影响任何程序运行。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &[
                "%PROGRAMDATA%\\Microsoft\\Windows\\WER",
                "%LOCALAPPDATA%\\Microsoft\\Windows\\WER",
            ],
            command: &[],
            note: "",
        },
        Spec {
            id: "crash_dumps",
            name: "程序崩溃转储",
            category: "日志与诊断",
            description: "C:\\Users\\<你>\\AppData\\Local\\CrashDumps 保存崩溃进程的内存转储文件，单个可能上百 MB。",
            safety: "只在排查崩溃时有用，日常可以放心删除。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["%LOCALAPPDATA%\\CrashDumps"],
            command: &[],
            note: "",
        },
        Spec {
            id: "memory_dump",
            name: "系统内存转储",
            category: "日志与诊断",
            description: "蓝屏时生成的 memory.dmp 与 Minidump 文件，可能占用数 GB。",
            safety: "仅在分析蓝屏原因时需要，普通用户可删除。",
            risk: "safe",
            admin: true,
            default_selected: true,
            kind: Kind::Glob,
            templates: &["%SYSTEMROOT%\\memory.dmp", "%SYSTEMROOT%\\Minidump\\*.dmp"],
            command: &[],
            note: "",
        },
        Spec {
            id: "windows_logs",
            name: "系统日志文件",
            category: "日志与诊断",
            description: "C:\\Windows\\Logs 下的安装、更新、组件服务等文本日志。",
            safety: "只是历史记录文本，可安全删除；正在写入的日志会自动跳过。",
            risk: "safe",
            admin: true,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["%SYSTEMROOT%\\Logs"],
            command: &[],
            note: "",
        },
        Spec {
            id: "defender_history",
            name: "Defender 扫描历史",
            category: "日志与诊断",
            description: "Windows Defender 的扫描记录与隔离历史。",
            safety: "只是历史记录，删除不影响实时防护。注意：清理会一并删掉隔离区记录。",
            risk: "medium",
            admin: true,
            default_selected: false,
            kind: Kind::DirContents,
            templates: &["%PROGRAMDATA%\\Microsoft\\Windows Defender\\Scans\\History"],
            command: &[],
            note: "如仍在被 Defender 使用可能失败。",
        },
        Spec {
            id: "recycle_bin",
            name: "回收站",
            category: "回收站",
            description: "各分区根目录下的 $Recycle.Bin，保存你删除的文件。",
            safety: "清空即永久删除这些文件，之后无法还原。请确认里面没有还需要的文件。",
            risk: "safe",
            admin: false,
            default_selected: false,
            kind: Kind::RecycleBin,
            templates: &["%SYSTEMDRIVE%\\$Recycle.Bin"],
            command: &[],
            note: "此操作不可撤销。",
        },
        Spec {
            id: "windows_old",
            name: "旧版 Windows 安装文件",
            category: "系统升级残留",
            description: "C:\\Windows.old，系统升级前的旧系统备份，可回退到上一版本。",
            safety: "删除后无法再回退到旧版本系统，但当前系统不受影响。通常可达 10GB+。",
            risk: "medium",
            admin: true,
            default_selected: false,
            kind: Kind::DirContents,
            templates: &["%SYSTEMDRIVE%\\Windows.old"],
            command: &[],
            note: "系统升级后 10 天内系统可能仍需要它，建议确认不再回退后清理。",
        },
        Spec {
            id: "upgrade_temp",
            name: "系统升级临时文件",
            category: "系统升级残留",
            description: "$WINDOWS.~BT / $WINDOWS.~WS / $GetCurrent 等升级过程留下的临时目录。",
            safety: "升级完成后即为残留文件，可删除。",
            risk: "medium",
            admin: true,
            default_selected: false,
            kind: Kind::DirContents,
            templates: &[
                "%SYSTEMDRIVE%\\$WINDOWS.~BT",
                "%SYSTEMDRIVE%\\$WINDOWS.~WS",
                "%SYSTEMDRIVE%\\$GetCurrent",
            ],
            command: &[],
            note: "",
        },
        Spec {
            id: "d3d_shader_cache",
            name: "DirectX 着色器缓存",
            category: "显卡缓存",
            description: "游戏与图形程序的 DirectX 着色器编译缓存。",
            safety: "删除后首次进入游戏/程序会重新编译着色器，仅短暂卡顿。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["%LOCALAPPDATA%\\D3DSCache"],
            command: &[],
            note: "",
        },
        Spec {
            id: "nvidia_cache",
            name: "NVIDIA 着色器缓存",
            category: "显卡缓存",
            description: "NVIDIA 驱动的 DXCache / GLCache / ComputeCache 缓存。",
            safety: "驱动自动重建，删除后首次运行相关程序稍慢。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &[
                "%LOCALAPPDATA%\\NVIDIA\\DXCache",
                "%LOCALAPPDATA%\\NVIDIA\\GLCache",
                "%LOCALAPPDATA%\\NVIDIA\\ComputeCache",
            ],
            command: &[],
            note: "",
        },
        Spec {
            id: "amd_cache",
            name: "AMD 着色器缓存",
            category: "显卡缓存",
            description: "AMD 驱动的 DxCache / GLCache 着色器缓存。",
            safety: "驱动自动重建，可放心删除。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["%LOCALAPPDATA%\\AMD\\DxCache", "%LOCALAPPDATA%\\AMD\\GLCache"],
            command: &[],
            note: "",
        },
        Spec {
            id: "chrome_cache",
            name: "Chrome 浏览器缓存",
            category: "浏览器缓存",
            description: "Chrome 各用户配置的网页缓存、代码缓存与 GPU 缓存（不含书签、密码、历史）。",
            safety: "只清网页缓存，登录状态与书签都在别的目录，不受影响；首次打开网站会略慢。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &[
                "%LOCALAPPDATA%\\Google\\Chrome\\User Data\\*\\Cache",
                "%LOCALAPPDATA%\\Google\\Chrome\\User Data\\*\\Code Cache",
                "%LOCALAPPDATA%\\Google\\Chrome\\User Data\\*\\GPUCache",
            ],
            command: &[],
            note: "建议先关闭 Chrome。",
        },
        Spec {
            id: "edge_cache",
            name: "Edge 浏览器缓存",
            category: "浏览器缓存",
            description: "Edge 各用户配置的网页缓存、代码缓存与 GPU 缓存。",
            safety: "只清缓存，不影响收藏夹、密码与登录状态。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &[
                "%LOCALAPPDATA%\\Microsoft\\Edge\\User Data\\*\\Cache",
                "%LOCALAPPDATA%\\Microsoft\\Edge\\User Data\\*\\Code Cache",
                "%LOCALAPPDATA%\\Microsoft\\Edge\\User Data\\*\\GPUCache",
            ],
            command: &[],
            note: "建议先关闭 Edge。",
        },
        Spec {
            id: "firefox_cache",
            name: "Firefox 浏览器缓存",
            category: "浏览器缓存",
            description: "Firefox 各配置文件的 cache2 目录。",
            safety: "只清缓存，不影响书签、密码与登录状态。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["%LOCALAPPDATA%\\Mozilla\\Firefox\\Profiles\\*\\cache2"],
            command: &[],
            note: "建议先关闭 Firefox。",
        },
        Spec {
            id: "npm_cache",
            name: "npm 包缓存",
            category: "开发工具缓存",
            description: "npm 下载过的包缓存（_cacache）。",
            safety: "只是下载缓存，下次安装会重新下载，不影响已安装的项目。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["%LOCALAPPDATA%\\npm-cache\\_cacache"],
            command: &[],
            note: "",
        },
        Spec {
            id: "yarn_cache",
            name: "Yarn 包缓存",
            category: "开发工具缓存",
            description: "Yarn 下载的包缓存。",
            safety: "可重新下载，删除不影响已安装项目。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["%LOCALAPPDATA%\\Yarn\\Cache", "%LOCALAPPDATA%\\Yarn\\Berry\\cache"],
            command: &[],
            note: "",
        },
        Spec {
            id: "pnpm_store",
            name: "pnpm 内容寻址存储",
            category: "开发工具缓存",
            description: "pnpm 的全局包存储（store），被多个项目硬链接共享。",
            safety: "清理后其它项目需要重新安装依赖、原来的硬链接会失效。建议仅在大空间紧张时清理。",
            risk: "medium",
            admin: false,
            default_selected: false,
            kind: Kind::DirContents,
            templates: &["%LOCALAPPDATA%\\pnpm\\store"],
            command: &[],
            note: "更推荐使用 pnpm store prune 而非直接删除。",
        },
        Spec {
            id: "pip_cache",
            name: "pip 包缓存",
            category: "开发工具缓存",
            description: "Python pip 下载的 wheel/sdist 缓存。",
            safety: "可重新下载，删除不影响已安装的环境。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["%LOCALAPPDATA%\\pip\\Cache"],
            command: &[],
            note: "",
        },
        Spec {
            id: "nuget_cache",
            name: "NuGet HTTP 缓存",
            category: "开发工具缓存",
            description: ".NET NuGet 的 HTTP 下载缓存。",
            safety: "可重新下载，不影响本地的已还原项目。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["%LOCALAPPDATA%\\NuGet\\v3-cache"],
            command: &[],
            note: "",
        },
        Spec {
            id: "vscode_cache",
            name: "VS Code 缓存",
            category: "开发工具缓存",
            description: "VS Code 的 Cache / CachedData / GPUCache。",
            safety: "只清缓存，扩展、设置与工作区不受影响。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &[
                "%APPDATA%\\Code\\Cache",
                "%APPDATA%\\Code\\CachedData",
                "%APPDATA%\\Code\\GPUCache",
            ],
            command: &[],
            note: "",
        },
        Spec {
            id: "teams_cache",
            name: "Teams 缓存",
            category: "应用缓存",
            description: "Microsoft Teams 的本地缓存（含旧版与新版）。",
            safety: "登录信息在别处保存，删除后需短暂重新加载。",
            risk: "safe",
            admin: false,
            default_selected: false,
            kind: Kind::DirContents,
            templates: &[
                "%APPDATA%\\Microsoft\\Teams\\Cache",
                "%APPDATA%\\Microsoft\\Teams\\Code Cache",
                "%APPDATA%\\Microsoft\\Teams\\GPUCache",
                "%LOCALAPPDATA%\\Microsoft\\Teams\\Cache",
            ],
            command: &[],
            note: "建议先退出 Teams。",
        },
        Spec {
            id: "discord_cache",
            name: "Discord 缓存",
            category: "应用缓存",
            description: "Discord 客户端的 Cache / Code Cache / GPUCache。",
            safety: "只清图片与网页资源缓存，不影响聊天记录（在服务器端）。",
            risk: "safe",
            admin: false,
            default_selected: false,
            kind: Kind::DirContents,
            templates: &[
                "%APPDATA%\\discord\\Cache",
                "%APPDATA%\\discord\\Code Cache",
                "%APPDATA%\\discord\\GPUCache",
            ],
            command: &[],
            note: "",
        },
        Spec {
            id: "hibernation",
            name: "休眠文件 hiberfil.sys",
            category: "系统功能",
            description: "C:\\hiberfil.sys 是休眠功能使用的内存镜像文件，大小≈内存容量。",
            safety: "关闭休眠后系统会删除该文件，可释放数 GB；代价是无法再使用“休眠”与快速启动。",
            risk: "high",
            admin: true,
            default_selected: false,
            kind: Kind::Command,
            templates: &["%SYSTEMDRIVE%\\hiberfil.sys"],
            command: &["powercfg", "-h", "off"],
            note: "会关闭休眠与快速启动，需要管理员权限。可用 powercfg -h on 恢复。",
        },
    ]
}

#[cfg(not(windows))]
pub fn catalog() -> Vec<Spec> {
    vec![
        Spec {
            id: "user_cache",
            name: "用户缓存 (~/.cache)",
            category: "临时文件",
            description: "Linux/macOS 下用户级缓存目录，保存应用可重建的缓存数据。",
            safety: "缓存可以重新生成，删除不影响程序正常使用。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["~/.cache"],
            command: &[],
            note: "",
        },
        Spec {
            id: "system_temp",
            name: "系统临时目录 (/tmp)",
            category: "临时文件",
            description: "系统临时目录，进程运行时的临时文件。",
            safety: "重启后系统本身也会清理；被占用的文件会跳过。",
            risk: "safe",
            admin: false,
            default_selected: false,
            kind: Kind::DirContents,
            templates: &["/tmp"],
            command: &[],
            note: "在 /tmp 清理时请注意正在运行的程序。",
        },
        Spec {
            id: "recycle_bin",
            name: "回收站",
            category: "回收站",
            description: "Freedesktop 回收站 ~/.local/share/Trash。",
            safety: "清空后回收站中的文件将永久删除。",
            risk: "safe",
            admin: false,
            default_selected: false,
            kind: Kind::RecycleBin,
            templates: &["~/.local/share/Trash/files", "~/.local/share/Trash/info"],
            command: &[],
            note: "此操作不可撤销。",
        },
        Spec {
            id: "chrome_cache",
            name: "Chrome 浏览器缓存",
            category: "浏览器缓存",
            description: "Chrome 的 HTTP 缓存。",
            safety: "只清缓存，不影响书签、密码与登录状态。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &[
                "~/.cache/google-chrome",
                "~/Library/Caches/Google/Chrome",
            ],
            command: &[],
            note: "",
        },
        Spec {
            id: "firefox_cache",
            name: "Firefox 浏览器缓存",
            category: "浏览器缓存",
            description: "Firefox 的配置文件缓存目录。",
            safety: "只清缓存，不影响书签与登录状态。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["~/.cache/mozilla", "~/Library/Caches/Firefox"],
            command: &[],
            note: "",
        },
        Spec {
            id: "npm_cache",
            name: "npm 包缓存",
            category: "开发工具缓存",
            description: "npm 的 _cacache 下载缓存。",
            safety: "可以重新下载，删除不影响已安装项目。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["~/.npm/_cacache"],
            command: &[],
            note: "",
        },
        Spec {
            id: "yarn_cache",
            name: "Yarn 包缓存",
            category: "开发工具缓存",
            description: "Yarn 的包缓存目录。",
            safety: "可以重新下载。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["~/.cache/yarn", "~/Library/Caches/Yarn"],
            command: &[],
            note: "",
        },
        Spec {
            id: "pip_cache",
            name: "pip 包缓存",
            category: "开发工具缓存",
            description: "pip 下载的 wheel 缓存。",
            safety: "可以重新下载，不影响已安装环境。",
            risk: "safe",
            admin: false,
            default_selected: true,
            kind: Kind::DirContents,
            templates: &["~/.cache/pip"],
            command: &[],
            note: "",
        },
        Spec {
            id: "cargo_cache",
            name: "Cargo 下载缓存",
            category: "开发工具缓存",
            description: "Rust Cargo 的 registry 下载缓存。",
            safety: "可以重新下载，不会影响已编译产物。",
            risk: "safe",
            admin: false,
            default_selected: false,
            kind: Kind::DirContents,
            templates: &["~/.cargo/registry/cache"],
            command: &[],
            note: "",
        },
    ]
}

// ---------------------------------------------------------------------------
// 解析与查询
// ---------------------------------------------------------------------------

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::DirContents => "dirContents",
        Kind::Glob => "glob",
        Kind::RecycleBin => "recycleBin",
        Kind::Command => "command",
    }
}

fn resolve(spec: &Spec) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for t in spec.templates {
        for p in fsutil::expand_pattern(t) {
            if !p.exists() {
                continue;
            }
            let key = p.to_string_lossy().to_lowercase();
            if seen.insert(key) {
                out.push(p);
            }
        }
    }
    out
}

fn to_target(spec: &Spec) -> CleanupTarget {
    let resolved = resolve(spec);
    CleanupTarget {
        id: spec.id.to_string(),
        name: spec.name.to_string(),
        category: spec.category.to_string(),
        description: spec.description.to_string(),
        safety: spec.safety.to_string(),
        risk: spec.risk.to_string(),
        admin: spec.admin,
        default_selected: spec.default_selected,
        kind: kind_name(spec.kind).to_string(),
        templates: spec.templates.iter().map(|s| s.to_string()).collect(),
        resolved: resolved.iter().map(|p| p.to_string_lossy().to_string()).collect(),
        exists: !resolved.is_empty(),
        size: None,
        note: spec.note.to_string(),
    }
}

pub fn targets() -> Vec<CleanupTarget> {
    catalog().iter().map(to_target).collect()
}

fn quick(path: &Path) -> u64 {
    if path.is_dir() {
        fsutil::dir_size(path).0
    } else {
        std::fs::symlink_metadata(path).map(|m| m.len()).unwrap_or(0)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeEntry {
    pub id: String,
    pub size: u64,
    pub files: u64,
}

/// 估算指定目标的占用大小（并行执行，并汇报进度）。
/// 不带进度的同步版本，供 CLI 使用。
pub fn sizes(ids: &[String]) -> Vec<SizeEntry> {
    catalog()
        .iter()
        .filter(|s| ids.is_empty() || ids.iter().any(|i| i == s.id))
        .map(|spec| {
            let paths = resolve(spec);
            let mut bytes = 0u64;
            let mut files = 0u64;
            for p in &paths {
                if spec.kind == Kind::Glob {
                    bytes += std::fs::symlink_metadata(p).map(|m| m.len()).unwrap_or(0);
                    files += 1;
                } else {
                    let (b, f) = fsutil::dir_size(p);
                    bytes += b;
                    files += f;
                }
            }
            SizeEntry {
                id: spec.id.to_string(),
                size: bytes,
                files,
            }
        })
        .collect()
}

pub fn estimate(app: AppHandle, ids: Vec<String>, cancel: Arc<AtomicBool>) -> Vec<SizeEntry> {
    let catalog = catalog();
    let mut picked: Vec<&Spec> = catalog
        .iter()
        .filter(|s| ids.is_empty() || ids.iter().any(|i| i == s.id))
        .collect();
    picked.sort_by(|a, b| a.id.cmp(b.id));

    let total = picked.len();
    let counter = std::sync::atomic::AtomicUsize::new(0);

    let results: Vec<SizeEntry> = picked
        .par_iter()
        .filter_map(|spec| {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let paths = resolve(spec);
            let mut bytes = 0u64;
            let mut files = 0u64;
            for p in &paths {
                if spec.kind == Kind::Glob {
                    bytes += std::fs::symlink_metadata(p).map(|m| m.len()).unwrap_or(0);
                    files += 1;
                } else {
                    let (b, f) = fsutil::dir_size(p);
                    bytes += b;
                    files += f;
                }
            }
            let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = app.emit(
                "cleanup:progress",
                EstimateProgress {
                    done,
                    total,
                    current: spec.name.to_string(),
                },
            );
            Some(SizeEntry {
                id: spec.id.to_string(),
                size: bytes,
                files,
            })
        })
        .collect();

    results
}

/// 执行清理。`ids` 为空表示清理全部；否则只清理指定 id。
pub fn clean(ids: Vec<String>) -> CleanResult {
    let catalog = catalog();
    let picked: Vec<&Spec> = catalog
        .iter()
        .filter(|s| ids.is_empty() || ids.iter().any(|i| i == s.id))
        .collect();

    let mut result = CleanResult::default();
    for spec in picked {
        let mut tr = TargetResult {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            freed_bytes: 0,
            deleted_files: 0,
            errors: Vec::new(),
        };

        match spec.kind {
            Kind::Command => {
                if spec.command.is_empty() {
                    continue;
                }
                let before: u64 = resolve(spec).iter().map(|p| quick(p)).sum();
                let output = std::process::Command::new(spec.command[0])
                    .args(&spec.command[1..])
                    .output();
                match output {
                    Ok(o) if o.status.success() => {
                        let after: u64 = resolve(spec).iter().map(|p| quick(p)).sum();
                        tr.freed_bytes = before.saturating_sub(after);
                    }
                    Ok(o) => {
                        let msg = String::from_utf8_lossy(&o.stderr).trim().to_string();
                        tr.errors.push(if msg.is_empty() {
                            "命令执行失败（可能需要管理员权限）".to_string()
                        } else {
                            msg
                        });
                    }
                    Err(e) => tr.errors.push(format!("无法执行 {}: {e}", spec.command[0])),
                }
            }
            Kind::Glob => {
                for path in resolve(spec) {
                    match fsutil::remove_path(&path) {
                        Ok((b, f)) => {
                            tr.freed_bytes += b;
                            tr.deleted_files += f;
                        }
                        Err(e) => tr.errors.push(format!("{}: {e}", path.display())),
                    }
                }
            }
            Kind::DirContents | Kind::RecycleBin => {
                for path in resolve(spec) {
                    let (b, f, errs) = fsutil::clear_dir_contents(&path);
                    tr.freed_bytes += b;
                    tr.deleted_files += f;
                    tr.errors.extend(errs);
                }
            }
        }

        result.freed_bytes += tr.freed_bytes;
        result.deleted_files += tr.deleted_files;
        result.errors.extend(tr.errors.iter().cloned());
        result.per_target.push(tr);
    }

    result
}
