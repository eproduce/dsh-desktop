//! 注入到每个页面的初始化脚本。
//!
//! 脚本定义与既有 Electron preload 同名的全局对象，因此消费这些桥接的客户端
//! 代码不需要改动。Tauri 在页面自身脚本之前执行初始化脚本，已在本机实测。

/// 脚本中与平台无关的部分。
const BODY: &str = r#"
  const { core, event } = globalThis.__TAURI__;
  const invoke = (command, args) => core.invoke(command, args);
  const define = (name, value) => {
    Object.defineProperty(globalThis, name, { configurable: true, value });
  };

  // 共享 Web UI 的 CSS 依赖这个标记来启用桌面端样式。
  const markRoot = () => {
    document.documentElement.dataset.platform = PLATFORM;
  };
  if (document.documentElement) markRoot();
  else addEventListener("DOMContentLoaded", markRoot, { once: true });

  // 加载页与工作区文档都会调用它：加载页用它取 Host 地址，
  // 工作区文档用它取得启动注入数据。
  define("dshDesktopBoot", {
    ready: () => invoke("boot"),
    failed: (message) => invoke("boot_failed", { message }),
  });

  // 产品文档据此判断自己运行在桌面外壳中。
  define("dshDesktop", { protocolVersion: 1 });

  // macOS 进入全屏时红绿灯隐去，共享 Web UI 的 CSS 靠这个标记撤掉留白。
  if (PLATFORM === "macos") {
    event.listen("dsh://window-fullscreen", (message) => {
      const root = document.documentElement;
      if (!root) return;
      if (message.payload === true) root.dataset.fullscreen = "true";
      else delete root.dataset.fullscreen;
    });
  }

  // 原生目录选择：取消时 resolve 为 null，与上游约定一致。
  define("__DSH_DIRECTORY_PICKER__", {
    pick: () => invoke("pick_directory"),
  });
"#;

/// 生成注入脚本。
///
/// `platform` 取自 `std::env::consts::OS`，例如 `macos`、`windows`、`linux`。
pub fn initialization_script(platform: &str) -> String {
    format!("(() => {{\n  const PLATFORM = {platform:?};\n{BODY}}})();\n")
}
