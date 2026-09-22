//! DeepSeek Harness 桌面外壳的 Rust 核心。
//!
//! 职责：注册 `dsh-app` 协议提供加载页、监管 dsh Host 子进程、持有窗口，并向
//! 渲染层注入与上游 Electron preload 同名的桥接对象。

mod bridge;
mod commands;
mod host;
mod protocol;

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

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

/// 运行期需要的位置。
struct ShellPaths {
    /// 桌面 profile 目录，同时作为 Host 的工作目录。
    profile: PathBuf,
    /// 桥接脚本路径。
    bridge: PathBuf,
}

/// 准备数据目录，并把嵌入的桥接脚本写入缓存目录。
///
/// 每次都覆盖，使开发运行与打包运行使用同一份脚本，不需要按运行位置分支解析资源。
fn resolve_paths(app: &tauri::AppHandle) -> Result<ShellPaths, Box<dyn std::error::Error>> {
    let profile = app.path().app_data_dir()?.join("profile");
    fs::create_dir_all(&profile)?;
    let cache = app.path().app_cache_dir()?;
    fs::create_dir_all(&cache)?;
    let bridge = cache.join("host-bridge.cjs");
    fs::write(&bridge, include_str!("../../host/host-bridge.cjs"))?;
    Ok(ShellPaths { profile, bridge })
}

/// 构建并运行桌面应用。
pub fn run() {
    let builder = tauri::Builder::default().invoke_handler(tauri::generate_handler![
        commands::boot,
        commands::boot_failed,
        commands::host_status,
        commands::host_restart,
    ]);

    protocol::register(builder)
        .setup(|app| {
            let paths = resolve_paths(app.handle())?;
            app.manage(ShellState {
                host: Mutex::new(host::HostSupervisor::new(host::HostConfig::from_env(
                    paths.profile,
                    paths.bridge,
                ))),
            });

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

            if let Err(error) = commands::start_host(&window.app_handle().clone()) {
                eprintln!("dsh 桌面外壳：{error}");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Host 必须在进程退出前收尾，因此先拦下关闭，收尾完成后再退出。
                api.prevent_close();
                commands::shutdown_and_exit(window.app_handle().clone());
            }
        })
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}
