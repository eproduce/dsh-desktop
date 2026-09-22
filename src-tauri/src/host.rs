//! dsh Host 子进程的启动、上报解析与关闭。
//!
//! Host 通过 Node 的 IPC 通道上报，而本外壳是 Rust 进程，没有该通道；随包提供的
//! `host/host-bridge.cjs` 负责转换，见该文件的说明。这里的事件名与上游 Electron
//! 外壳一致，因此 Host 侧不需要为换壳做改动。

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 保留的诊断字节数，与上游 Electron 外壳一致。
const MAX_DIAGNOSTIC_BYTES: usize = 64 * 1024;

/// 关闭时序，与上游 Electron 外壳一致。
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);
const TERMINATE_GRACE: Duration = Duration::from_secs(5);
const KILL_GRACE: Duration = Duration::from_secs(5);

/// 轮询子进程状态的间隔。
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Host 启动所需的输入。
#[derive(Debug, Clone)]
pub struct HostConfig {
    /// 运行桥接与 Host 的 Node 可执行文件。
    pub node: String,
    /// dsh Host 入口脚本的绝对路径；缺失时不启动。
    pub entry: Option<String>,
    /// 不可变运行时目录，作为 Host 的 argv[2]。
    pub runtime_dir: Option<String>,
    /// 捆绑的 Python 载荷目录，作为 Host 的 argv[4]；缺失时由 Host 自行推导。
    pub primary_runtime: Option<String>,
    /// 桌面 profile 目录，同时作为 Host 的工作目录。
    pub project_dir: PathBuf,
    /// 桥接脚本路径。
    pub bridge: PathBuf,
}

impl HostConfig {
    /// 从环境变量读取配置。
    ///
    /// `project_dir` 与 `bridge` 由调用方按运行位置提供，因为它们来自应用的
    /// 数据目录而不是用户环境。
    pub fn from_env(project_dir: PathBuf, bridge: PathBuf) -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        Self {
            node: var("DSH_HOST_NODE").unwrap_or_else(|| "node".to_string()),
            entry: var("DSH_HOST_ENTRY"),
            runtime_dir: var("DSH_HOST_RUNTIME"),
            primary_runtime: var("DSH_HOST_PRIMARY_RUNTIME"),
            project_dir,
            bridge,
        }
    }

    /// 缺少哪一项就无法启动。
    fn missing(&self) -> Option<&'static str> {
        if self.entry.is_none() {
            return Some("环境变量 DSH_HOST_ENTRY 未设置");
        }
        if self.runtime_dir.is_none() {
            return Some("环境变量 DSH_HOST_RUNTIME 未设置");
        }
        None
    }

    /// 交给桥接的参数，顺序与 Host 读取的 argv 位置一致。
    fn bridge_arguments(&self) -> Vec<String> {
        let mut arguments = vec![
            self.entry.clone().unwrap_or_default(),
            self.runtime_dir.clone().unwrap_or_default(),
            self.project_dir.to_string_lossy().into_owned(),
        ];
        if let Some(primary) = &self.primary_runtime {
            arguments.push(primary.clone());
        }
        arguments
    }
}

/// Host 上报的事件。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum HostEvent {
    /// Host 已完成初始化，并给出渲染层可访问的地址与启动注入数据。
    Ready {
        url: String,
        #[serde(default)]
        injections: Vec<serde_json::Value>,
    },
    /// Host 无法继续，进程即将退出。
    Fatal { message: String },
    /// 收到关闭请求后已完成收尾。
    ShutdownComplete,
    /// 子进程已退出；由桥接上报。
    Exit {
        #[serde(default)]
        code: i32,
        #[serde(default)]
        signal: String,
    },
}

/// 外壳向加载页公开的 Host 状态。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "phase", rename_all = "kebab-case")]
pub enum HostState {
    /// 未配置 dsh 运行时，Host 不会启动。
    NotConfigured { reason: String },
    /// 桥接进程已创建，尚未上报就绪。
    Starting,
    /// Host 已就绪。
    Ready {
        url: String,
        injections: Vec<serde_json::Value>,
    },
    /// Host 上报了致命错误。
    Failed {
        message: String,
        /// 端口被占用；加载页据此建议退出其它实例。
        port_in_use: bool,
        /// 保留的诊断尾巴，可能包含插件输出。
        detail: String,
    },
    /// 子进程在未收到关闭请求时退出。
    Exited { code: i32, detail: String },
    /// 收到关闭请求并已完成收尾。
    Stopped,
}

/// 外壳发给 Host 的控制消息。
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum HostCommand {
    /// 请求优雅关闭。
    Shutdown,
}

/// 持有 Host 桥接进程、诊断尾巴与最近一次上报的状态。
pub struct HostSupervisor {
    config: HostConfig,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    diagnostics: Arc<Mutex<Vec<u8>>>,
    state: HostState,
    stopping: bool,
}

