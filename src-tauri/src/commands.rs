//! 渲染层可调用的命令，以及 Host 的启动编排。

use crate::host::{self, HostState};
use crate::{lock, ShellState, MAIN_WINDOW};
use serde::Serialize;
use std::io::BufReader;
use std::process::ChildStdout;
use tauri::{AppHandle, Emitter, Manager, State};

/// Host 状态变化时推送给前端的通道名。
pub const HOST_STATE_EVENT: &str = "dsh://host-state";

/// `boot` 的返回值。字段名与既有 Electron preload 暴露的一致。
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
    BootPayload {
        stream_base_url: match host.state() {
            HostState::Ready { url } => Some(url),
            _ => None,
        },
        injections: Vec::new(),
    }
}

/// 接收渲染层上报的启动失败。
///
/// 恢复界面由外壳负责，这里只记录；原生恢复对话框是后续工作。
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
    start_host(&app)
}

/// 启动 Host，并把它的上报转发到前端与窗口导航。
///
/// 只应在主线程调用；读取循环在独立线程中运行。
pub fn start_host(app: &AppHandle) -> Result<(), String> {
    let stdout = {
        let state = app.state::<ShellState>();
        let mut host = lock(&state.host);
        host.spawn()
    };
    let stdout = match stdout {
        Ok(stdout) => stdout,
        Err(error) => {
            // 未配置是预期状态：加载页会显示原因，所以只广播状态而不报错。
            publish(app);
            return Err(error);
        }
    };
    publish(app);
    spawn_reader(app.clone(), stdout);
    Ok(())
}

/// 在独立线程中消费 Host 的 stdout。
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
            if let HostState::Ready { url } = &snapshot {
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
    let _ = app.run_on_main_thread(move || {
        match handle.get_webview_window(MAIN_WINDOW) {
            Some(window) => {
                if let Err(error) = window.navigate(parsed) {
                    eprintln!("dsh 桌面外壳：导航到工作区失败：{error}");
                }
            }
            None => eprintln!("dsh 桌面外壳：{MAIN_WINDOW} 窗口不存在"),
        }
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
