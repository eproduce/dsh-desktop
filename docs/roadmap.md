# 路线图

从当前骨架推进到可发布的 Tauri 2 桌面端。阶段划分与出口判据见 [迁移说明](tauri-migration.md)；本文只列可执行任务、验证方式与需要拍板的岔路口。

## 起点

Phase 0 实测完成，Phase 1 骨架可编译可运行：窗口能开、加载页能渲染状态、Host 子进程能被监管、桥接能被注入。

## 上游接口现状

查上游源码得到三条决定计划形状的事实：

- `DesktopBrowserBridge`（`packages/client/ui-sidebar-browser/src/types.ts`）是**壳无关**接口，只有 `acquire(workspace)`、`release(lease)`、`onOpenRequested(lease, listener)`，没有 Electron 对象穿过它。它可以从 Tauri 侧实现。
- 但**页面实现的挑选是硬编码的**：`src/client/index.ts` 里 `desktop === undefined ? createIframePage : createElectronPage`，而 `createElectronPage` 走 Electron 的 `<webview>` 元素。
- 因此：Tauri shim **只要不暴露 `browser`**，上游就回退到 `createIframePage`。那个 provider 是 sandboxed iframe，在任何 WebView 里都能跑。

同一模式也适用于其它桥接：`__DSH_DIRECTORY_PICKER__`、`dshDesktop.updates`、`__DSH_HOST_PATHS__` 都是**可选全局**，上游在缺失时会降级。这意味着一部分功能可以零上游改动拿到，另一部分必须改上游。

## P1 进度

已完成并有实测证据：

- **Host 启动契约**：上游 Host 用 Node 的 IPC 通道上报（`stdio` 末项为 `ipc`），Rust 没有该通道，因此新增 `host/host-bridge.cjs` 做转换：它与 Host 走 IPC，把消息以换行分隔 JSON 写到 stdout，并把 stdin 转回 Host。Host 的 stdout/stderr 都转到桥接的 stderr 作为诊断。
- **启动参数**：镜像上游的 argv 位置（`[2]=runtimeDir`、`[3]=projectDir`、`[4]=primaryRuntime`），端到端验证时页面回读到的三个参数与传入一致。
- **事件与状态机**：`ready`（含 injections）、`fatal`、`shutdown-complete`、新增的 `exit`；失败按端口占用分类并保留诊断尾巴。
- **导航与注入**：Host 就绪后窗口导航到它给出的地址，`boot` 返回 Host 上报的 `streamBaseUrl` 与 `injections`。实测拿到假 Host 的注入数据 `[{"marker":"fake-host"}]`。
- **桥接在异地源可用**：工作区文档运行在 Host 的环回 HTTP 源上，桥接对象与平台标记在那里同样生效。
- **测试台**：`tests/fake-host.mjs` 按上游协议提供最小 Host，并把工作区文档的探测结果回传到 `GET /last-report`，验证不依赖窗口焦点或截图。

未完成：窗口关闭时的优雅收尾已实现，但自动化验证受 macOS 辅助功能权限限制无法完成，需要手工关窗确认进程链被清理。

### 命令许可（已解决）

工作区文档调用外壳命令曾被 Tauri 的 ACL 拒绝，报 `boot not allowed. Plugin not found`。用最小复现工程跑完整矩阵后确认机制本身正确——**文档的源类别必须与能力的上下文匹配**：

| 能力声明的上下文 | `dsh-app://localhost`（加载页） | `http://127.0.0.1:PORT`（工作区） |
| --- | --- | --- |
| 无能力文件但有 `permissions/*.toml` | 拒绝 | 拒绝 |
| 仅 `local: true` | 放行 | 拒绝 |
| 仅 `remote: {urls: [...]}` | 拒绝 | 放行 |
| `local: true` + `remote` | 放行 | 放行 |

另外两点由矩阵确认：只要存在 `permissions/*.toml`，应用 ACL 清单就会被启用，**即使一个能力文件都没有**，自定义命令也需要显式许可；非本地来源无论是否有清单都受 ACL 约束。

**根因是构建产物陈旧。** 新增 `permissions/*.toml` 或改动 `capabilities/*.json` 之后，普通 `cargo build` 没有让 `tauri-build` 重新生成并嵌入 ACL，运行中的二进制带着旧清单，因此运行期查不到应用许可键，报出的正是 `Plugin not found` 这一支。删构建目录不足以触发重新生成。