impl HostSupervisor {
    /// 创建一个尚未启动的监管器。
    pub fn new(config: HostConfig) -> Self {
        let state = match config.missing() {
            Some(reason) => HostState::NotConfigured {
                reason: reason.to_string(),
            },
            None => HostState::Stopped,
        };
        Self {
            config,
            child: None,
            stdin: None,
            diagnostics: Arc::new(Mutex::new(Vec::new())),
            state,
            stopping: false,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> HostState {
        self.state.clone()
    }

    /// 启动桥接进程并把 stdout 交给调用方的读取循环。
    ///
    /// 子进程仍由监管器持有，以便关闭时回收。
    pub fn spawn(&mut self) -> Result<ChildStdout, String> {
        self.stop();
        if let Some(reason) = self.config.missing() {
            self.state = HostState::NotConfigured {
                reason: reason.to_string(),
            };
            return Err(reason.to_string());
        }
        self.stopping = false;
        {
            let mut diagnostics = self.diagnostics();
            diagnostics.clear();
        }
        let mut child = Command::new(&self.config.node)
            .arg(&self.config.bridge)
            .args(self.config.bridge_arguments())
            .current_dir(&self.config.project_dir)
            .env("DSH_HOST_PROJECT", &self.config.project_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("无法启动 {}：{error}", self.config.node))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "未能接管桥接进程的 stdout".to_string())?;
        self.stdin = child.stdin.take();
        if let Some(stderr) = child.stderr.take() {
            collect_diagnostics(stderr, Arc::clone(&self.diagnostics));
        }
        self.child = Some(child);
        self.state = HostState::Starting;
        Ok(stdout)
    }

    /// 记录一次上报的事件。
    pub fn apply(&mut self, event: &HostEvent) {
        let detail = self.diagnostics_text();
        let next = match event {
            HostEvent::Ready { url, injections } => HostState::Ready {
                url: url.clone(),
                injections: injections.clone(),
            },
            HostEvent::Fatal { message } => HostState::Failed {
                port_in_use: is_port_in_use(message),
                message: message.clone(),
                detail,
            },
            HostEvent::ShutdownComplete => HostState::Stopped,
            HostEvent::Exit { code, .. } => {
                if self.stopping {
                    HostState::Stopped
                } else {
                    HostState::Exited { code: *code, detail }
                }
            }
        };
        self.state = next;
    }

    /// 请求关闭，并在超时后升级终止手段。
    ///
    /// 上游先发 SIGTERM 再升级到 SIGKILL；`std::process` 只提供 SIGKILL，因此这里
    /// 的升级是一段而不是两段。桥接被杀后 Host 会因 IPC 通道断开而自行收尾。
    pub fn stop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        self.stopping = true;
        if let Some(mut stdin) = self.stdin.take() {
            let command = serde_json::to_string(&HostCommand::Shutdown)
                .unwrap_or_else(|_| "{\"type\":\"shutdown\"}".to_string());
            let _ = writeln!(stdin, "{command}");
            let _ = stdin.flush();
        }
        if !wait_for_exit(&mut child, SHUTDOWN_GRACE) {
            let _ = child.kill();
            if !wait_for_exit(&mut child, TERMINATE_GRACE) {
                let _ = child.kill();
                let _ = wait_for_exit(&mut child, KILL_GRACE);
            }
        }
        self.state = HostState::Stopped;
    }

    /// 诊断尾巴的文本形式。
    fn diagnostics_text(&self) -> String {
        let guard = self
            .diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        String::from_utf8_lossy(&guard).trim().to_string()
    }

    /// 可写的诊断缓冲区。
    fn diagnostics(&self) -> std::sync::MutexGuard<'_, Vec<u8>> {
        self.diagnostics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for HostSupervisor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 逐行解析 Host 上报，忽略无法识别为事件的输出。
pub fn read_events<R: std::io::BufRead>(reader: R, mut on_event: impl FnMut(HostEvent)) {
    for line in reader.lines().map_while(Result::ok) {
        if let Ok(event) = serde_json::from_str::<HostEvent>(&line) {
            on_event(event);
        }
    }
}

/// 持续读取 stderr，保留最后 `MAX_DIAGNOSTIC_BYTES` 字节。
fn collect_diagnostics(stderr: impl Read + Send + 'static, sink: Arc<Mutex<Vec<u8>>>) {
    std::thread::spawn(move || {
        let mut reader = stderr;
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    let mut guard = sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    guard.extend_from_slice(&buffer[..count]);
                    if guard.len() > MAX_DIAGNOSTIC_BYTES {
                        let overflow = guard.len() - MAX_DIAGNOSTIC_BYTES;
                        guard.drain(..overflow);
                    }
                }
            }
        }
    });
}

/// 轮询直到子进程退出或超时。
fn wait_for_exit(child: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return true,
            Ok(None) => {}
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// 诊断文本是否表示监听端口被占用。
fn is_port_in_use(text: &str) -> bool {
    text.contains("EADDRINUSE")
}
