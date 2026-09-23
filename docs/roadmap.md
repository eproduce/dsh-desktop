# 路线图

从当前骨架推进到可发布的 Tauri 2 桌面端。阶段划分与出口判据见 [迁移说明](tauri-migration.md)；本文只列可执行任务、验证方式与需要拍板的岔路口。

## 方向复盘（2026-09-23）

Phase 0 到 P4 做完后回看，本项目的产出是**外壳层**：进程监督与事件状态机、导航与认证自愈、目录选择器、全屏标记、profile 引导、关闭 Tauri 默认拖放处理器。产品本体（对话交互、子任务、插件系统、会话日志、模型接入）全部来自上游，本项目没有实现其中任何一项。

所以它的价值分两种，必须区分清楚：

- **想自用**：复用上游整个产品再换壳，是拿到一个成熟 agent 桌面端的最便宜路径，做完 P5 打包即可。价值真实，但属于「用产品」。
- **想借它的架构**：外壳帮不上忙。经机制分析后认为这才是更值得投入的方向，产出见 [可借鉴机制分析](harness-mechanisms.md)。

代价是持续的：本轮修掉的缺陷多数是在迁就**上游为 Electron 写下的假设**（注入表被应用两次、`dshDesktop` 全局、cookie 的 `SameSite=Strict`、拖放处理器认领拖放）。上游没有按「可替换外壳」设计过这条边界，所以换壳本质上是在逆着它做——上游任何一次改动都可能让我们坏。

**当前决定**：外壳冻结在「可运行、已验收」状态，不再新增功能。代码与本文档全部保留，随时可回来做 P5。精力转向机制分析。

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
- **优雅收尾**：关窗后外壳、桥接、Host 三个进程全部退出。测试台记录到的握手是「收到 shutdown → 已发送 shutdown-complete → IPC 通道断开」，说明走的是约定流程而不是被强杀。
- **测试台**：`tests/fake-host.mjs` 按上游协议提供最小 Host，并把工作区文档的探测结果回传到 `GET /last-report`，验证不依赖窗口焦点或截图。
- **开发体验**：`rust-toolchain.toml` 固定 1.98.0（含 rustfmt/clippy），`.nvmrc` 与 `.editorconfig` 对齐 Node 24 与既有仓库约定；`scripts/e2e.sh` 把上面整条链路做成可重复的检查，`npm run e2e` 一条命令跑完构建、启动、探测、关窗与收尾断言。构建、检查与端到端都不需要 npm 依赖；只有打包与图标生成需要 `@tauri-apps/cli`（届时需联网 `npm install`）。

- **数据根与上游一致**：profile 目录原先落在 Tauri 的应用数据目录下，与上游的 `~/.dsh/profiles/desktop` 不是同一个根，换壳后用户已有的会话与设置会「消失」。现已按上游 `resolveDshHome` 的优先级解析：`DSH_HOME` 优先（空白视为未设置，支持 `~`），否则 `~/.dsh`，结果规范化为绝对路径；profile 取 `<根>/profiles/desktop`，与 `resolveDesktopPaths` 一致。端到端脚本把 `DSH_HOME` 指向临时目录，既保证验证不碰真实数据，又反过来断言外壳没有另开数据根。

关窗验证可以自动化：`osascript` 经 `System Events` 对窗口执行 `AXPress of button 1` 即可按下关闭按钮。按窗口名引用（而不是 `window 1`）才可靠。GUI 自动化的重试预算必须留足余量：机器有重负载（例如同时跑整仓 `tsc`）时 AX 操作会明显变慢，预算过紧会表现为「窗口切不动 / 关不掉」而不是断言失败。

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

**已从根上修复。** `src-tauri/build.rs` 现在把 `capabilities/` 与 `permissions/` 声明为构建脚本输入，cargo 会在它们变化时重跑构建脚本并重新嵌入 ACL。实测：只改权限文件、不碰其他文件，构建后清单自动跟上。此前「改了却像没改」的现象不会再出现。

