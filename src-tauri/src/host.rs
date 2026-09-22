//! dsh Host 子进程的启动、上报解析与关闭。
//!
//! Host 在 stdout 上按行上报 JSON。这里的事件类型与既有 Electron 外壳的
//! `DesktopHostEvent` 同名，因此 Host 侧不需要为换壳做改动。

use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::process::{Child, ChildStdout, Command, Stdio};

/// Host 启动所需的输入，全部来自环境变量。
#[derive(Debug, Clone)]
pub struct HostConfig {
    /// 运行 Host 的 Node 可执行文件。
    pub node: String,
    /// dsh CLI 入口脚本的绝对路径；缺失时不启动 Host。
    pub entry: Option<String>,
    /// 追加到入口脚本之后的参数。
    pub args: Vec<String>,
}

impl HostConfig {
    /// 读取 `DSH_HOST_NODE`、`DSH_HOST_ENTRY` 与 `DSH_HOST_ARGS`。
    pub fn from_env() -> Self {
        Self {
            node: std::env::var("DSH_HOST_NODE").unwrap_or_else(|_| "node".to_string()),
            entry: std::env::var("DSH_HOST_ENTRY")
                .ok()
                .filter(|entry| !entry.is_empty()),
            args: std::env::var("DSH_HOST_ARGS")
                .map(|args| args.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default(),
        }
    }
}

/// Host 在 stdout 上上报的事件。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum HostEvent {
    /// Host 已完成初始化，并给出渲染层可访问的地址。
    Ready { url: String },
    /// Host 无法继续，进程即将退出。
    Fatal { message: String },
    /// 收到关闭请求后已完成收尾。
    ShutdownComplete,
}

/// 外壳向加载页公开的 Host 状态。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "phase", rename_all = "kebab-case")]
pub enum HostState {
    /// 未配置 dsh 运行时，Host 不会启动。
    NotConfigured { reason: String },
    /// 子进程已创建，尚未上报就绪。
    Starting,
    /// Host 已就绪。
    Ready { url: String },
    /// Host 上报了致命错误。
    Failed { message: String },
    /// 子进程已退出。
    Stopped,
}

/// 持有 Host 子进程及其最近一次上报的状态。
pub struct HostSupervisor {
    config: HostConfig,
    child: Option<Child>,
    state: HostState,
}

impl HostSupervisor {
    /// 创建一个尚未启动的监管器。
    pub fn new(config: HostConfig) -> Self {
        let state = match config.entry {
            Some(_) => HostState::Stopped,
            None => HostState::NotConfigured {
                reason: "环境变量 DSH_HOST_ENTRY 未设置".to_string(),
            },
        };
        Self {
            config,
            child: None,
            state,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> HostState {
        self.state.clone()
    }

    /// 启动子进程并把 stdout 交给调用方的读取循环。
    ///
    /// 子进程仍由监管器持有，以便关闭时回收。
    pub fn spawn(&mut self) -> Result<ChildStdout, String> {
        self.stop();
        let Some(entry) = self.config.entry.clone() else {
            self.state = HostState::NotConfigured {
                reason: "环境变量 DSH_HOST_ENTRY 未设置".to_string(),
            };
            return Err("dsh Host 未配置".to_string());
        };
        let mut child = Command::new(&self.config.node)
            .arg(entry)
            .args(&self.config.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("无法启动 {}：{error}", self.config.node))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "未能接管 Host 的 stdout".to_string())?;
        self.child = Some(child);
        self.state = HostState::Starting;
        Ok(stdout)
    }

    /// 记录一次上报的事件。
    pub fn apply(&mut self, event: &HostEvent) {
        self.state = match event {
            HostEvent::Ready { url } => HostState::Ready { url: url.clone() },
            HostEvent::Fatal { message } => HostState::Failed {
                message: message.clone(),
            },
            HostEvent::ShutdownComplete => HostState::Stopped,
        };
    }

    /// 终止子进程并释放句柄；未启动时是空操作。
    pub fn stop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let _ = child.kill();
        let _ = child.wait();
        self.state = HostState::Stopped;
    }
}

impl Drop for HostSupervisor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 逐行解析 Host 上报，忽略无法识别为事件的输出。
pub fn read_events<R: BufRead>(reader: R, mut on_event: impl FnMut(HostEvent)) {
    for line in reader.lines().map_while(Result::ok) {
        if let Ok(event) = serde_json::from_str::<HostEvent>(&line) {
            on_event(event);
        }
    }
}
