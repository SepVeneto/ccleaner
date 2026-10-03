//! C 盘清理助手 —— Tauri 命令入口。

mod cleanup;
mod disk;
mod fsutil;
mod scan;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};

use cleanup::{CleanResult, CleanupTarget, SizeEntry};
use disk::Volume;
use scan::{ScanOptions, ScanResult};

#[derive(Default)]
struct AppState {
    cancel: Mutex<Option<Arc<AtomicBool>>>,
}

/// 当前平台是否为 Windows（前端据此提示 C 盘相关功能）。
#[tauri::command]
fn platform() -> String {
    std::env::consts::OS.to_string()
}

#[tauri::command]
fn list_volumes() -> Vec<Volume> {
    disk::list_volumes()
}

#[tauri::command]
fn default_root() -> String {
    disk::default_root().to_string_lossy().to_string()
}

#[tauri::command]
fn home_dir() -> String {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/"))
        .to_string_lossy()
        .to_string()
}

#[tauri::command]
fn path_exists(path: String) -> bool {
    std::path::Path::new(&path).exists()
}

#[tauri::command]
fn path_size(path: String) -> u64 {
    let p = std::path::Path::new(&path);
    if p.is_dir() {
        fsutil::dir_size(p).0
    } else {
        std::fs::symlink_metadata(p).map(|m| m.len()).unwrap_or(0)
    }
}

#[tauri::command]
async fn scan(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    options: Option<ScanOptions>,
) -> Result<ScanResult, String> {
    let root = PathBuf::from(&path);
    if !root.exists() {
        return Err(format!("路径不存在: {path}"));
    }

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = state.cancel.lock().map_err(|e| e.to_string())?;
        if let Some(prev) = guard.as_ref() {
            prev.store(true, Ordering::Relaxed);
        }
        *guard = Some(Arc::clone(&cancel));
    }

    let opts = options.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || scan::scan(app, root, opts, cancel))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn cancel_scan(state: State<'_, AppState>) {
    if let Ok(guard) = state.cancel.lock() {
        if let Some(c) = guard.as_ref() {
            c.store(true, Ordering::Relaxed);
        }
    }
}

#[tauri::command]
fn cleanup_targets() -> Vec<CleanupTarget> {
    cleanup::targets()
}