**纪律**：这三类文件（`capabilities/`、`permissions/`、`tauri.conf.json` 的安全段）改动之后，先 `cargo clean -p dsh-desktop` 再构建，否则会在错误方向上排查很久。

## 岔路口

这四处决定后面几个月的工作量，建议先定：

| # | 问题 | 选项 |
| --- | --- | --- |
| D1 | 侧边栏浏览器怎么做 | **A**：shim 不暴露 `browser`，用 iframe provider，零上游改动，代价是部分站点拒绝被 frame、没有独立存储分区<br>**B**：实现 bridge + 上游加 provider 注册点，得到原生子 webview 与真实分区，代价是需要上游 PR |
| D2 | 是否接受改上游 | 若接受：本仓库 + 上游 PR 两条线并行。若不接受：功能范围受限于上游现有的可选桥接，侧边栏浏览器只能走 A，拖放路径只能靠顺序相关性 |
| D3 | 拖放取 `@path` | **A**：用 `tauri://drag-drop` 事件与 DOM drop 的顺序相关性配对，脆弱但不改上游<br>**B**：上游扩展 `__DSH_HOST_PATHS__` 接受路径数组 |
| D4 | 发布凭据 | Windows 代码签名证书与 macOS 公证凭据由谁提供、放在哪 |

## P1 — 让窗口真正进入会话

目标：从「窗口能开」到「能看到自己的会话列表」。

- [ ] **1.1 Host 启动配置**：把 `DSH_HOST_ENTRY`/`DSH_HOST_ARGS` 扩成读一个配置文件（profile 名、`DSH_HOME`、端口），与上游 Electron 外壳的启动参数对齐；参考上游 `apps/desktop/src/main.ts` 里传给子进程的实参。
- [ ] **1.2 启动数据注入**：上游的 `dshDesktopBoot.ready()` 返回 Host 提供的 boot injections。现在返回空数组，需要让 Host 把 injections 交给外壳（上游 Host 已经在 `ready` 事件里带 `injections` 字段，读出来即可）。
- [ ] **1.3 失败分支**：区分「未配置」「端口占用（`listen EADDRINUSE`）」「插件加载失败」三类，加载页各自给出可操作文案；上游 Electron 外壳对这三类有成熟文案，照抄语义。
- [ ] **1.4 退出与重启**：窗口关闭时向 Host 发关闭请求并等待 `shutdown-complete`，超时则强杀；`host_restart` 已经存在，补上等待旧进程真正退出的时序。
- [ ] **1.5 开发体验**：加 `pnpm`/`npm` 脚本跑 `tauri dev`；`rust-toolchain.toml` 与 `.nvmrc` 对齐你其它仓库。

**验证**：设置真实 Host 入口并在窗口里看到会话列表；关闭窗口后 `pgrep node` 不再有残留 Host 进程。

## P2 — 补全可选桥接

目标：把上游在桌面壳里期待的可选全局补齐到可用状态。

- [ ] **2.1 `__DSH_DIRECTORY_PICKER__`**：接 `tauri-plugin-dialog` 的原生目录选择，返回值与上游 `NativeFlowInjected` 的 `pick()` 对齐。
- [ ] **2.2 `__DSH_LOCALE__`**：接 `tauri-plugin-os` 取系统语言，实现 `read()` 与 `onChange()`；同时把语言变化同步到窗口，供原生菜单使用。
- [ ] **2.3 `html[data-fullscreen]`**：监听窗口全屏变化并打标记，供上游 CSS 撤掉 macOS 红绿灯留白。
- [ ] **2.4 `dshDesktop.updates`**：先只做 `status()` 与 `subscribe()`，返回空闲状态；真正的更新器在 P6。
- [ ] **2.5 平台请求头核对**：确认 Host 发出的 `x-client-platform` 在 Tauri 下取值正确（`desktop-mac`/`desktop-win`），这是上游账号与更新策略的分支依据。

**验证**：逐个桥接写单元测试（Rust 侧的命令返回结构 + shim 暴露的成员名），再跑上游使用这些桥接的客户端测试。

## P3 — 侧边栏浏览器（取决于 D1）

**若选 A（iframe）**：不做代码改动，只做验证——确认 `desktop === undefined` 分支真的被走到，并实测一批常见站点能否被 frame。

**若选 B（原生子 webview）**：工作量最大的一个阶段。

