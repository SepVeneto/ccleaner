//! 磁盘 / 卷信息与系统集成（在文件管理器中定位）。

use serde::Serialize;
use std::path::{Path, PathBuf};
use sysinfo::Disks;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub name: String,
    pub mount: String,
    pub total: u64,
    pub free: u64,
    pub file_system: String,
    pub removable: bool,
    pub is_system: bool,
}

pub fn list_volumes() -> Vec<Volume> {
    let disks = Disks::new_with_refreshed_list();
    let system_drive = std::env::var("SystemDrive").unwrap_or_default().to_lowercase();
    let mut out: Vec<Volume> = disks
        .list()
        .iter()
        .filter(|d| d.total_space() > 0)
        .map(|d| {
            let mount = d.mount_point().to_string_lossy().to_string();
            let is_system = !system_drive.is_empty()
                && mount.to_lowercase().starts_with(&system_drive);
            Volume {
                name: d.name().to_string_lossy().to_string(),
                mount,
                total: d.total_space(),
                free: d.available_space(),
                file_system: d.file_system().to_string_lossy().to_string(),
                removable: d.is_removable(),
                is_system,
            }
        })
        .collect();

    out.sort_by(|a, b| b.total.cmp(&a.total));

    #[cfg(not(windows))]
    if out.is_empty() {
        out.push(Volume {
            name: "/".to_string(),
            mount: "/".to_string(),
            total: 0,
            free: 0,
            file_system: String::new(),
            removable: false,
            is_system: true,
        });
    }
    out
}

/// 默认扫描根目录：Windows 上是系统盘（通常是 C:\），其它平台是用户主目录。
pub fn default_root() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(drive) = std::env::var("SystemDrive") {
            if !drive.is_empty() {
                let p = PathBuf::from(format!("{}\\", drive.trim_end_matches('\\')));
                if p.exists() {
                    return p;
                }
            }
        }
        PathBuf::from("C:\\")
    }
    #[cfg(not(windows))]
    {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
    }
}

/// 在系统文件管理器中定位到某个路径。
pub fn reveal(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        // explorer 只认反斜杠分隔的原生路径；正斜杠或 `\\?\` 前缀会让它
        // 当成无效参数，进而回退到“文档”目录。这里先归一化再使用。
        let target = PathBuf::from(
            crate::fsutil::strip_extended(path).to_string_lossy().replace('/', "\\"),
        );

        // 目录：直接打开该目录；文件/不存在：在父目录中选中，父目录也不存在时
        // 直接打开最近的已存在祖先，避免 explorer 落到“文档”。
        let arg = if target.is_dir() {
            format!("\"{}\"", target.display())
        } else if target.parent().map(|p| p.exists()).unwrap_or(false) {
            format!("/select,\"{}\"", target.display())
        } else {
            let existing = target
                .ancestors()
                .find(|p| p.exists())
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| target.clone());
            format!("\"{}\"", existing.display())
        };

        // 用 raw_arg 原样传入，只给路径本身加引号（explorer 的 /select, 参数
        // 不接受整体加引号，否则同样会回退到“文档”）。
        std::process::Command::new("explorer")
            .raw_arg(arg)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("open");
        if path.is_dir() {
            cmd.arg(path);
        } else {
            cmd.arg("-R").arg(path);
        }
        cmd.spawn().map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let target = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("/"))
        };
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