## P2 进度

上游的桌面桥接大多是**可选全局**，缺失时上游会降级，因此可以从容地逐个补齐。

- **2.1 原生目录选择**：接入 `tauri-plugin-dialog`，新增 `pick_directory` 命令，并在桥接里以 `__DSH_DIRECTORY_PICKER__` 暴露，签名与上游 `NativeFlowInjected` 的 `pick(): Promise<string | null>` 一致。选择器挂到主窗口。dialog 插件只在 Rust 侧调用，前端不直接访问插件命令，**因此不需要给前端授予任何 dialog 权限**。
- **2.3 全屏标记**：窗口尺寸变化时比较全屏状态，仅在变化时广播 `dsh://window-fullscreen`；桥接在 macOS 上据此维护 `html[data-fullscreen]`，对应上游 `preload-platform.ts` 的行为。端到端脚本会真实切换全屏，并分别断言标记被设置与被清除，两个方向都验证过。切换必须**读回确认**：macOS 的全屏是动画，动画未结束时再次切换会被忽略，只判断「设置动作已发出」会在动画窗口期误判为已生效。
- **2.2 语言**：**外壳不需要做任何事**，与最初的计划相反。上游 `packages/client/locale` 在没有 `__DSH_LOCALE__` 时会退回 `detectBrowserLocale` 读取 `navigator.languages`；本机实测 WebView 报 `['zh-CN']`，与 macOS 的 `AppleLanguages`（`zh-Hans-CN`）指向同一语言。

  原先担心打包后 WebKit 会按 bundle 的 `CFBundleLocalizations` 过滤该列表，因此把开发二进制包成 `.app` 做了对照实验：声明 `en` 与声明 `en zh` 两种 bundle 都仍然报 `zh-CN`，**该过滤不存在**。存储的偏好也不需要外壳代劳：`LocaleRuntime` 的构造函数里就调用 `adopt(host)`，直接从 Host 设置投影读取 `locale.preference`，而 `bootstrap.preference` 只是它读取之前的一份临时值。

  因此不引入 `tauri-plugin-os`：它只能给出单个 locale，比 `navigator.languages` 的有序列表更弱；引入后语言会有两个来源，还可能互相不一致。`onChange` 同理跳过——上游用它刷新原生菜单与平台窗口的文案，而本外壳目前没有任何随语言变化的原生界面。

  残留未知：本机只有一种系统语言，多语言时的**顺序**语义没有实测。若日后收到「系统语言顺序未生效」的报告，再补 `__DSH_LOCALE__`。
- **2.4 更新桥接**：**有意不做**。返回「空闲」的存根会让用户误以为已是最新，比暂时缺失更糟；让上游走「无桌面更新桥接」的降级分支是更安全的状态。真正接上更新器属于 P6。
- **2.5 平台请求头**：**无需外壳做任何事**。`x-client-platform` 由 Host 侧从 `process.platform` 推导（`packages/bundle/base/cordis.patch.yml`），属于 profile 逻辑，外壳只负责用桌面 profile 启动 Host。

## 岔路口

这四处决定后面几个月的工作量。已定两条：

| # | 问题 | 决定 |
| --- | --- | --- |
| D1 | 侧边栏浏览器怎么做 | **已定 A（知情决定）**：看过可框性数据后仍选择 iframe 路线，接受抽样 10 个站点里 6 个打不开，换取零上游改动。理由见 P3-A 实测结论；转 B 需要上游加 provider 注册点（即 D2）。 |
| D2 | 是否接受改上游 | **已定：能不改就不改**。零上游改动的路径优先；确实必须改上游时单独提出再定。 |
| D3 | 拖放取 `@path` | **已定：先关掉 Tauri 默认拖放处理器**。实测 Tauri 默认配置下页面收不到任何 DOM 拖放事件，拖放整体失效；关掉后恢复，但外壳拿不到磁盘路径，`@path` 芯片要等上游扩展（原 B）。原 A 经查不可行，理由见 P4。 |
| D4 | 发布凭据 | **待定**。Windows 代码签名证书与 macOS 公证凭据由谁提供、放在哪。 |

