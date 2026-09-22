//! `dsh-app` URI 协议。
//!
//! 它只提供加载页：窗口在 Host 就绪前显示它，因此启动失败时用户看到的是状态
//! 而不是空白页。应用文档不走这里——Host 就绪后窗口直接导航到它的环回 HTTP
//! 源，因此不需要反向代理，也不需要在 Node 侧重新实现认证 cookie 转发。

use tauri::{http, Runtime};

/// 外壳自定义协议的名称。
pub const SCHEME: &str = "dsh-app";

/// 加载页内容。
const LOADING_PAGE: &str = include_str!("../../web/loading.html");

/// 窗口首次载入的地址。
///
/// Tauri 由协议名与平台推导主机名：macOS 与 Linux 是 `<scheme>://localhost/`，
/// Windows 是 `http://<scheme>.localhost/`。应用无法自行指定主机名。
pub fn loading_url() -> Result<tauri::Url, String> {
    #[cfg(windows)]
    const BASE: &str = "http://dsh-app.localhost/";
    #[cfg(not(windows))]
    const BASE: &str = "dsh-app://localhost/";

    BASE.parse()
        .map_err(|error| format!("加载页地址无效：{error}"))
}

/// 在构建器上注册 `dsh-app` 协议。
pub fn register<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.register_uri_scheme_protocol(SCHEME, |_context, request| match request.uri().path() {
        "/" | "/index.html" | "/loading.html" => http::Response::builder()
            .header(http::header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(LOADING_PAGE.as_bytes().to_vec())
            .expect("加载页响应可构造"),
        _ => http::Response::builder()
            .status(http::StatusCode::NOT_FOUND)
            .body(Vec::new())
            .expect("空 404 响应可构造"),
    })
}
