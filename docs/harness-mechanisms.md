# 可借鉴机制分析：DeepSeek Harness → 自有 agent 项目

## 缘起

本仓库（`dsh-desktop`）是把上游 DeepSeek Harness 的 Electron 桌面端换成 Tauri 外壳。这个工作的实际产出是「把上游产品装进另一个窗口框架」——产品本体（对话交互、子任务、插件系统、会话日志、模型接入）全部来自上游，我们没有写。

因此继续投入的价值分两种情况：想要一个可自用的成熟桌面 agent（值，做完 P5 打包即可），或想借它的架构长自己的能力（不值，外壳一点忙帮不上）。

这份文档服务第二种目的：把上游真正值得借鉴的机制挑出来，逐条给出**出处、它解决的失效场景、维护代价、要不要落地**。

**优先级最高的三条**——都是「为长期可维护预付结构成本」，而多数 agent 项目到后期才被迫补：

1. 会话日志作为唯一真相，并用运行时不变式强制（机制 1）
2. 未知事件默认「拒绝重建」而不是静默跳过（机制 2）
3. 注册即副作用，一切可撤销（机制 5）

## 怎么读

每条机制给出三样东西：**出处**（`路径:行号`，相对上游仓库根）、**它解决什么失效场景**、**落地建议**。落地建议分三档：

- **照搬** —— 与技术栈无关，直接抄思路
- **按需** —— 有明确触发条件时再引入
- **暂不建议** —— 当前规模下不划算，但要知道它存在

本分析基于对上游仓库的静态阅读，未在上游仓库修改任何文件。若你自有项目的技术栈与 Cordis（TypeScript 插件框架）差异大，注意区分「机制本身」与「Cordis 的实现方式」。

---

## 1. 会话日志即唯一真相

**出处**：`AGENTS.md:136`、`docs/architecture.md:125`

原文（`docs/architecture.md:125`）：

> **Model-visible means logged.** Anything that reaches a model request must be reconstructable from the log, and a runtime invariant asserts it. A new model-visible input requires a session event. Plugins that change existing message content register pure message projections.

**强制方式**：`packages/core/agent-loop/src/invariant.ts:38-41` —— 在 dispatch 时把实际发出的请求与 `session.deriveMessages()` 的投影结果比对，不一致即报 `log-reconstruction desync`。

**解决什么**：上下文来源不可审计、会话无法回放、格式迁移无据可依。更隐蔽的是：日志和真实请求会**漂移**——某处顺手往请求里加了内容但没记日志，于是回放出的会话与真实会话不同，而没人发现。

**落地建议：照搬。** 关键不在「写日志」，而在**让日志成为模型请求的唯一构造来源**：模型输入由 `deriveMessages(log)` 投影得出，而不是各处拼装。后半句才是这条规则的重量所在。

那条运行时不变式是灵魂。没有它，规则会随时间腐化——总有人为了方便在某处直接改请求。有了它，「模型可见输入必须落成事件」从约定变成运行时会炸的约束。

代价：所有模型可见输入都要走事件，插件不能私自往请求里塞东西。

---

## 2. 未知事件默认「拒绝重建」

**出处**：`packages/core/session/src/types.ts:501-510`、读侧守卫 `packages/session/session-persistence/src/storage-contract.ts:75`

原文：

> Absent means required: a reader meeting an unrecognized type without this marker MUST refuse to reconstruct the session instead of silently dropping the event, because an unrecognized required event may change how the rest of the log is interpreted. ... defaulting to required means a forgotten marker over-refuses (an inconvenience) rather than silently resuming a gutted session.

**解决什么**：新版写入的日志被旧版读到，旧版静默丢掉不认识的事件，重建出一个「被掏空的会话」——而且表面看不出任何错误。

**落地建议：照搬。** 这是一个**默认值选择**问题，成本几乎为零，收益是消灭一整类静默失败。任何持久化事件流（会话、审计、任务队列）都适用。

注意它的推理方向：**默认值选错时，宁可「多拒一次」（烦人），不要「静默继续」（危险）**。这个判断标准比规则本身更可复用。

---

## 3. 版本只在结构变更时 bump，且已提交世代永不改动