## P1 — 让窗口真正进入会话

目标：从「窗口能开」到「能看到自己的会话列表」。

### 真实 Host 验收（进行中）

上游的官方 registry 连不通但国内镜像可达，因此依赖走镜像安装：`npm_config_registry=https://registry.npmmirror.com corepack pnpm install --frozen-lockfile`（lockfile 未内嵌 registry 地址，换源不会改写它，实测安装后上游仓库 `git status` 干净）。随后 `pnpm run build` 构建产物。

运行时树不需要启动 Electron 就能准备：直接调用上游自己的 `prepareDevelopmentProject`。准备过程会在 pnpm 的虚拟提升目录里撞上**悬空的平台可选依赖链接**（`@anthropic-ai/claude-agent-sdk-darwin-x64`、`@openai/codex-darwin-x64`、`@deepseek-ai/libreoffice-kit-darwin-x64`），删掉这三个坏链接即可继续；它们是 pnpm 为整个 lockfile 闭包建链接、而被跳过的可选依赖没有落地造成的。

本机工具链实测为 **x86_64**（`uname -m` 与 `process.arch` 均为 x64，`node` 是 x86_64 Mach-O），因此上游的打包目标解析结果是 `mac-x64` 而不是 `mac-arm64`。

接真实 Host 的命令：

```
DSH_HOME=<空目录> \
DSH_HOST_ENTRY=<runtimeDir>/node_modules/@deepseek-ai/dsh-desktop-host/lib/index.js \
DSH_HOST_RUNTIME=<runtimeDir> \
  dsh-desktop
```

其中 `<runtimeDir>` 是 `apps/desktop/.desktop-build/development/project`。

已依次清除的阻塞：

1. **profile 目录必须由外壳创建**。`apps/desktop-host` 直接调用 `loadProfileDirectory`，绕过了 `loadProfile` 里按名称查内置模板的兜底，而 `PROFILE_TEMPLATES` 里没有 `desktop`；清单不存在时 Host 以 `failed to read profile manifest` 退出。已按上游 `initProfile` 与 `createPluginProfile` 实现，见 `src-tauri/src/profile.rs`。
2. **致命错误被退出码覆盖**。Host 报 `fatal` 后紧接着退出，状态被降级成「意外退出」，把指明下一步的致命信息换成了无意义的退出码。现在保留致命信息，同时换上退出时刻更完整的 stderr（报 `fatal` 时进程常常还没输出堆栈）。

越过后 Host 能启动 Web 服务并打印 `dsh web: http://127.0.0.1:19387/?token=...`，外壳、桥接、Host 三个进程同时存活。补上载荷后外壳收到 Host 的 `ready`，并把窗口导航到带 token 的真实工作区地址（导航调用成功）。

载荷准备需要补齐两个环节：

1. **下载**。上游用单次 `fetch` 读取整个响应再校验 sha256，遇到代理中途掐断连接就整体失败（实测 `TypeError: terminated`），且不续传。缓存以 sha256 为文件名，因此可以先用 `curl` 的续传与重试把 15 个资源（Python 发行版、Node、wheels）预填进 `.desktop-build/downloads`，再重跑准备步骤；准备本身只需 32 秒。
2. **位置**。载荷落在 `.desktop-build/targets/mac-x64/runtime/primary-runtime`，而外壳不传 argv[4] 时 Host 会推导到 `<runtimeDir>/../runtime/primary-runtime`，两者不一致。运行时用 `DSH_HOST_PRIMARY_RUNTIME` 显式指向即可。

### 已解决：工作区文档在壳内启动失败