- [ ] **3.1** 实现 `DesktopBrowserBridge`：`acquire` 返回 `{lease, partition}`，`release` 销毁 guest，`onOpenRequested` 转发新窗口请求。租约管理与上游 `browser-guests.ts` 的语义一致（主进程签发、匹配租约与分区）。
- [ ] **3.2** 用 `Window::add_child` 创建 guest webview，位置与尺寸跟随渲染层报告的占位区域。
- [ ] **3.3** 隔离：guest 的 webview 标签**不匹配任何 capability**，从而拿不到 Tauri IPC；存储分区用 `dataStoreIdentifier`（macOS 14+）/ `dataDirectory`（Windows、Linux）。
- [ ] **3.4** 上游加 provider 注册点：把 `desktop === undefined ? iframe : electron` 换成按壳类型查表，让 Tauri provider 能注册进来。这是需要上游 PR 的一步。
- [ ] **3.5** 导航白名单：上游 Electron 外壳只放行 HTTP(S) 且拒绝 URL 内嵌凭据，新 provider 保持同样规则。

**验证**：打开真实 HTTPS 站点、站内跳转、多标签、退出后 guest 被销毁；确认 guest 无法调用任何 Tauri 命令。

## P4 — 拖放取路径（取决于 D3）

- [ ] **4.1 探针**：写一个最小页面，同时接 `tauri://drag-drop` 与 DOM `drop`，拖入多个文件，记录两者的顺序与数量是否稳定对应。
- [ ] **4.2** 按探针结论实现 `__DSH_HOST_PATHS__.pathFor`，或按 D3-B 提上游扩展。
- [ ] **4.3** 回归上游 `packages/client/ui-conversation` 里关于 `@path` chip 的测试。

**验证**：拖入文件、文件夹、图片各一次，确认「带真实路径的非图片文件成为 `@path` chip、图片仍上传」这一上游行为不变。

## P5 — 打包与签名

- [ ] **5.1 图标**：`tauri icon` 生成 `.icns`/`.ico` 全套。注意上游 `resources/icon.png` 是 1104×1104，Tauri 按 1024 源图生成，需重新导出或用平台 PNG。
- [ ] **5.2 资源布局**：`bundle.active = true`；dsh 生产依赖树作为 `resources` 落盘（没有 ASAR），确认原生模块与 Office 原生引擎能从普通目录加载。
- [ ] **5.3 运行时完整性**：用清单校验替代 ASAR 封存，至少覆盖 dsh 运行时树的文件集合与哈希。
- [ ] **5.4 Windows**：NSIS 安装器 + 目录替换升级 + PE 重签名。上游这套在 electron-builder 里，需要在新项目重建。
- [ ] **5.5 macOS**：公证、`hardenedRuntime`、sidecar `node` 的 JIT entitlement。

**验证**：产物在干净机器上安装并启动，无系统 Node/pnpm；通过上游的载荷与 Host 冒烟检查。

## P6 — 更新链路

- [ ] **6.1** `tauri-plugin-updater` 接线，静态 JSON 或动态服务的取舍。
- [ ] **6.2** 强制更新策略：上游的策略状态机与 `dshMandatoryUpdatePolicy` 元数据语义，落到新产物格式上。
- [ ] **6.3** 上传与清单发布；若沿用 COS，重写上传脚本。

**验证**：完成一次端到端升级；拒绝未签名产物；拒绝降级。

## P7 — 与上游功能对齐

- [ ] **7.1** 按迁移说明的职责映射表逐项对照，确认没有能力遗漏。
- [ ] **7.2** 决定两份实现的去向：上游是否接受 Tauri 外壳成为受支持目标，还是本仓库长期作为独立分支跟随上游。

**验证**：对照表全部通过。

## 风险

**上游契约会漂移。** 本仓库依赖上游的可选全局与 `DesktopBrowserBridge`；上游是 pre-stable 仓库，这些接口会变。P7 的 7.2 需要在早期就定，否则每次上游改动都可能让这里失效。

**P5 的 Windows 发布路径最重。** 目录替换升级与 PE 重签名都是承担发布资格验证的新代码，没有可复用的上游实现。

**P2 的 `dshDesktop.updates` 存根有误导风险。** 返回空闲状态但不真正检查更新，会让用户以为已是最新。建议在真正接上更新器之前，宁可让上游走「无桌面更新桥接」的降级分支。
