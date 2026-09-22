//! DeepSeek Harness 桌面外壳的 Rust 核心。
//!
//! 职责：注册 `dsh-app` 协议提供加载页、监管 dsh Host 子进程、持有窗口，并向
//! 渲染层注入与既有 Electron preload 同名的桥接对象。

mod bridge;
mod commands;
mod host;
mod protocol;

use std::sync::{Mutex, MutexGuard};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// 主窗口的标签。
pub const MAIN_WINDOW: &str = "main";

/// 外壳的共享状态。
pub struct ShellState {
    /// Host 子进程与它最近一次上报的状态。
    pub host: Mutex<host::HostSupervisor>,
}

/// 取出互斥锁。
///
/// 持锁方 panic 时沿用内部值，避免把一次状态丢失升级成后续每次调用都 panic。
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 构建并运行桌面应用。
pub fn run() {
    let builder = tauri::Builder::default()
        .manage(ShellState {
            host: Mutex::new(host::HostSupervisor::new(host::HostConfig::from_env())),
        })
        .invoke_handler(tauri::generate_handler![
            commands::boot,
            commands::boot_failed,
            commands::host_status,
            commands::host_restart,
        ]);

    protocol::register(builder)
        .setup(|app| {
            let window = WebviewWindowBuilder::new(
                app,
                MAIN_WINDOW,
                WebviewUrl::CustomProtocol(protocol::loading_url()?),
            )
            .title("DeepSeek Harness")
            .inner_size(1280.0, 820.0)
            .min_inner_size(960.0, 640.0)
            .initialization_script(bridge::initialization_script(std::env::consts::OS))
            .build()?;

            // 未找到 dsh 运行时是预期状态：加载页会显示原因并保持窗口可用，
            // 因此这里不终止启动。
            if let Err(error) = commands::start_host(&window.app_handle().clone()) {
                eprintln!("dsh 桌面外壳：{error}");
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}
