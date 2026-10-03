//! 目录扫描：构建用于矩形树图可视化的目录树，并统计最大文件。

use crate::fsutil;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BinaryHeap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanOptions {
    /// 返回的目录树最大深度（更深的层级通过“重新扫描该目录”下钻）
    pub max_depth: usize,
    /// 小于该大小的文件不单独显示
    pub min_file_size: u64,
    /// 小于该大小的目录不显示
    pub min_dir_size: u64,
    /// 记录多少个最大的文件
    pub top_files: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            max_depth: 4,
            min_file_size: 1024 * 1024,
            min_dir_size: 2 * 1024 * 1024,
            top_files: 200,
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub files: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ext: Option<String>,
    pub modified: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Node>,
    /// 该目录因为深度限制没有展开子节点（前端可点击下钻）
    pub truncated: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BigFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub root: Node,
    pub total_size: u64,
    pub total_files: u64,
    pub total_dirs: u64,
    pub elapsed_ms: u64,
    pub top_files: Vec<BigFile>,
    pub skipped: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    files: u64,
    bytes: u64,
    current: String,
    elapsed_ms: u64,
}

struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
    len: u64,
    modified: u64,
}

#[derive(Default)]
struct Counters {
    files: AtomicU64,
    dirs: AtomicU64,
    bytes: AtomicU64,
    skipped: AtomicU64,
}

pub struct Scanner {
    pub cancel: Arc<AtomicBool>,
    counters: Counters,
    current: Mutex<String>,
    top: Mutex<BinaryHeap<std::cmp::Reverse<(u64, String)>>>,
    top_limit: usize,
}

impl Scanner {
    pub fn new(cancel: Arc<AtomicBool>, top_limit: usize) -> Self {
        Self {
            cancel,
            counters: Counters::default(),
            current: Mutex::new(String::new()),
            top: Mutex::new(BinaryHeap::new()),
            top_limit,
        }
    }

    fn touch(&self, path: &Path) {
        if let Ok(mut cur) = self.current.lock() {
            if cur.len() < 120 {
                *cur = path.to_string_lossy().to_string();
            }
        }
    }

