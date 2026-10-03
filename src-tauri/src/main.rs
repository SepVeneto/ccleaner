// Windows 发布版不弹出控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if disk_sweeper_lib::maybe_run_cli() {
        return;
    }
    disk_sweeper_lib::run()
}
