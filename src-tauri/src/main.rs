//! Tauri 应用入口；平台相关的启动细节都在 `dsh_desktop_lib::run` 中。

// 发布构建不弹出额外的控制台窗口。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dsh_desktop_lib::run()
}
