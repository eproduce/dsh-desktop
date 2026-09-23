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

  // 这里刻意不定义 `dshDesktop`。上游用这个标记表示「原生壳自己承接凭据设置」：它的
  // 存在会抑制 Web 侧的模型凭据引导（`ui-settings-models` 里判定为
  // `credentialOnboarding && !('dshDesktop' in globalThis)`），同时启用仅桌面注册的
  // 账号 UI（`ui-settings-account` 缺该标记即整体返回）。Electron 之所以能这样，是因为
  // 它有欢迎窗口负责凭据设置、且桌面登录页所需的 `dshPlatform` 桥接也在那里。本外壳
  // 两者都还没有，若照样定义该标记，用户就会既看不到 API key 入口、又走不通登录。等
  // 欢迎窗口或 `dshPlatform` 落地后，这里应恢复该标记以与上游桌面行为对齐。

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

  // Host 用「带 token 的地址换取 cookie，再重定向」完成页面认证，而那个 cookie 是
  // SameSite=Strict。本窗口的第一份文档来自 dsh-app 加载页，属于跨站发起，WebKit
  // 因此在重定向后的请求上不回送该 cookie，窗口会停在 Host 的 401 纯文本页上。
  // 该页与 Host 同源，从它再发一次同源导航，cookie 就会被带上。
  const AUTH_FAILURE = "dsh web authentication required";
  const recoverFromAuthFailure = () => {
    const body = document.body;
    if (body === null) return;
    if (!(body.textContent ?? "").includes(AUTH_FAILURE)) return;
    location.replace(location.origin + "/");
  };
  if (document.contentType.startsWith("text/plain")) {
    if (document.readyState === "loading") {
      addEventListener("DOMContentLoaded", recoverFromAuthFailure, { once: true });
    } else {
      recoverFromAuthFailure();
    }
  }
"#;

/// 生成注入脚本。
///
/// `platform` 取自 `std::env::consts::OS`，例如 `macos`、`windows`、`linux`。
pub fn initialization_script(platform: &str) -> String {
    format!("(() => {{\n  const PLATFORM = {platform:?};\n{BODY}}})();\n")
}