接入真实 Host 后应用文档能加载，但 62 个客户端插件全部 import 失败。逐层定位出两处缺陷并修复，另有一处与上游对齐的改动。

**一、注入表被应用两次。** 本外壳的工作区文档直接来自 Host，而 Host 已把注入表渲染进 HTML（上游 `tapIndex` 的服务端形式）。Electron 不同：它的文档取自本地静态 `dist`，不含注入行，必须由应用运行时再应用一次。两者都做会让插件 bundle 二次加载并触发重复注册，模块系统构造失败、`__ModuleLoader__.mode` 停在 `queue`，于是所有入口都无法激活。`boot` 现在返回空注入表——这个前提成立，全靠文档来自 Host。

**二、认证 cookie 不会在重定向链上被回送。** Host 下发的 cookie 是 `SameSite=Strict`，而窗口的第一份文档来自 dsh-app 加载页、属于跨站发起，WebKit 因此在 303 重定向后的请求上不带该 cookie，窗口停在 401 纯文本页。该页与 Host 同源，从它再发一次同源导航即可让 cookie 生效；注入脚本据 content-type 与页面文案识别该情形并自愈。

上一条是 62 个入口全部失败的成因，下一条是窗口停在认证页的成因。此外还有一处**不是**成因、但与上游对齐的改动：`streamBaseUrl` 改为返回源（`new URL(hostUrl).origin`，与 Electron 一致），因为该值被客户端当作资源基址，带 `/?token=…` 的完整地址不是基址。当时一度把它列为三处成因之一，反证时才发现：单独改它 62 个入口依然失败，把它改回去也不触发任何失败——它有记录价值，但不是修复。

排查中记下两条方法论，都曾导致误判：

- **wry 的 `cookies_for_url` 在 IP 主机下恒为空**。它用 `cookie.domain() == url.domain()` 过滤，而 `url::Url::domain()` 对 IP 字面量返回 `None`。曾据它得出「webview 没存 cookie」的结论，实为读数无效。可靠信号是「应用文档是否调用 `boot`」——`apps/web/src/main.ts` 在 `dshDesktopBoot` 存在时必定调用。
- **用真实浏览器打开同一个 Host 地址做对照**，应用与 62 个插件全部正常，从而把故障范围确定在壳侧。隔离环境差异比在单侧深挖更快。

**验证**：外壳、桥接、Host 三进程稳定存活，运行期无任何渲染层启动失败上报——`bootClient` 在任何入口未激活时都会经 `boot_failed` 上报。

### GUI 断言依赖辅助功能通道

macOS 的辅助功能通道会**整体性**变得不可用，而不只是对某个应用失效：实测失效时连 Finder 与 VS Code 都报 0 个窗口。此时任何基于 AX 的读数都无效——包括「窗口数为 0」这种看似确凿的证据——依赖它的断言也不能作为失败依据。

因此 `scripts/e2e.sh` 先探测 AX 可用性：不可用则跳过全屏与关窗断言并说明本次未验证什么，退出码为 0；可用时按原样硬断言。判断外壳是否真的进入工作区应看外壳侧的导航日志，而不是窗口枚举。

真实 Host 的启动契约（`apps/desktop/src/host-process.ts`）已核对，argv 位置与本外壳一致：

```
node --expose-internals <runtimeDir>/node_modules/@deepseek-ai/dsh-desktop-host/lib/index.js \
     <runtimeDir> <projectDir> <primaryRuntime> [pnpm, nodeBin]
```

上游用 `desktopNodeEnvironment` 构造子进程环境，其中只有 `ELECTRON_RUN_AS_NODE`（对普通 node 无意义）与**打包应用里捆绑的包管理器路径**会影响行为。后者对应可选的 argv[5]、[6]，本外壳目前不提供——见 P5。

**验证**：关闭窗口后 `pgrep node` 不再有残留 Host 进程。

## P2 — 补全可选桥接

