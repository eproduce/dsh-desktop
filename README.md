# dsh-desktop

用 **Tauri 2** 重写 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) 的桌面外壳，替换掉原来的 Electron 实现。

Rust 核心负责窗口、`dsh-app` 协议、Host 子进程监管与桥接注入；Web 前端与 dsh Host 沿用上游实现，本仓库不复制它们的代码。

## 状态

**Phase 0（可行性实测）已完成，Phase 1（骨架）可编译可运行。**

已实测并据以定型的关键结论见 [迁移说明](docs/tauri-migration.md#已实测结论)：

| 结论 | 影响 |
| --- | --- |
| 自定义协议 origin 为 `dsh-app://localhost`（Windows 为 `http://dsh-app.localhost`） | 应用文档改由 Host 的环回 HTTP 源提供，反向代理与 cookie 转发整层不需要 |
| 初始化脚本先于页面脚本执行 | Electron 的 `contextBridge` 可用同名 shim 替代，客户端包不改动 |
| `Window::add_child` 多 webview 可用 | 侧边栏访客可行，但需 `unstable` 特性，且同 scheme 下不隔离 origin |

接下来要做的任务、验证方式与需要拍板的岔路口见 [路线图](docs/roadmap.md)。

还有一个上游接口事实影响范围判断：`DesktopBrowserBridge` 是壳无关接口，但**页面实现的挑选是硬编码的**——只要本外壳不暴露 `browser`，上游就回退到 sandboxed iframe provider，那个在任何 WebView 里都能跑。

## 运行

需要 dsh CLI 入口。从上游仓库构建后把入口路径指过来：

```sh
cd src-tauri
DSH_HOST_ENTRY=/path/to/deepseek-harness/packages/cli/lib/bin.js cargo run
```

未设置 `DSH_HOST_ENTRY` 时窗口仍会打开，加载页显示缺失原因——这是设计行为，不是崩溃。

### 环境变量

| 变量 | 默认值 | 用途 |
| --- | --- | --- |
| `DSH_HOST_NODE` | `node` | 运行 Host 的 Node 可执行文件 |
| `DSH_HOST_ENTRY` | 无 | dsh CLI 入口的绝对路径；缺失时不启动 Host |
| `DSH_HOST_ARGS` | 无 | 追加到入口之后的参数，按空白切分 |

## 结构

```
src-tauri/src/
  lib.rs        构建器装配、窗口创建、共享状态
  commands.rs   渲染层命令与 Host 启动编排
  host.rs       Host 子进程监管与上报解析
  protocol.rs   dsh-app 协议，只提供加载页
  bridge.rs     注入每个页面的初始化脚本
web/loading.html  Host 就绪前的加载页
```

Host 在 stdout 上按行上报 JSON，事件名沿用上游 Electron 外壳的 `ready` / `fatal` / `shutdown-complete`，因此 Host 侧不需要为换壳做改动。

## 尚未实现

- **拖放取路径**：Electron 的 `webUtils.getPathForFile` 没有等价物，需要改走 `tauri://drag-drop` 事件，会牵动输入框的 `@path` 引用逻辑。
- **原生目录选择**：`__DSH_DIRECTORY_PICKER__` 需要 `tauri-plugin-dialog`。
- **语言与主题同步**：`__DSH_LOCALE__`、`html[data-fullscreen]` 尚未接线。
- **更新链路**：`tauri-plugin-updater`、强制更新策略、COS 上传与签名流水线。
- **打包**：`bundle.active` 现为 `false`；产物格式、NSIS 安装器、PE 重签名、macOS 公证都待做。
- **浏览器访客与内嵌账户页**：多 webview 隔离方案待定，Accounts 页需要独立 webview 与凭据注入。

## 图标

`src-tauri/icons/icon.png` 取自上游客户端的 `resources/icon-macos.png`（1024×1024）。

注意上游的 `resources/icon.png` 是 1104×1104，而 Tauri 打包器按 1024 源图生成 `.icns` 与 `.ico`，迁移时要么重新导出为 1024，要么统一用平台 PNG。