**出处**：`packages/core/session/src/types.ts:74-83`、`AGENTS.md:7`、`docs/session-format-status.md:20`、`.agents/notes/implemented/architecture/2026-08-31-released-session-format-migrations.md:82`

原文（bump 判据）：

> Only structural changes reach that bar: the header shape, the event envelope, core event semantics, or the surface mechanism. Adding an ordinary event type does not bump — the per-event `ignorable` guard covers vocabulary growth instead. When in doubt, bump: a near-identity upgrade step is almost free, a missed bump makes older runtimes read new logs wrong silently.

原文（世代保全，`AGENTS.md:7`）：

> Adjacent migration may add a version-named successor but never move, overwrite, or delete committed generations; predecessors imply neither fallback nor downgrade support.

原文（写入器单一权威，`docs/session-format-status.md:20`）：

> **Checkout writer:** `SESSION_FORMAT_VERSION` in core Session types is the only hand-maintained current-writer number in code. ... A package version, codec export name, fixture filename, or projection-cache version is not the writer authority.

**解决什么**：版本号多源混乱（包版本、文件名、常量各说各话）、升级路径缺失、历史数据被就地迁移覆盖导致无法回退取证。

**落地建议：按需。** 若你的项目已经有发布给别人用的持久化会话数据，**照搬**；若还在原型、随时可清库，**别过度设计**——但「写者单一权威」这一条现在就该定，否则后期要费力统一。

两个可直接抄的细节：**兼容性变更不 bump、结构变更才 bump**；**兼容性变更在同一个版本内用新的 acknowledgement 记录**。

---

## 4. 事件类型用 declaration merging，并生成目录

**出处**：`AGENTS.md:133`、`docs/cordis-primer.md:27`、`scripts/gen-scoped-events.ts:9-12`、`scripts/persistence-catalog-source.ts:197`

三个部分：

1. **可合并、只追加的事件表**：各包用 `declare module '@deepseek-ai/dsh-session/types' { interface SessionEventMap { ... } }` 扩展（例：`packages/core/tools/src/types.ts:28`、`packages/goal/goal/src/domain.ts:62`）。文档注释称其为 "The merge-extensible, append-only source of truth for an agent interaction. Message history is derived from this log."（`packages/core/session/src/types.ts:279-282`）

2. **`@mode` 标签**（`docs/cordis-primer.md:27`）：

   > The dispatch mode is part of the event's public contract. New harness events document it with an `@mode` tag so the generated catalog can check declarations against dispatch sites.

   取值 `emit|waterfall|parallel|serial|bail`（`scripts/jsdoc.ts:62`）。生成器把 JSDoc 声明与真实分发点对照，缺标签即失败。

3. **`@dshScopeScan unsupported`**：作用域事件必须能从 payload 推出路由键；推不出或推重即报错（`scripts/gen-scoped-events.ts:9-12`：*"Zero matches require `@dshScopeScan unsupported`; multiple matches are ambiguous and always fail loud."*）

**反向约束值得注意**：会话日志事件**不得**带 `@mode`——它不是总线事件。生成器硬报错，原话是 "carries an @mode tag, but a log event has no dispatch mode ... Remove the tag."（`scripts/persistence-catalog-source.ts:197`）

**解决什么**：事件表散落各处、文档与代码脱节、作用域路由靠口头约定。

**落地建议：按需，但建议抄一半。** `declaration merging` 那半解决「谁定义了哪些事件有单一位置」，成本极低，**照搬**。`@mode` + 生成目录那半的价值是**用生成物抵消文档腐化**——如果你没有生成器基建，可以先只抄「事件表是合并扩展的接口」，等需要时再加生成器。

---

## 5. 注册即副作用

**出处**：`AGENTS.md:131`、`docs/cordis-primer.md:13,45`、`docs/cordis-api/fiber.md:9`、`docs/cordis-tutorial/02-lifecycle-and-effects.md:5`

原文（`docs/cordis-primer.md:13`）：

> Registrations are reversible effects. Prompt sections, tool schemas, adapters, providers, and listeners are installed through `ctx.effect()` or `ctx.on()` so reload and teardown unwind them predictably.

