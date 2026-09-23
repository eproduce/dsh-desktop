# 从 Electron 迁移到 Tauri 2

记录把 DeepSeek Harness 桌面端从 Electron 外壳换成 Tauri 2 外壳的范围、实测结论与阶段计划。

## 背景

上游的桌面端是一个 Electron 外壳：Electron 同时提供了四类能力——Chromium 渲染引擎与执行 Host 的 Node 运行时、带完整性封存的 ASAR 包格式、基于 `electron-updater` 与 electron-builder 的更新安装流水线、以及原生窗口/菜单/对话框/内嵌 webview API。外壳源码、打包脚本与装机资格测试都是围绕这四类能力写的。

换成 Tauri 2 后由平台 WebView 渲染，Electron 主进程由 Rust 核心取代。收益是安装包体积、常驻内存与冷启动；代价是外壳与发布流水线都要重写，因为 Electron 提供的大部分能力无法原样沿用。

已确认的硬约束：**Tauri 不捆绑 Node 运行时**。上游依赖的 `ELECTRON_RUN_AS_NODE` 方案消失，必须改为以 `externalBin` 提供真实的 `node` 二进制作为 sidecar，并把 dsh 生产依赖树作为捆绑资源（没有 ASAR，连带作废 ASAR 完整性封存与 Office 原生引擎的 `.asar.unpacked` 路径解析）。

## 已实测结论

用一次性 Tauri 2.11.5 应用在 macOS 上构建并运行取得，全程离线（本机 Cargo 缓存）。

```
SETUP webviews_in_window=2
PROTOCOL_REQUEST uri=dsh-app://localhost/guest method=GET
PROBE_REPORT from=guest {"origin":"dsh-app://localhost","href":"dsh-app://localhost/guest","initScriptRanBeforePageScripts":true}
PROBE_REPORT from=main  {"origin":"dsh-app://localhost","hostname":"localhost","protocol":"dsh-app:"}
```

- **文档 origin 是 `dsh-app://localhost`**，主机名与协议名由 Tauri 推导，应用无法自行指定，因此上游的 `dsh-app://app` 不可达。Windows 为 `http://dsh-app.localhost`（`useHttpsScheme` 可切 https）。所有起源比较都要改写。
- **初始化脚本先于页面脚本执行**，因此在那里定义的桥接对应用代码可见。上游 16 个 preload 用 `contextBridge` 暴露的全局对象可以用同名 shim 等价提供，消费这些桥接的客户端代码不需要改动——这是本次迁移最大的降本点。
- **多 webview 可用**：`Window::add_child` 能在同一窗口创建第二个 webview，带独立的标签、URL 与初始化脚本，两个页面都能加载并应答命令。两个代价：桌面端需要 `tauri` 的 `unstable` Cargo 特性；**同一协议下的多个 webview 共享 origin**，所以侧边栏访客无法靠 origin 隔离，必须另找机制。
- **整个 Tauri 依赖树可离线编译**，本机具备持续验证条件。

尚未实测（都需要交互式 GUI 操作）：拖放路径来源、自绘装饰下的菜单行为、麦克风授权路径。

## 职责映射

| 职责 | 上游 Electron 归属 | Tauri 2 替代方案 | 状态 |
| --- | --- | --- | --- |
| Host 进程监管 | `host-process.ts` | 由 Rust 启动 sidecar，保留 JSON 控制通道 | 已实现 |
| 应用文档 | `web-document.ts` 中的 `dsh-app://app` | Host 的环回 HTTP 源，窗口就绪后导航 | 已实现 |
| Host 就绪前的加载页 | `dsh-app://app` 静态资源 | `dsh-app` 自定义协议 | 已实现 |
| 渲染层桥接 | 16 个 `preload-*.ts` | 初始化脚本 + `invoke` + 事件 | 部分实现 |
| 桌面标记 `dshDesktop` | `preload-app.ts` | **有意不定义**，见下 | 有意分歧 |
| 拖入文件的路径 | `webUtils.getPathForFile` | `tauri://drag-drop` 事件 | 待验证 |
| 目录选择 | Electron `dialog` | `tauri-plugin-dialog` | 已实现 |
| 单实例与 `dsh://open` | `single-instance.ts` | `tauri-plugin-single-instance`、`tauri-plugin-deep-link` | 未开始 |
| 菜单与 Windows 标题栏 | `preload-menu.ts`、`windows-layout.ts` | `tauri::menu` 与自绘装饰 | 未开始 |
| 内嵌 Platform 账户页 | `WebContentsView` | 子 webview + 导航处理器 + 凭据注入 | 未开始 |
| 侧边栏浏览器访客 | `<webview>` | 多 webview（需隔离方案） | 未开始 |
| 麦克风授权 | Electron 会话权限处理器 | 平台授权与 webview 采集提示 | 待验证 |
| 更新检查与安装 | `electron-updater` | `tauri-plugin-updater` | 未开始 |
| 安装器与签名 | electron-builder、NSIS、PE 重签名 | Tauri 打包器 + NSIS 钩子 + 保留的签名步骤 | 未开始 |
| 捆绑 Node、pnpm 与 Python | `scripts/primary-runtime/*` | sidecar 二进制与捆绑资源 | 未开始 |