目标：把上游在桌面壳里期待的可选全局补齐到可用状态。四项已结案（2.1、2.2、2.3、2.5），一项有意不做（2.4），证据见上面的「P2 进度」。

**验证**：逐个桥接写单元测试（Rust 侧的命令返回结构 + shim 暴露的成员名），再跑上游使用这些桥接的客户端测试。

## P3 — 侧边栏浏览器（A 已定）

D1 定为 **A（iframe）**：shim 不暴露 `browser`，上游回退到 sandboxed iframe provider，零上游改动。实测得到可框性数据后仍然选择 A，接受已知代价；数据与代价见下面的 P3-A 结论。

### 依赖获取

本机的官方 registry（`registry.npmjs.org`、`crates.io`、`pypi.org`）经代理连不通，国内镜像可达：`registry.npmmirror.com`、`rsproxy.cn`（已实测可取到 `tauri-plugin-os` 等包）。因此装依赖走镜像，**不改全局配置**：cargo 用项目内 `.cargo/config.toml` 或 `--config`，pnpm 用 `--registry`。这不影响已构建完成的离线链路——`cargo build --offline` 依然可用。

若选 A（iframe）：不做代码改动，只做验证——确认 `desktop === undefined` 分支真的被走到，并实测一批常见站点能否被 frame。

### P3-A 实测结论

**分支确认为 iframe。** 运行时在侧边栏浏览器面板里查到 `<iframe>` 元素，同一页面上 `webviewSupported === false`（Chromium 没有 Electron 的 `<webview>`），因此走的是 `createIframePage`。代码侧的判据也成立：`carrier?.protocolVersion === 1 ? carrier.browser : undefined`，而本外壳不再定义 `dshDesktop`，故 `desktop === undefined`；端到端脚本断言了该标记缺席。

**可框性：抽样的 10 个站点里有 6 个拒绝被 frame。**

| 站点 | 屏蔽方式 | 能否内嵌 |
| --- | --- | --- |
| GitHub | `X-Frame-Options: deny` + `frame-ancestors 'none'` | 否 |
| MDN | `X-Frame-Options: DENY` | 否 |
| Stack Overflow | `SAMEORIGIN` | 否 |
| VS Code 文档 | `frame-ancestors 'self'` | 否 |
| 阮一峰博客 | `SAMEORIGIN` | 否 |
| DeepWiki | `frame-ancestors 'self' vscode-webview://*` | 否 |
| 掘金 | 无 | 是 |
| Python 文档 | 无 | 是 |
| 百度 | 无 | 是 |
| 知乎 | 无 | 是 |

拒绝时面板里是浏览器自己的错误页，没有产品文案。实测 GitHub 的报错是 `net::ERR_BLOCKED_BY_RESPONSE` 加一条 `Framing … violates … "frame-ancestors 'none'"`；可框站点（掘金）能完整渲染。

**这对 D1 是决定性的：`X-Frame-Options` 与 `frame-ancestors` 只约束 frame，不约束以顶层文档加载的原生子 webview。** 上游 Electron 外壳用 `<webview>` 正是因此不受这批限制，而 D1-B 会整类消除这些失败。按上面的数据，A 路线在一个面向开发者的浏览器侧边栏里会有一半以上的常用站点打不开。

A 路线另有两条固有代价，均已由上游注释与代码确认：`keepMounted: desktop !== undefined`，iframe 路径下为 `false`，切走再切回会重建页面；没有独立存储分区。

**若选 B（原生子 webview）**：工作量最大的一个阶段。