原文（`docs/cordis-tutorial/02-lifecycle-and-effects.md:5`）：

> A Cordis plugin can be unloaded by a config edit, hot reload, explicit disposal, or loss of a required service. Registrations made through Cordis APIs are effects and are undone when their owning plugin unloads; resources managed outside those APIs must be wrapped in `ctx.effect()`.

disposer 语义（`docs/cordis-api/fiber.md:9`）：逆序执行；重复调用是 no-op；fiber 已释放时报 `INACTIVE_EFFECT`。**但**多个异步 disposer 并发执行、无串行完成保证（`docs/user/develop/framework/index.md:63`）——所以需要顺序收尾的工作要放进同一个 effect。

**解决什么**：热重载后旧注册残留（工具重复注册、监听器翻倍）、插件卸载泄漏。上游把它描述为连带的开发体验收益（`docs/cookbook/extension-cookbook.md:132`）："Plugin hot-reload | every registration is a `ctx.effect` → vendored HMR just works"

**落地建议：照搬。** 核心是三句话：每个注册返回 disposer；容器在卸载时自动跑；**配置改动等价于卸载+重装**。第三句收益最大——它让「改配置」和「重启进程」等价，开发循环完全不同。

诚实提示：上游**没有**专门静态强制「所有注册都走 effect」的门禁，只有规则文本（`AGENTS.md:131`）和邻近门禁。这条靠人守。

---

## 6. 能力缝三角色

**出处**：`docs/glossary.md:7-9`、`docs/architecture.md:129,131`、`AGENTS.md:135`

原文（`docs/architecture.md:129`）：

> A **seam** is a swappable capability with three roles: a **Service Definition** declaring the interface, a **Service Provider** implementing it, and a **Consumer** using it, commonly a model-facing tool. A package may combine roles, but one role alone is not a seam; adding a capability means designing all three.

拆缝判据（`docs/glossary.md:7-9`）：角色**独立演进**时才分属不同包；一个 concern 可由单包同时持有多角色（举例：`dsh-user-approval` 同时持有审批缝的 Service Definition 与实现）。

收益案例（`docs/architecture.md:131`）：

> Seams are why one provider swap changes the whole product. Filesystem and subprocess providers share one execution world, so pointing them at a remote sandbox moves Bash, PTY, and LSP with them, with no provider forks.

**解决什么**：两个相反的毛病——加能力时不知道要定义哪些东西（只写了接口没有实现，或没有模型面入口）；或反过来，为每个小接口拆三个包。

**落地建议：照搬词汇与判据，别照搬拆包粒度。** 真正有用的是那两句：「一个角色不构成缝」（挡住半成品设计）和「独立演进才拆」（挡住过度拆分）。

---

## 7. 组合用 YAML 补丁层，不用代码分支

**出处**：`packages/preset/agent-preset/skills/cordis-composition-reference/SKILL.md:10-23`、`docs/cordis-primer.md:39`、`docs/user/develop/basic/publish.md:124-131`、`docs/postmortem/0002-js-expression-disabled-filesystem-tools.md:9`

支持的操作（`SKILL.md:10-23`）：

- `insert: [rows]` 追加；带 `id` 指向已有 `group: true` 行时插入该组
- 带 `id` 且无 `insert` 则覆盖该行。**`config` 整体替换，绝不深合并**，必须重述该行需要的每个字段
- `group: true` 配 `name: cordis:group` 让 `config` 成为嵌套条目列表
- `disabled` 接受布尔、null 或 `!!js` 表达式（每次挂载判定时求值）
- `isolate` 映射服务名到 `true` 或 realm 标签，隔离服务实例
- `cordis:include` 从 `config.path` 载入条目列表

层序（`docs/user/develop/basic/publish.md:124-131`）：bundle patch → profile 自己的 patch → home 级 patch → 每个 `--patch` overlay（argv 顺序）。

`!!js` 位置白名单（`docs/cordis-primer.md:39`）：只在插件 entry 的 `config`（在声明的 inject 生效后、对该插件 ctx 求值）与 entry 的 `disabled`（每次挂载判定）中求值，**其他元数据保持字面量**。