应用文档改走 Host 环回 HTTP 源，让上游的整层反向代理、cookie 交换与逐请求 `origin` 校验都变成不需要的代码。加载页仍单独由自定义协议提供，保留「Host 未就绪时窗口已可见」的体验。

上游 Host 上报的事件集比上表体现的更宽（`apps/desktop/src/host-process.ts`）。除 `ready`、`fatal`、`shutdown-complete` 外，还有 `platform-session`（账号平台窗口的凭证）与 `update-tasks`（更新调度）；这两个本外壳尚未实现，会把它们记成 stderr 诊断而不是无声丢弃。前者归上表的「内嵌 Platform 账户页」，后者归「更新检查与安装」。

### 为何不定义 `dshDesktop`

上游把这个标记当作「原生壳自己承接凭据设置」的声明，它同时控制两处：`ui-settings-models` 用它抑制 Web 侧的模型凭据引导（`credentialOnboarding && !('dshDesktop' in globalThis)`），`ui-settings-account` 则只在它存在时才注册账号 UI。Electron 能这样做，是因为它有欢迎窗口负责凭据设置、且桌面登录页所需的 `dshPlatform` 桥接也在那里。

本外壳两者都还没有。若照样定义该标记，用户会既看不到 API key 入口、又走不通登录——实测就是这个后果。因此当前有意不定义它，代价是暂时失去账号 UI（一个在没有 `dshPlatform` 时本就无法完成登录的界面）。该标记的全部消费点已核对：另外两处是 `ui-settings-general` 的更新桥接（本外壳不提供）与 `ui-sidebar-browser` 的 `carrier.browser`（本外壳不提供，因此 iframe 分支不受影响）。等欢迎窗口或 `dshPlatform` 落地后应恢复该标记。

### 凭据与模型范围

**模型凭据不弹引导，统一在设置页配置。** 上游的 `credentialOnboarding` 默认为 `true`，会在模型页弹出一次凭据引导；Electron 靠 `dshDesktop` 标记抑制它，因为那条路由由欢迎窗口承接。本外壳没有欢迎窗口，因此按上游为该类原生壳准备的开关，在 profile 补丁层写入 `ui-settings-models` 的 `credentialOnboarding: false`。设置页的模型与 API 配置不受影响，只是不再弹引导。该条目由 `src/profile.rs` 幂等追加：用户已有条目与注释一律保留，用户若自行写一条同 id 的覆盖（放在其后）即可改回。

不需要登录就能使用：凭据按三层解析，优先级为**继承的进程环境变量（只读，最高）→ 调用目录的 `.env` → Harness 根目录的 `.env`**（`packages/util/launch-environment`）。因此 `DEEPSEEK_API_KEY=… ` 既可以直接放进环境变量，也可以写进 `~/.dsh/.env` 与 CLI 共用。有凭据后 `needsWelcome` 判定为不需要登录。

模型也不限于 DeepSeek：`llm-pi-ai` 在多提供方插件里是**休眠挂载**的——在 `llm-pi-ai:` 设置段提供 provider profile 之前不注册任何路由，提供后这些路由会即时注册、密钥按其 `apiKeyEnv` 引用逐请求解析。上游注释指明「提供这些 profile 正是 Web 模型页所做的事」。

## 替代方案

