//! 渲染层可调用的命令，以及 Host 的启动与关闭编排。

use crate::host::{self, HostState};
use crate::{lock, ShellState, MAIN_WINDOW};
use serde::Serialize;
use std::io::BufReader;
use std::process::ChildStdout;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, State};

/// Host 状态变化时推送给前端的通道名。
pub const HOST_STATE_EVENT: &str = "dsh://host-state";

/// 关闭流程只执行一次；重复的关闭请求在收尾期间会再次到达。
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// `boot` 的返回值。字段名与上游 Electron preload 暴露的一致。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootPayload {
    /// Host 就绪后渲染层可访问的地址；未就绪时为 `None`。
    pub stream_base_url: Option<String>,
    /// Host 提供给渲染层的启动注入数据。
    pub injections: Vec<serde_json::Value>,
}

/// 返回 Host 就绪状态与启动注入数据。
#[tauri::command]
pub fn boot(state: State<'_, ShellState>) -> BootPayload {
    let host = lock(&state.host);
    match host.state() {
        HostState::Ready { url, injections } => BootPayload {
            stream_base_url: Some(url),
            injections,
        },
        _ => BootPayload {
            stream_base_url: None,
            injections: Vec::new(),
        },
    }
}

/// 接收渲染层上报的启动失败。
///
/// 原生恢复界面是后续工作；这里只记录，加载页负责呈现。
#[tauri::command]
pub fn boot_failed(message: String) {
    eprintln!("dsh 桌面外壳：渲染层启动失败：{message}");
}

/// 返回 Host 当前状态。
#[tauri::command]
pub fn host_status(state: State<'_, ShellState>) -> HostState {
    lock(&state.host).state()
}

/// 重启 Host 子进程。
#[tauri::command]
pub fn host_restart(app: AppHandle) -> Result<(), String> {
    SHUTTING_DOWN.store(false, Ordering::SeqCst);
    start_host(&app)
}

/// 启动 Host，并把它的上报转发到前端与窗口导航。
pub fn start_host(app: &AppHandle) -> Result<(), String> {
    let spawned = {
        let state = app.state::<ShellState>();
        let mut host = lock(&state.host);
        host.spawn()
    };
    let stdout = match spawned {
        Ok(stdout) => stdout,
        Err(error) => {
            // 未配置是预期状态：加载页会显示原因，所以只广播状态而不终止启动。
            publish(app);
            return Err(error);
        }
    };
    publish(app);
    spawn_reader(app.clone(), stdout);
    Ok(())
}

/// 关闭 Host 后退出应用。
///
/// 收尾最多等待 20 秒，因此放在独立线程上，不阻塞窗口事件线程。
pub fn shutdown_and_exit(app: AppHandle) {
    if SHUTTING_DOWN.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        {
            let state = app.state::<ShellState>();
            let mut host = lock(&state.host);
            host.stop();
        }
        app.exit(0);
    });
}

/// 在独立线程中消费桥接进程的 stdout。
fn spawn_reader(app: AppHandle, stdout: ChildStdout) {
    std::thread::spawn(move || {
        host::read_events(BufReader::new(stdout), |event| {
            let state = app.state::<ShellState>();
            let snapshot = {
                let mut host = lock(&state.host);
                host.apply(&event);
                host.state()
            };
            let _ = app.emit(HOST_STATE_EVENT, &snapshot);
            if let HostState::Ready { url, .. } = &snapshot {
                navigate_main(&app, url);
            }
        });
    });
}

/// 把主窗口导航到 Host 提供的地址。
fn navigate_main(app: &AppHandle, url: &str) {
    let Ok(parsed) = url.parse::<tauri::Url>() else {
        eprintln!("dsh 桌面外壳：Host 给出的地址无效：{url}");
        return;
    };
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || match handle.get_webview_window(MAIN_WINDOW) {
        Some(window) => {
            if let Err(error) = window.navigate(parsed) {
                eprintln!("dsh 桌面外壳：导航到工作区失败：{error}");
            }
        }
        None => eprintln!("dsh 桌面外壳：{MAIN_WINDOW} 窗口不存在"),
    });
}

/// 把当前状态广播给前端。
fn publish(app: &AppHandle) {
    let snapshot = {
        let state = app.state::<ShellState>();
        let host = lock(&state.host);
        host.state()
    };
    let _ = app.emit(HOST_STATE_EVENT, &snapshot);
}