**一个真实事故**（`docs/postmortem/0002-js-expression-disabled-filesystem-tools.md:9`）：

> The ACP example attempted to enable filesystem plugins conditionally with `disabled: !!js ...`, but Cordis evaluates JavaScript expressions only inside plugin `config`. The raw expression object was truthy, so the filesystem stack was always disabled.

**解决什么**：部署差异（开发/生产、各平台、各用户）用代码里的 `if` 处理，导致组合逻辑散落、无法从配置看出实际装了什么。

**落地建议：按需。** 值得抄的是方向——**部署差异用数据层叠解决**。两个必须记住的细节：(a) 按 id 覆盖时 `config` 不深合并，要重述全部字段；(b) 表达式只允许出现在白名单位置，其他位置的字面量表达式对象**恒真**，会静默改变组合。第二点若你的配置支持表达式，务必抄这个白名单。

---

## 8. 工具 UI 呈现前置设计

**出处**：`AGENTS.md:156`、`docs/cookbook/adding-a-tool.md:69,87,88,89,97`、`packages/client/AGENTS.md:56`

原文（`AGENTS.md:156`）：

> Design each tool's UI presentation up front. Host presenters stay pure; Web cards derive from raw events and persisted result metadata.

原文（纯度硬约束，`adding-a-tool.md:87`）：

> These run on live streaming AND on session-log REPLAY, so they must be pure functions of `args` (+ the result) — NO I/O, NO reading session state, NO clock/random. ... If you find yourself wanting the file's old content or the working directory inside `presentCall`, stop — that belongs in durable result metadata or the adapter, not the presenter.

原文（UI 格式不得进模型结果，`adding-a-tool.md:88`）：

> A fenced console block, a diff, a relativized path — none of these belongs in the canonical value or Native content merely to serve a UI. `output.render` owns model-facing prose; `presentationMeta` plus the card presenters own replayable UI state.

另外两点：`defineTool` 对显示路径**软校验**——老日志或畸形参数让 presenter 返回 `undefined`（退化成通用卡片）而不是抛错，理由是 "display must never crash a replay"（`:89`）；工具包不导入任何 UI 或 transport 类型，中性词汇（`ToolCallKind`，`docs/subsystems/tools.md:481`）由 `dsh-tools` 持有。

**解决什么**：回放时卡片与实时不一致（presenter 读了文件系统或时钟）；为了好看改动模型看到的内容；老数据把回放打崩。

**落地建议：照搬。** 对任何「有工具调用 UI」的 agent 项目都成立，而且好处是**复合的**：presenter 纯 → 回放一致 → 可以拿录制会话做 UI 回归测试。

最容易忽略的是「UI 专用格式不许进模型结果」。把 console 围栏块塞进工具返回值会让模型多读噪音，还会破坏程序化取值。

---

## 9. 无密钥录制会话回放快照

**出处**：`package.json:64-66`、`vitest.snapshot.config.ts:28-32`、`snapshots/AGENTS.md:3,5,11`、`docs/testing.md:14,53-55`

原文（为什么可以免 key，`vitest.snapshot.config.ts:28-32`）：

> Replay is the keyless default: boot real subprocess paths from recorded model responses and diff assembled requests, normalized protocol or transcript output, and persisted-log expected outputs. `record` calls the real API and updates fixtures and expected outputs; `refresh` replays committed scripts and updates current expected outputs. Replay/refresh never load `.env`; only record reads a key.

原文（何时必须更新，`docs/testing.md:53-55`）：

> Every non-trivial model-, protocol-, or human-visible change adds or updates a keyless recorded-session scenario in the same PR; package, e2e, mock-only, and rationale evidence does not replace the assembled transcript.

归属规则（`snapshots/AGENTS.md:3`）：顶层树**只**放「已提交的会话 JSONL 既是回放输入又是期望输出」的用例；ARIA、几何、生成器、CLI、单元测试的期望输出留在各自 owner 本地。

归一化固定点（`snapshots/AGENTS.md:11`）：把易变身份替换为保留关系的 token，但**不得**因为「看起来像标识符」就脱敏真实文本。