- [ ] **3.1** 实现 `DesktopBrowserBridge`：`acquire` 返回 `{lease, partition}`，`release` 销毁 guest，`onOpenRequested` 转发新窗口请求。租约管理与上游 `browser-guests.ts` 的语义一致（主进程签发、匹配租约与分区）。
- [ ] **3.2** 用 `Window::add_child` 创建 guest webview，位置与尺寸跟随渲染层报告的占位区域。
- [ ] **3.3** 隔离：guest 的 webview 标签**不匹配任何 capability**，从而拿不到 Tauri IPC；存储分区用 `dataStoreIdentifier`（macOS 14+）/ `dataDirectory`（Windows、Linux）。
- [ ] **3.4** 上游加 provider 注册点：把 `desktop === undefined ? iframe : electron` 换成按壳类型查表，让 Tauri provider 能注册进来。这是需要上游 PR 的一步。
- [ ] **3.5** 导航白名单：上游 Electron 外壳只放行 HTTP(S) 且拒绝 URL 内嵌凭据，新 provider 保持同样规则。

**验证**：打开真实 HTTPS 站点、站内跳转、多标签、退出后 guest 被销毁；确认 guest 无法调用任何 Tauri 命令。

## P4 — 拖放（D3 已定）

**结论：Tauri 默认配置下 HTML5 拖放整体失效，而不只是 `@path` 芯片失效。**

Tauri 默认给 webview 装一个认领拖放的处理器，`tauri-runtime-wry` 里的闭包固定返回 `true`。wry 在认领时不调用 WebKit 的 `super`（`wry-0.55.1/src/wkwebview/drag_drop.rs` 的 `dragging_entered` / `perform_drag_operation` 都是「认领则返回 Copy/YES，否则 `msg_send![super(...)]`」）。Web 进程因此拿不到 `draggingEntered`，页面连 `dragenter`/`dragover`/`drop` 都收不到——拖图片进对话也上不了传。`disable_drag_drop_handler()` 会把处理器换成 wry 的默认实现 `Box::new(|_| false)`，恢复委托给系统。

- [x] **4.1** 窗口构建时关闭 Tauri 的拖放处理器（`src-tauri/src/lib.rs`）。
- [ ] **4.2** `@path` 芯片与拖入文件夹：外壳不装 `__DSH_HOST_PATHS__`（关掉 Tauri 处理器后没有磁盘路径来源），于是走上游为「无桥接」设计的降级分支（`packages/client/ui-conversation/src/client/apply.ts:429-435`）：`path === ''` 时一律 `uploads.push(file)`，且 `bridge === undefined` 时拖入文件夹直接返回 `attachment.directoryDesktopOnly`。与 Electron 桌面端有两处差异：**非图片文件上传为附件**而不是 `@path` 芯片；**拖入文件夹被拒**。上游 JSDoc 把这一分支写成「a served Web page has none, so every non-image file uploads there」，即本外壳行为与浏览器里打开 Web 版一致，属上游有意设计而非本项目的缺陷。要补只能走上游扩展（原 D3-B，待提）。
- [ ] **4.3** 回归上游 `packages/client/ui-conversation` 里关于 `@path` chip 的测试。

**原 D3-A 不可行**：`tauri://drag-drop` 与 DOM drop 不会同时到达——认领就没有 DOM drop，不认领就没有事件，两者互斥。

**验证**：2026-09-23 实测。真人拖拽恢复正常；再用会话日志交叉核对——拖入的 PDF 记为 `{"type":"file","attachment":{…}}` 内容部件，落盘在 `~/.dsh/attachments/v1/files/`，与图片（`{"type":"image","attachment":{…}}`）走同一条附件管线，消息内无 `text` 部件即无 `@路径` 芯片。操作系统级拖拽无法自动化，只能靠真人触发加日志核对。

## P5 — 打包与签名

- [ ] **5.0 捆绑包管理器**：Host 用可选的 argv[5]、[6] 接收 pnpm 与 node 的路径，配合 `DSH_DESKTOP_NODE_EXECUTABLE` 与 PATH 前缀，供插件安装使用。上游 Electron 产物里捆绑了这两者；本外壳目前不传，于是插件安装只能在系统已装 Node/pnpm 的机器上工作。打包时必须一并解决。
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