**保留 Electron 外壳。** Electron 已满足上表每一项，而要替换的四类能力恰恰承担了最多的资格验证工作：经过签名的 Windows 目录替换、公证、嵌入式 webview 会话以及装机更新测试集。当安装包体积、内存与冷启动的实测收益不足以支撑数月重写一条目前能过企业代码完整性策略的发布流水线时，这个方案胜出。

**连 Host 运行时一起迁移到 Rust。** 完全去掉 Node 可以进一步缩小载荷。它失败的原因是 Host 是共享的 CLI profile 运行器，带有 Cordis 插件图、由 pnpm 管理的插件安装路径与捆绑的 Python 载荷；重写它比换壳更大，属于另一个项目，且捆绑运行时的规则仍归上游决策负责。

**使用系统或用户安装的 Node.js。** 这样可以省去 sidecar 二进制。它失败的原因是上游要求应用在没有系统 Node.js 或 pnpm 安装的情况下运行，而用户安装的 Node 会把未经测试的版本组合带进一个随包发布单一确定运行时的产品中。

**在长期分支上就地替换。** 这样可以避免同时维护两套外壳。它失败的原因是迁移跨越数月，而每一个中间提交都会让上游唯一可发布的桌面实现处于不可用状态。

## 阶段计划

| 阶段 | 内容 | 出口判据 |
| --- | --- | --- |
| P0 实测 | 上述四个未知项 | 每项有可运行或实测的结论 |
| P1 骨架 | 本仓库当前进度 | 能启动、能连上真实 Host 进入会话 |
| P2 原生功能 | 菜单、Windows 标题栏、内嵌账户页、浏览器访客、麦克风、主题/全屏同步、DevTools | 与上游功能对照清单逐项通过 |
| P3 打包签名 | 去 ASAR 的资源布局、NSIS 安装器、PE 重签名、macOS 公证、完整性校验 | 产物可装机并通过上游的载荷与 Host 冒烟检查 |
| P4 更新链路 | `tauri-plugin-updater`、强制更新策略、清单与上传 | 完成一次端到端升级，并拒绝未签名产物与降级 |
| P5 收尾 | 删除上游 Electron 实现并同步门禁 | 上游文档门禁通过 |

## 验收标准

- 能在 Rust 核心与 Node sidecar 之上启动窗口并进入可用的会话，且上游 Web 前端保持未改动。
- 职责映射表中每一处桥接都已定义并通过 Tauri 命令接口应答，使用这些桥接的上游录制会话测试、客户端测试与 Host 测试全部通过。
- 打包后的 macOS 与 Windows 构建能安装、在没有系统 Node.js 或 pnpm 的情况下启动，并通过上游的载荷与 Host 冒烟检查。
- 更新器能安装后继构建，并同时拒绝未签名产物与降级。
- 实测记录列出 Tauri 在 macOS 与 Windows 上产生的确切 origin、侧边栏访客的多 webview 稳定性、拖放路径来源、自绘装饰下的菜单行为以及麦克风授权路径，每一项都带实测或观察到的结果。

## 风险

**侧边栏浏览器访客风险最高。** 上游依赖 `<webview>` 分区与主进程签发的租约，而 Tauri 多 webview 需要 `unstable` 特性且同协议下不隔离 origin。如果子 webview 无法满足该隔离要求，这一能力需要重新设计，或者放弃本次迁移。

**Windows 发布路径次之。** 上游安装器通过同卷重命名替换目录并支持回滚，签名环节遍历运行时中的每个 PE 文件。Tauri 的 NSIS 钩子覆盖安装器生命周期，但不覆盖目录提升逻辑，两者都是承担发布资格验证重量的新代码。

**去掉 ASAR 会失去一项完整性保证。** 上游把文件字节封存进归档并校验成员；普通资源目录需要等效的清单校验，在此之前对运行时树的本地改动不会被发现。

**sidecar 改变了进程身份。** 由 Rust 父进程启动的 `node` 二进制在 Gatekeeper 与公证视角下是独立的已签名可执行文件，macOS 上需要当前 Electron 运行时已声明的 JIT 权限。

**图标尺寸不一致。** 上游 `resources/icon.png` 是 1104×1104，而 Tauri 打包器按 1024 源图生成 `.icns` 与 `.ico`；`icon-macos.png` 与 `icon-windows.png` 才是 1024×1024。