**解决什么**：LLM 应用做不了常规回归——每次真调 API 又慢又贵又不确定；而 mock 掉模型又测不到真实拼装出的请求。

**落地建议：照搬。** 我认为这是**投入产出比最高的一条**——它把「模型可见行为」变成可进 CI 的确定性测试。

三个实现要点：录制时**同时存请求与响应**（这样能 diff「实际拼装出的请求」，而不只是比对输出）；回放默认免 key（否则 CI 跑不起来，这条件就没有意义）；归一化只替换易变身份，别过度脱敏。

---

## 10. 逐文件 100% 覆盖率，且把未覆盖行当死代码

**出处**：`vitest.config.ts:358-362`、`docs/testing.md:10`、`AGENTS.md:88,118`

原文（`vitest.config.ts:358-362`）：

> 100% or it doesn't merge ... Per-file so a well-covered big file can't subsidize a bare one.

原文（`docs/testing.md:10`）：

> An uncovered line is often dead code the gate flags for deletion, not a missing test to bolt on. Line coverage is necessary, never sufficient — it proves lines ran, not that the feature works as shipped.

**落地建议：按需，但先抄一半。** 可以立刻抄的是**心态**：看到未覆盖行，先怀疑是死代码，而不是「补个测试盖住」。

机械的逐文件 100% 阈值适合「核心不变式集中、变动少」的库层（上游正是把它限定在 `packages/*/*/src`），不适合早期快速变动的 UI 层。

---

## 11. 大量机械门禁（`verify-*`）

上游有约 40 个 `scripts/verify-*.ts`。摘几条最可移植的：

| 门禁 | 检查什么 | 可移植性 |
|---|---|---|
| `verify-export-jsdoc` | 每个非 vendor 包导出的 JSDoc 完整性（函数需 `@param`、非 void 需 `@returns`）；未知形态 fail closed | 高 |
| `verify-md-links` | 相对 Markdown 链接、图片、定义必须可解析，`#fragment` 必须是真实标题或显式 `<a id>` | 高 |
| `verify-md-wrap` | 拒绝跨多个物理行的 Markdown 段落 | 高（顺带让 diff 干净） |
| `verify-no-unknown-casts` | 拒绝新增 `as unknown` / `<unknown>`，只允许存量基线递减 | 高 |
| `verify-optional-dependency-imports` | 拒绝静态 import 可选依赖："one absent package turns 'this capability is unavailable' into a load failure for everything that reaches the importing module" | 高 |
| `duplication`（jscpd） | 跨文件克隆检测，`minTokens: 60`、`minLines: 6` | 高，成本极低 |
| `verify-client-ui-i18n` | 拒绝把产品 UI 文案硬编码在 Client 源码 | 中，有 i18n 需求才要 |
| `verify-doc-budgets` | 按清单给常驻文档设词数上限（`wc -w` 式） | 中，文档多了才需要 |

**落地建议：按需挑 3-4 条起步。** 真正的启发不是「门禁多」，而是**把约定从人的记忆搬进 CI**——上游连「段落不许换行」「不许出现某个特定词」这种审美约定都机械化了。

挑选标准：**找那些「违反了但代码仍能跑」的约定**。它们没有失败信号，所以必然腐化。

---

## 12. Source plane 与 artifact plane 不混用

**出处**：`AGENTS.md:146`、`docs/testing.md:43-45`、`docs/development.md:52-68`

原文（`docs/testing.md:43-45`）：

> Every vitest config points vite-tsconfig-paths at `tsconfig.base.json`; bare workspace imports resolve to `src`, never through package `exports` to built `lib/` — **stale artifacts there load a second copy of module singletons.** Built artifacts are consumed only explicitly: `lib`-mode subprocesses and the built smokes below.

**解决什么**：monorepo 里陈旧构建产物让模块单例被加载两份，症状是诡异的「状态不同步」——两个模块看似在共享状态，实际各有一份。

**落地建议：按需。** 只在 monorepo + 多包 + 测试直接跑源码的场景下有意义。但它标识的失效很隐蔽，值得提前知道。

---

## 13. 子代理 / 后台任务 / 定时任务的三层切法