    fn record_file(&self, path: &Path, len: u64, modified: u64) {
        self.counters.files.fetch_add(1, Ordering::Relaxed);
        self.counters.bytes.fetch_add(len, Ordering::Relaxed);
        let mut heap = match self.top.lock() {
            Ok(h) => h,
            Err(_) => return,
        };
        if heap.len() < self.top_limit {
            heap.push(std::cmp::Reverse((len, path.to_string_lossy().to_string())));
        } else if let Some(std::cmp::Reverse((smallest, _))) = heap.peek() {
            if len > *smallest {
                heap.pop();
                heap.push(std::cmp::Reverse((len, path.to_string_lossy().to_string())));
            }
        }
        let _ = modified;
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

fn modified_secs(md: &std::fs::Metadata) -> u64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ext_of(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .filter(|e| !e.is_empty())
}

fn read_entries(dir: &Path, sc: &Scanner) -> Vec<Entry> {
    let mut out = Vec::new();
    // 用带长路径前缀的路径做实际 IO，但对外保留用户熟悉的路径
    let rd = match std::fs::read_dir(fsutil::extended(dir)) {
        Ok(rd) => rd,
        Err(_) => {
            sc.counters.skipped.fetch_add(1, Ordering::Relaxed);
            return out;
        }
    };
    for entry in rd.flatten() {
        if sc.is_cancelled() {
            break;
        }
        let path = fsutil::strip_extended(&entry.path());
        // 用 symlink_metadata 识别符号链接 / 目录联结点，跳过以避免死循环
        let md = match std::fs::symlink_metadata(&path) {
            Ok(md) => md,
            Err(_) => {
                sc.counters.skipped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };
        if md.file_type().is_symlink() {
            sc.counters.skipped.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let is_dir = md.is_dir();
        out.push(Entry {
            path,
            name: entry.file_name().to_string_lossy().to_string(),
            is_dir,
            len: if is_dir { 0 } else { md.len() },
            modified: modified_secs(&md),
        });
    }
    out
}

/// 递归扫描一个目录，返回该目录的节点（含按大小排序的子节点）。
fn walk(dir: &Path, depth: usize, opts: &ScanOptions, sc: &Scanner) -> Node {
    sc.touch(dir);
    if depth > 0 {
        sc.counters.dirs.fetch_add(1, Ordering::Relaxed);
    }

    // 用户取消了扫描：不再深入，直接返回已统计到的部分
    if sc.is_cancelled() {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| dir.to_string_lossy().to_string());
        return Node {
            name,
            path: dir.to_string_lossy().to_string(),
            size: 0,
            is_dir: true,
            files: 0,
            ext: None,
            modified: 0,
            children: Vec::new(),
            truncated: true,
        };
    }

    let entries = read_entries(dir, sc);
    let mut files: Vec<Entry> = Vec::new();
    let mut dirs: Vec<Entry> = Vec::new();
    for e in entries {
        if e.is_dir {
            dirs.push(e);
        } else {
            files.push(e);
        }
    }

    let mut total: u64 = 0;
    let mut count: u64 = 0;
    let mut children: Vec<Node> = Vec::new();

    for f in &files {
        total += f.len;
        count += 1;
        sc.record_file(&f.path, f.len, f.modified);
        if depth < opts.max_depth && f.len >= opts.min_file_size {
            children.push(Node {
                name: f.name.clone(),
                path: f.path.to_string_lossy().to_string(),
                size: f.len,
                is_dir: false,
                files: 1,
                ext: ext_of(&f.name),
                modified: f.modified,
                children: Vec::new(),
                truncated: false,
            });
        }
    }

    // C 盘的顶层目录通常数量少但体积大，在这一层做并行能明显提速
    let sub: Vec<Node> = if depth == 0 && dirs.len() > 1 {
        dirs.par_iter()
            .map(|d| walk(&d.path, depth + 1, opts, sc))
            .collect()
    } else {
        dirs.iter()
            .map(|d| walk(&d.path, depth + 1, opts, sc))
            .collect()
    };

    let mut truncated = false;
    for node in sub {
        total += node.size;
        count += node.files;
        if node.size >= opts.min_dir_size {
            children.push(node);
        } else if !node.truncated {
            // 太小的目录不展示，但它的空间已经被计入父目录
        }
    }

    // 深度耗尽，标记该目录可继续下钻
    if depth >= opts.max_depth && !dirs.is_empty() {
        truncated = true;
    }

    children.sort_by(|a, b| b.size.cmp(&a.size));

    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| dir.to_string_lossy().to_string());

    let modified = std::fs::metadata(dir).map(|m| modified_secs(&m)).unwrap_or(0);

    Node {
        name,
        path: dir.to_string_lossy().to_string(),
        size: total,
        is_dir: true,
        files: count,
        ext: None,
        modified,
        children,
        truncated,
    }
}

fn assemble(sc: &Scanner, tree: Node, started: Instant) -> ScanResult {
    let mut top: Vec<BigFile> = {
        let heap = sc.top.lock().map(|h| h.clone()).unwrap_or_default();
        heap.into_iter()
            .map(|std::cmp::Reverse((size, path))| {
                let name = Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.clone());
                let modified = std::fs::metadata(&path).map(|m| modified_secs(&m)).unwrap_or(0);
                BigFile { path, name, size, modified }
            })
            .collect()
    };
    top.sort_by(|a, b| b.size.cmp(&a.size));

    ScanResult {
        root: tree,
        total_size: sc.counters.bytes.load(Ordering::Relaxed),
        total_files: sc.counters.files.load(Ordering::Relaxed),
        total_dirs: sc.counters.dirs.load(Ordering::Relaxed),
        elapsed_ms: started.elapsed().as_millis() as u64,
        top_files: top,
        skipped: sc.counters.skipped.load(Ordering::Relaxed),
    }
}

/// 无 GUI 依赖的扫描实现（供 CLI 与单元测试使用）。
pub fn scan_tree(root: PathBuf, opts: ScanOptions, cancel: Arc<AtomicBool>) -> ScanResult {
    let sc = Scanner::new(cancel, opts.top_files.max(1));
    let started = Instant::now();
    let tree = walk(&root, 0, &opts, &sc);
    assemble(&sc, tree, started)
}

/// 执行一次扫描，并在扫描过程中通过事件汇报进度。
pub fn scan(app: AppHandle, root: PathBuf, opts: ScanOptions, cancel: Arc<AtomicBool>) -> ScanResult {
    let sc = Arc::new(Scanner::new(cancel, opts.top_files.max(1)));
    let started = Instant::now();

    // 进度上报线程
    let reporter = {
        let sc = Arc::clone(&sc);
        let app = app.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(200));
                let progress = Progress {
                    files: sc.counters.files.load(Ordering::Relaxed),
                    bytes: sc.counters.bytes.load(Ordering::Relaxed),
                    current: sc.current.lock().map(|c| c.clone()).unwrap_or_default(),
                    elapsed_ms: started.elapsed().as_millis() as u64,
                };
                let _ = app.emit("scan:progress", progress);
            }
        });
        (stop, handle)
    };

    let tree = walk(&root, 0, &opts, &sc);

    reporter.0.store(true, Ordering::Relaxed);
    let _ = reporter.1.join();

    assemble(&sc, tree, started)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::SystemTime;

    fn temp_base(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("disk_sweeper_test_{tag}_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn opts() -> ScanOptions {
        ScanOptions {
            max_depth: 5,
            min_file_size: 1,
            min_dir_size: 1,
            top_files: 10,
        }
    }

    #[test]
    fn counts_sizes_and_nests_directories() {
        let base = temp_base("scan");
        fs::create_dir_all(base.join("a/b")).unwrap();
        fs::create_dir_all(base.join("c")).unwrap();
        fs::write(base.join("a/big.bin"), vec![0u8; 3_000_000]).unwrap();
        fs::write(base.join("a/b/small.bin"), vec![0u8; 1_000]).unwrap();
        fs::write(base.join("c/two.bin"), vec![0u8; 2_000_000]).unwrap();

        let res = scan_tree(base.clone(), opts(), Arc::new(AtomicBool::new(false)));

        assert_eq!(res.total_files, 3, "应统计到 3 个文件");
        assert_eq!(res.total_size, 5_001_000, "应统计到全部字节数");
        assert_eq!(res.root.children.len(), 2, "根目录下应有 a、c 两个子目录");
        // 子目录按大小降序：a (3.001MB) > c (2MB)
        assert_eq!(res.root.children[0].name, "a");
        assert_eq!(res.root.children[0].size, 3_001_000);
        assert!(res.root.children[0].children.iter().any(|n| n.name == "b"));
        assert_eq!(res.top_files.len(), 3);
        assert_eq!(res.top_files[0].size, 3_000_000, "最大文件应排在最前");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn respects_min_file_size_for_display() {
        let base = temp_base("minsize");
        fs::create_dir_all(base.join("d")).unwrap();
        fs::write(base.join("d/big.bin"), vec![0u8; 2_000_000]).unwrap();
        fs::write(base.join("d/tiny.bin"), vec![0u8; 10]).unwrap();

        let mut o = opts();
        o.min_file_size = 1_000_000;
        o.min_dir_size = 1;
        let res = scan_tree(base.clone(), o, Arc::new(AtomicBool::new(false)));

        // 体积统计不受显示阈值影响
        assert_eq!(res.total_size, 2_000_010);
        let d = res.root.children.iter().find(|n| n.name == "d").expect("应有子目录 d");
        // 但小文件不会作为可见子节点出现
        assert_eq!(d.children.len(), 1);
        assert_eq!(d.children[0].name, "big.bin");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn cancel_stops_scan_immediately() {
        let base = temp_base("cancel");
        fs::create_dir_all(base.join("x")).unwrap();
        fs::write(base.join("x/f.bin"), vec![0u8; 5_000]).unwrap();

        let cancel = Arc::new(AtomicBool::new(true));
        let res = scan_tree(base.clone(), opts(), cancel);
        assert_eq!(res.total_files, 0, "取消后不应继续统计");
        assert_eq!(res.total_size, 0);

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn clear_dir_contents_keeps_directory() {
        let base = temp_base("clear");
        let sub = base.join("nested");
        fs::create_dir_all(&sub).unwrap();
        fs::write(base.join("one.tmp"), vec![0u8; 1_500]).unwrap();
        fs::write(sub.join("two.tmp"), vec![0u8; 2_500]).unwrap();

        let (bytes, files, errors) = fsutil::clear_dir_contents(&base);
        assert!(base.exists(), "目录本身应保留");
        assert_eq!(files, 2);
        assert_eq!(bytes, 4_000);
        assert!(errors.is_empty(), "不应有错误: {errors:?}");
        assert_eq!(fs::read_dir(&base).unwrap().count(), 0);

        let _ = fs::remove_dir_all(&base);
    }
}
