//! 跨平台的路径与文件系统小工具。

use std::path::{Path, PathBuf};

/// 展开路径中的环境变量与特殊标记。
///
/// 支持：
/// - `%VAR%`（Windows 风格，在其它平台也会尝试按环境变量匹配）
/// - `${VAR}` / `$VAR`
/// - `~` 表示当前用户主目录
pub fn expand(input: &str) -> PathBuf {
    let mut out = String::with_capacity(input.len());
    let bytes: Vec<char> = input.chars().collect();
    let mut i = 0usize;

    while i < bytes.len() {
        let c = bytes[i];
        if c == '%' {
            if let Some(end) = bytes[i + 1..].iter().position(|&x| x == '%') {
                let name: String = bytes[i + 1..i + 1 + end].iter().collect();
                if !name.is_empty() {
                    if let Some(val) = get_env(&name) {
                        out.push_str(&val);
                        i += end + 2;
                        continue;
                    }
                }
            }
            out.push(c);
            i += 1;
        } else if c == '$' && i + 1 < bytes.len() && bytes[i + 1] == '{' {
            if let Some(end) = bytes[i + 2..].iter().position(|&x| x == '}') {
                let name: String = bytes[i + 2..i + 2 + end].iter().collect();
                if let Some(val) = get_env(&name) {
                    out.push_str(&val);
                    i += end + 3;
                    continue;
                }
            }
            out.push(c);
            i += 1;
        } else if c == '$' && i + 1 < bytes.len() && bytes[i + 1].is_alphanumeric() {
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j].is_alphanumeric() || bytes[j] == '_') {
                j += 1;
            }
            let name: String = bytes[i + 1..j].iter().collect();
            if let Some(val) = get_env(&name) {
                out.push_str(&val);
                i = j;
                continue;
            }
            out.push(c);
            i += 1;
        } else if c == '~' && (i == 0 || bytes[i - 1] == '/') && (i + 1 == bytes.len() || bytes[i + 1] == '/') {
            if let Some(home) = dirs::home_dir() {
                out.push_str(&home.to_string_lossy());
            } else {
                out.push(c);
            }
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }

    PathBuf::from(out)
}

fn get_env(name: &str) -> Option<String> {
    if let Ok(v) = std::env::var(name) {
        if !v.is_empty() {
            return Some(v);
        }
    }
    // Windows 环境变量通常大小写不敏感，这里做个兜底匹配
    for (k, v) in std::env::vars() {
        if k.eq_ignore_ascii_case(name) && !v.is_empty() {
            return Some(v);
        }
    }
    None
}

/// 规范化分隔符，统一用 `/`，便于 glob 与展示。
pub fn normalize_sep(s: &str) -> String {
    s.replace('\\', "/")
}

/// 展开可能包含通配符的路径模板（用于 `User Data/*/Cache` 这类目录）。
pub fn expand_pattern(input: &str) -> Vec<PathBuf> {
    let expanded = expand(input);
    let raw = normalize_sep(&expanded.to_string_lossy());

    if !raw.contains('*') && !raw.contains('?') && !raw.contains('[') {
        return vec![PathBuf::from(raw)];
    }

    // 只把含通配符的片段当作模式，其余片段做字面量转义，避免路径里的 [ ] 被误解析
    let mut pattern = String::new();
    for (idx, seg) in raw.split('/').enumerate() {
        if idx > 0 {
            pattern.push('/');
        }
        if seg.contains('*') || seg.contains('?') || seg.contains('[') {
            pattern.push_str(seg);
        } else {
            pattern.push_str(&glob::Pattern::escape(seg));
        }
    }

    match glob::glob(&pattern) {
        Ok(paths) => paths.flatten().collect(),
        Err(_) => Vec::new(),
    }
}

/// 在 Windows 上为超长路径加上 `\\?\` 前缀。
pub fn extended(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let s = path.to_string_lossy();
        if path.is_absolute() && !s.starts_with(r"\\?\") {
            return PathBuf::from(format!(r"\\?\{}", s));
        }
    }
    path.to_path_buf()
}

/// 去掉 `\\?\` / `\\?\UNC\` 前缀，还原成用户熟悉的路径。
pub fn strip_extended(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

/// 递归统计目录占用（跳过符号链接/联结点，避免死循环）。返回 (字节数, 文件数)。
pub fn dir_size(path: &Path) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    let walker = walkdir::WalkDir::new(extended(path))
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !e.file_type().is_symlink());

    for entry in walker.flatten() {
        if entry.file_type().is_file() {
            if let Ok(md) = entry.metadata() {
                bytes += md.len();
                files += 1;
            }
        }
    }
    (bytes, files)
}

/// 删除文件或目录，返回释放的 (字节数, 文件数)。
pub fn remove_path(path: &Path) -> Result<(u64, u64), String> {
    let md = std::fs::symlink_metadata(path).map_err(|e| format!("无法访问: {e}"))?;

    if md.is_dir() && !md.file_type().is_symlink() {
        let (bytes, files) = dir_size(path);
        let target = extended(path);
        if let Err(err) = std::fs::remove_dir_all(&target) {
            // Windows 上常见原因是只读属性，清除后重试一次
            let _ = clear_readonly_recursive(path);
            std::fs::remove_dir_all(&target).map_err(|e| format!("{err} / {e}"))?;
        }
        Ok((bytes, files))
    } else {
        let size = md.len();
        if let Err(err) = std::fs::remove_file(path) {
            let _ = clear_readonly(path);
            std::fs::remove_file(path).map_err(|e| format!("{err} / {e}"))?;
        }
        Ok((size, 1))
    }
}

/// 清空某个目录里的内容，但保留目录本身。
pub fn clear_dir_contents(dir: &Path) -> (u64, u64, Vec<String>) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut errors = Vec::new();

    let rd = match std::fs::read_dir(extended(dir)) {
        Ok(rd) => rd,
        Err(e) => {
            errors.push(format!("{}: {e}", dir.display()));
            return (bytes, files, errors);
        }
    };

    for entry in rd.flatten() {
        let path = entry.path();
        match remove_path(&path) {
            Ok((b, f)) => {
                bytes += b;
                files += f;
            }
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }
    (bytes, files, errors)
}

fn clear_readonly(path: &Path) -> std::io::Result<()> {
    let md = std::fs::symlink_metadata(path)?;
    let mut perms = md.permissions();
    if perms.readonly() {
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn clear_readonly_recursive(path: &Path) -> std::io::Result<()> {
    for entry in walkdir::WalkDir::new(path).follow_links(false).into_iter().flatten() {
        let _ = clear_readonly(entry.path());
    }
    Ok(())
}