三组的形状（`packages/subagent/README.md`、`packages/jobs/README.md`、`packages/schedule/README.md`）：

| 组 | Service Definition | Provider | Consumer |
|---|---|---|---|
| subagent | `ctx.subagents`（`packages/subagent/subagent/src/index.ts:200`） | 8 个 provider 包（spawn / fork / acp / codex / claude-code / dsh-sdk…） | `tool-subagent` 暴露模型面工具，`tool-subagent-control` 提供控制面 |
| jobs | `ctx.jobs` 抽象类（`packages/jobs/jobs/src/index.ts:85`） | `jobs-local` | `tool-jobs`（`job_output` / `job_list` / `job_kill`） |
| schedule | 无（单包内完成） | — | `schedule` 包自注册三个工具 |

值得抄的判断：

**契约与实现分属两包，且直接实例化契约类会抛错**（`packages/jobs/jobs/README.md:69`）：

> `JobRegistry` is an abstract Cordis service; loading the class directly throws, so a misconfigured composition fails **at load** instead of registering an empty `ctx.jobs`.

**服务 + provider 还不够，模型面工具是第三个角色**（`packages/subagent/subagent/README.md:43`）：

> Mounting the service alone changes nothing: nothing can delegate until a provider and a tool are composed.

**工具生命周期镜像 provider**（`packages/subagent/tool-subagent/README.md:28`）：

> Mount one instance per delegation target, each with a distinct `toolName`. The tool exists exactly while its provider does, so sibling load order and provider reloads never strand it.

**能力缺口显式拒绝，不静默省略**（`packages/subagent/subagent-acp/README.md:32`）：

> this provider advertises no optional start-time capabilities, so the seam rejects requests for `agentOptions`, structured output, depth caps, tool filters, or personas rather than silently omitting them.

**最小可运行组合写进 README**（`packages/jobs/jobs/README.md:57-62` 直接给出两行 YAML）。

**落地建议：照搬「三角色」和「最小组合写进 README」。** 前者直接对应你要的子任务能力；后者让新人（和三个月后的你）能在五分钟内跑起来——上游把这一条做成了 README 的固定节。

---

## 不建议照搬的

诚实列几条，它们的收益需要规模才兑现：

- **逐文件 100% 覆盖率** —— 上游是 200+ 包的库式仓库；早期产品层照抄会拖慢迭代。抄它的心态，别抄阈值。
- **约 40 个静态门禁** —— 每个都要有人维护和解释。起步挑 3-4 条。
- **双语文档 + 词数预算 + Agent Note 归档封印** —— 为「多人长期维护 + 对外发布」设计的重流程，小团队会变成仪式。
- **`!!js` 白名单、`isolate` 语义等 Cordis 特有约束** —— 不用 Cordis 就不适用。但「表达式只在白名单位置求值」这个教训是通用的。
- **`@dshScopeScan` 这类生成器驱动的作用域标注** —— 需要生成器基建，投入不小。

## 建议的落地顺序

假设要在自有项目里改，按依赖关系排：

1. **定下模型请求的唯一构造来源**（机制 1 的前半）：模型输入由日志投影，不能各处拼装。这是后面所有机制的地基。
2. **加未知事件默认拒绝**（机制 2）：一个默认值选择，几乎零成本。
3. **把注册改成可撤销**（机制 5）：需要一次重构，但换来「改配置 = 重启」的开发循环。
4. **上录制会话回放快照**（机制 9）：把模型可见行为纳入 CI，投入产出比最高。
5. **用能力缝三角色审视工具与子任务边界**（机制 6 + 13），顺手抄「工具生命周期镜像 provider」和「能力缺口显式拒绝」。
6. 其余按需。

## 验证状态

- 本文所有引用来自对上游仓库的静态阅读，逐条标注了 `路径:行号`。
- 未在上游仓库（`~/op/deepseek-harness`）修改任何文件。
- 机制 1、2、3、5、9 的**行为**未在本机实测；机制 8 与拖放相关的部分在本次会话中有实测记录（见 `roadmap.md` 的 P4 与 D3）。
- 若要把本文件并入你自有项目，直接移动即可，本文不依赖本仓库的其他内容。