#[tauri::command]
async fn estimate_cleanup(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<Vec<SizeEntry>, String> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = state.cancel.lock().map_err(|e| e.to_string())?;
        *guard = Some(Arc::clone(&cancel));
    }
    tauri::async_runtime::spawn_blocking(move || cleanup::estimate(app, ids, cancel))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn run_cleanup(ids: Vec<String>) -> Result<CleanResult, String> {
    tauri::async_runtime::spawn_blocking(move || cleanup::clean(ids))
        .await
        .map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteRequest {
    paths: Vec<String>,
    permanent: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeleteResponse {
    freed_bytes: u64,
    deleted_files: u64,
    errors: Vec<String>,
}

/// 删除任意路径：可以永久删除，也可以移到回收站（Windows）。
#[tauri::command]
async fn delete_paths(req: DeleteRequest) -> Result<DeleteResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut resp = DeleteResponse {
            freed_bytes: 0,
            deleted_files: 0,
            errors: Vec::new(),
        };
        for p in req.paths {
            let path = PathBuf::from(&p);
            if !path.exists() {
                continue;
            }
            if !req.permanent {
                #[cfg(windows)]
                {
                    let size = if path.is_dir() {
                        fsutil::dir_size(&path).0
                    } else {
                        std::fs::symlink_metadata(&path).map(|m| m.len()).unwrap_or(0)
                    };
                    match trash::delete(&path) {
                        Ok(_) => {
                            resp.freed_bytes += size;
                            resp.deleted_files += 1;
                            continue;
                        }
                        Err(e) => {
                            resp.errors.push(format!("{}: {e}", path.display()));
                            continue;
                        }
                    }
                }
            }
            match fsutil::remove_path(&path) {
                Ok((b, f)) => {
                    resp.freed_bytes += b;
                    resp.deleted_files += f;
                }
                Err(e) => resp.errors.push(format!("{}: {e}", path.display())),
            }
        }
        resp
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal(path: String) -> Result<(), String> {
    disk::reveal(std::path::Path::new(&path))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            platform,
            list_volumes,
            default_root,
            home_dir,
            path_exists,
            path_size,
            scan,
            cancel_scan,
            cleanup_targets,
            estimate_cleanup,
            run_cleanup,
            delete_paths,
            reveal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ---------------------------------------------------------------------------
// 无界面模式：便于在服务器 / CI 上验证核心逻辑，也方便自动化脚本调用
//
//   disk-sweeper --scan <路径>        扫描并打印各目录占用
//   disk-sweeper --cleanup-list       列出清理目标与功能说明
//   disk-sweeper --clean <id,id> --yes  执行清理（不带 --yes 只演示）
// ---------------------------------------------------------------------------

fn human(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut idx = 0usize;
    while value >= 1024.0 && idx < UNITS.len() - 1 {
        value /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[idx])
    }
}

fn print_scan(path: &str, res: &ScanResult, depth: usize, node: &scan::Node) {
    if depth == 0 {
        println!("扫描目录: {path}");
        println!(
            "总计 {} · {} 个文件 · {} 个目录 · 耗时 {} ms",
            human(res.total_size), res.total_files, res.total_dirs, res.elapsed_ms
        );
        println!("{:-<72}", "");
    }
    let indent = "  ".repeat(depth);
    let pct = if node.size > 0 {
        (node.size as f64 / res.total_size.max(1) as f64) * 100.0
    } else {
        0.0
    };
    let label = if node.is_dir { "📁" } else { "📄" };
    println!(
        "{indent}{label} {:<38} {:>10}  {:>5.1}%",
        node.name, human(node.size), pct
    );
}

fn walk_print(path: &str, res: &ScanResult, node: &scan::Node, depth: usize, max_depth: usize) {
    print_scan(path, res, depth, node);
    if depth >= max_depth {
        return;
    }
    for child in &node.children {
        walk_print(path, res, child, depth + 1, max_depth);
    }
}

pub fn maybe_run_cli() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "--scan" => {
            let path = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| disk::default_root().to_string_lossy().to_string());
            if !std::path::Path::new(&path).exists() {
                eprintln!("路径不存在: {path}");
                return true;
            }
            let res = scan::scan_tree(
                PathBuf::from(&path),
                ScanOptions::default(),
                Arc::new(AtomicBool::new(false)),
            );
            walk_print(&path, &res, &res.root, 0, 3);
            println!("{:-<72}", "");
            println!("最大的文件:");
            for f in res.top_files.iter().take(10) {
                println!("  {:>10}  {}", human(f.size), f.path);
            }
            true
        }
        "--cleanup-list" => {
            println!("清理目标列表（按分类）\n");
            let all_ids: Vec<String> = cleanup::targets().iter().map(|t| t.id.clone()).collect();
            let sizes: std::collections::HashMap<String, u64> = cleanup::sizes(&all_ids)
                .into_iter()
                .map(|s| (s.id, s.size))
                .collect();
            let mut last = String::new();
            for t in cleanup::targets() {
                if t.category != last {
                    println!("\n【{}】", t.category);
                    last = t.category.clone();
                }
                let size = human(*sizes.get(&t.id).unwrap_or(&0));
                let flag = if t.exists { "" } else { "（未发现）" };
                println!("  • {} [{}]{} {}", t.name, t.risk, flag, size);
                println!("      功能：{}", t.description);
                println!("      可清理：{}", t.safety);
                if !t.note.is_empty() {
                    println!("      注意：{}", t.note);
                }
            }
            true
        }
        "--clean" => {
            let ids: Vec<String> = args
                .get(2)
                .map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
                .unwrap_or_default();
            let confirmed = args.iter().any(|a| a == "--yes");
            let list = cleanup::targets();
            let picked: Vec<_> = list
                .iter()
                .filter(|t| ids.is_empty() || ids.contains(&t.id))
                .collect();
            if !confirmed {
                println!("将要清理以下目标（加上 --yes 才会真正执行）：");
                for t in &picked {
                    println!("  • {} ({})  路径: {}", t.name, t.id, t.resolved.join(", "));
                }
                return true;
            }
            let res = cleanup::clean(ids);
            println!(
                "已释放 {}，删除 {} 个文件",
                human(res.freed_bytes),
                res.deleted_files
            );
            for e in res.errors.iter().take(20) {
                println!("  ! {e}");
            }
            true
        }
        "--help" | "-h" | "--version" => {
            println!("C 盘清理助手 (DiskSweeper) {}", env!("CARGO_PKG_VERSION"));
            println!("用法：");
            println!("  disk-sweeper              启动图形界面");
            println!("  disk-sweeper --scan <路径>");
            println!("  disk-sweeper --cleanup-list");
            println!("  disk-sweeper --clean <id,id> [--yes]");
            true
        }
        _ => false,
    }
}
