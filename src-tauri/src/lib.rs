//! DeepSeek Harness 桌面外壳的 Rust 核心。
//!
//! 职责：注册 `dsh-app` 协议提供加载页、监管 dsh Host 子进程、持有窗口，并向
//! 渲染层注入与上游 Electron preload 同名的桥接对象。

mod bridge;
mod commands;
mod host;
mod profile;
mod protocol;

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// 主窗口的标签。
pub const MAIN_WINDOW: &str = "main";

/// Harness 数据根目录的环境变量名，与上游 `@deepseek-ai/dsh-home-paths` 一致。
const DSH_HOME_ENV: &str = "DSH_HOME";

/// 数据根目录未配置时的子目录名，与上游一致。
const DSH_HOME_DIR_NAME: &str = ".dsh";

/// 外壳的共享状态。
pub struct ShellState {
    /// Host 子进程与它最近一次上报的状态。
    pub host: Mutex<host::HostSupervisor>,
    /// 最近一次广播过的全屏状态；`None` 表示尚未广播过。
    pub fullscreen: Mutex<Option<bool>>,
}

/// 取出互斥锁。
///
/// 持锁方 panic 时沿用内部值，避免把一次状态丢失升级成后续每次调用都 panic。
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 运行期需要的位置。
struct ShellPaths {
    /// 桌面 profile 目录，同时作为 Host 的工作目录。
    profile: PathBuf,
    /// 桥接脚本路径。
    bridge: PathBuf,
}

/// 展开当前用户的 `~` 前缀；其它形式原样返回。
///
/// 与上游一致：只有单独的 `~` 与 `~/`、`~\` 会被展开，`~user` 形式不动。
fn expand_tilde(value: &str, home: &std::path::Path) -> PathBuf {
    match value.strip_prefix('~') {
        None => PathBuf::from(value),
        Some("") => home.to_path_buf(),
        Some(rest) if rest.starts_with('/') || rest.starts_with('\\') => {
            home.join(rest.trim_start_matches(['/', '\\']))
        }
        Some(_) => PathBuf::from(value),
    }
}

/// 解析 Harness 数据根目录。
///
/// 优先级与上游 `resolveDshHome` 一致：`DSH_HOME` 优先（空白值视为未设置），否则
/// `~/.dsh`；结果同样规范化为绝对路径，因此相对的 `DSH_HOME` 按当前工作目录解析，
/// 而不会相对 Host 的工作目录建目录。必须取到与上游、CLI 相同的根，否则换壳后
/// 看不到用户已有的会话与设置。
fn resolve_dsh_home(app: &tauri::AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let home = app.path().home_dir()?;
    let selected = match std::env::var(DSH_HOME_ENV).ok() {
        Some(configured) if !configured.trim().is_empty() => expand_tilde(&configured, &home),
        _ => home.join(DSH_HOME_DIR_NAME),
    };
    Ok(std::path::absolute(selected)?)
}

/// 准备数据目录，并把嵌入的桥接脚本写入缓存目录。
///
/// 每次都覆盖，使开发运行与打包运行使用同一份脚本，不需要按运行位置分支解析资源。
fn resolve_paths(app: &tauri::AppHandle) -> Result<ShellPaths, Box<dyn std::error::Error>> {
    // 位置与上游 `resolveDesktopPaths` 一致，两个外壳因此共享同一个 profile。
    let profile = resolve_dsh_home(app)?.join("profiles").join("desktop");
    // Host 读不到 profile 清单就直接退出，而上游把这个目录的创建交给外壳。
    profile::ensure_profile(&profile)?;
    let cache = app.path().app_cache_dir()?;
    fs::create_dir_all(&cache)?;
    let bridge = cache.join("host-bridge.cjs");
    fs::write(&bridge, include_str!("../../host/host-bridge.cjs"))?;
    Ok(ShellPaths { profile, bridge })
}

/// 构建并运行桌面应用。
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::boot,
            commands::boot_failed,
            commands::host_status,
            commands::host_restart,
            commands::pick_directory,
        ]);

    protocol::register(builder)
        .setup(|app| {
            let paths = resolve_paths(app.handle())?;
            app.manage(ShellState {
                host: Mutex::new(host::HostSupervisor::new(host::HostConfig::from_env(
                    paths.profile,
                    paths.bridge,
                ))),
                fullscreen: Mutex::new(None),
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
            // Tauri 默认安装一个认领拖放的处理器；认领时 wry 不调用 WebKit 的
            // `super`，Web 进程因此收不到 `dragenter`/`dragover`/`drop`，HTML5
            // 拖放整体失效。上游对话输入区靠 DOM drop 接收拖入的附件，所以必须
            // 关掉它。代价是拿不到被拖文件的磁盘路径，`__DSH_HOST_PATHS__` 没有
            // 来源，`@路径` 芯片不可用，拖入的文件一律按上传处理（见 roadmap D3）。
            .disable_drag_drop_handler()
            .build()?;

            if let Err(error) = commands::start_host(&window.app_handle().clone()) {
                eprintln!("dsh 桌面外壳：{error}");
            }
            commands::publish_fullscreen(&window.app_handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    // Host 必须在进程退出前收尾，因此先拦下关闭，收尾完成后再退出。
                    api.prevent_close();
                    commands::shutdown_and_exit(window.app_handle().clone());
                }
                // 进入或退出全屏时窗口会改变尺寸，借此同步全屏标记。
                WindowEvent::Resized(_) => commands::publish_fullscreen(window.app_handle()),
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}
