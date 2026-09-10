# Coding Agent Monitor

一款面向 Windows 的轻量级 Coding Agent Token 使用量监控工具。

基于 **Tauri 2 + React + TypeScript + Rust + SQLite + vendored ccusage** 构建：ccusage
v20.0.20 的采集源码与 Antigravity 的 downstream 移植直接编译进产品 EXE，并在同一 EXE
的隔离 worker 进程中采集全部 17 个 Coding Agent 的用量，不再依赖任何外部 ccusage 可执行文件。

目标很简单：

> 不打开命令行，也能随时查看本机 Coding Agent 今天用了多少 Token。

> **当前状态：v0.3.0 已于 2026-09-01 正式发布**，可从
> [GitHub Release](https://github.com/A1S4kuR4/coding-agent-monitor/releases/tag/v0.3.0)
> 下载 MSI / NSIS 安装包。仅官方支持 Windows 11 x64；安装包未签名，
> 非 ASCII 用户目录完整 GUI 场景未验证。发布验收与三项已批准 waiver 见
> [`docs/V0.3_RELEASE_GATE_DECISION.md`](docs/V0.3_RELEASE_GATE_DECISION.md)；
> 过程记录见 [`docs/V0.3_PHASE5_RELEASE_CANDIDATE.md`](docs/V0.3_PHASE5_RELEASE_CANDIDATE.md)。
> v0.2.0 已发布（[验证记录](docs/V0.2_RELEASE_VERIFICATION.md)）。
> 非 ASCII Windows 用户目录的完整人工 GUI Gate 历史上为 **WAIVED / NOT RUN**；
> 发布安装包未经 Authenticode 签名，安装时可能出现 SmartScreen/未知发布者提示，
> 请核对 Release 附带的 SHA-256 校验值。

## v0.4.0 本地候选包

v0.4.0 已进入本轮干净构建与原生验收；它尚未作为正式 Release 发布。
新功能包括统一新鲜度/覆盖说明、中英偏好、托盘启动行为、跨重启恢复和按需 30 日历史。
构建与实际验收状态见 [v0.4 验证记录](docs/V0.4_RELEASE_VERIFICATION.md)，
变化摘要见 [v0.4 Release Notes](docs/V0.4_RELEASE_NOTES.md)。

## 隐私与安全

- 所有统计都在本机完成，程序不要求登录或云端账号。
- vendored ccusage 以 `offline: true` 调用，使用内嵌定价快照；React 前端只接收聚合结果，不读取原始日志。
- 项目不包含遥测。真实 Token 导出、成本明细、日志和本机截图均被 Git 忽略。
- 请勿在 Issue 中上传真实 Agent 日志、用量导出、数据库或包含隐私信息的截图。

---

## 核心功能

### 1. 自动统计 Coding Agent Token

启动后自动检测本机已有的 Coding Agent 数据。下文数字与文本布局均为说明用示例，
不代表真实用量或当前界面的逐字还原：

```text
Claude Code    8.42M Tokens
Codex          5.17M Tokens

今日合计      13.59M Tokens
```

当前用户可见支持范围（17 个 Agent，开放字符串 ID + Rust `displayName`，未知 Agent
可安全显示）：Claude Code、OpenAI Codex、OpenCode、Amp、Droid、Codebuff、Hermes、
Pi、Goose、OpenClaw、Kilo、GitHub Copilot、Gemini CLI、Kimi、Qwen Code、Grok CLI，
以及 **Antigravity（CAM 维护的 downstream 移植，非 ccusage 官方支持）**。

v0.3 值语义（相对 v0.2 的两项已批准修正）：来源明确的 reasoning/thinking 单独计入
`reasoningTokens`（不再混入 unclassified）；存在未定价模型的日子成本显示为
`null`——不再伪造 `$0.00` 或输出误导性的部分成本。底层复用 vendored ccusage
已有的数据解析能力，不重复实现 Agent 日志解析。

---

### 2. 简洁的使用量可视化

提供一个简单的主界面，只展示真正有用的信息：

```text
Coding Agent Monitor

今日使用
13.59M Tokens

Claude Code
████████████████    8.42M

Codex
██████████          5.17M


最近 7 天

6.2M   8.1M   5.4M   9.7M   12.1M   10.3M   13.6M
```

当前界面包含：

- 今日 Token 总量、较昨日变化与按用量排序的动态 Agent 列表。
- 可展开的模型 Token 明细，以及来源明确的 reasoning 和未分类余量说明。
- 最近 7 天趋势，按需切换最近 30 天；均包含查询当日，按本地时区日历日分桶。
  全部 Agent 堆叠展示或单 Agent 筛选；点击某日查看 Token 构成和已有模型明细，今日摘要独立显示。
  30 日图表内部横向滚动，保留日期、绝对数值和键盘详情；范围级跳过诊断适用于每一天。
  成功查询缺日补零只表示没有记录；全范围任一有用量日成本未知时不显示部分成本总额。
  30 日仅用户查看/手动刷新时采集，托盘后台与跨重启快照仍保持七日范围。
- 可用时显示预估 USD 成本、缓存输入占比与数据更新时间。
- 手动刷新、加载/空/错误状态；刷新失败时保留最近成功数据并提供重试。

不设计复杂 BI Dashboard。

---

### 3. Windows 系统托盘

程序可以长期运行在 Windows 系统托盘中。

无需一直打开主窗口。

托盘提示与菜单摘要展示今日总量和最多两个用量最高的 Agent；左键点击打开主界面，
右键菜单提供刷新、打开与退出操作。关闭主窗口会隐藏到托盘（首次关闭时窗口内会
说明这一点，可选择不再提示），使用 Exit 才会退出程序；再次启动会直接唤起已运行
的主窗口，而不会开启第二个实例。
启动、打开主界面及每 5 分钟周期触发统一采集；托盘与窗口共享最后成功快照、最近尝试和
刷新中状态。失败会保留并明确标注旧数字，超过 10 分钟或跨本地日期/时区时标为旧数据，
跨日快照继续显示其原日期而不再称为“今日”。
主界面底部折叠的“偏好设置”区域提供最小本地偏好：界面语言（跟随系统/中文/English）、
随 Windows 启动（默认关闭）与启动时隐藏到托盘，并自动恢复正常窗口位置尺寸
（跨显示器/DPI 变化时安全回退）。示意摘要：

```text
Coding Agent Monitor
────────────────────

今日          13.59M

Claude Code    8.42M
Codex          5.17M

立即刷新
打开主界面
退出
```

目标体验类似 TrafficMonitor：

> 安静地运行，需要的时候看一眼。

---

# 产品定位

Coding Agent Monitor 不是：

- Coding Agent 管理平台
- LLM API Proxy
- 模型 Benchmark
- 企业级 Analytics Dashboard

它只是一个：

> **轻量、简单、本地运行的 Coding Agent Token Monitor。**

核心原则：

**轻便**

尽量降低常驻内存和 CPU 占用。

**简单**

不堆积复杂分析功能。

**本地优先**

直接读取本地 Coding Agent 使用记录，不需要账号或云端服务。

---

# 技术方案

```text
Local Agent Records (17 agents, read-only)
        │
        ▼
  product EXE worker（同一 EXE 的隐藏 worker 模式）
  · vendored ccusage engine（worker 内直接调用、离线、只读）
  · 每次快照一个 worker；父进程监督超时/崩溃、合并并发刷新
        │
        ▼
  CAM typed snapshot → Rust normalize_snapshot → UsageSummary
        │
        ▼
    Tauri 2 Application
        │
  ┌─────┴─────┐
  ▼           ▼
React      Windows Tray
```

技术栈：

| 模块 | 技术 |
|---|---|
| Desktop | Tauri 2 |
| UI | React |
| Language | TypeScript |
| Native | Rust |
| Database | SQLite |
| Usage Parser | vendored ccusage v20.0.20 |
| Platform | Windows 11 x64（Windows 10 未经验证，见下方支持声明） |

---

# 为什么复用 ccusage

`ccusage` 已经能够解析多种 Coding Agent 的本地使用记录。

v0.2 通过运行外部 ccusage 可执行文件复用这些 Parser；v0.3 把 ccusage v20.0.20 的
Rust 采集源码 vendored 进本仓库（含可审计的补丁与定价快照），在同一产品 EXE 的
隔离 worker 进程内直接调用，不再下载、打包或运行任何外部 ccusage 可执行文件。

这样可以把开发重点放在：

- Windows 桌面体验
- 系统托盘
- 轻量可视化

而不是重新解决 Agent 日志解析问题。

---

# MVP（历史范围）

第一版只实现三个核心功能，当前均已完成：

- [x] 自动检测并统计 Claude Code / Codex Token（后续版本扩展为动态 Agent）
- [x] 今日用量 + 最近 7 天趋势可视化
- [x] Windows 系统托盘快速查看

其他功能暂不进入 MVP。

---

# 项目状态

> v0.3.0 是已发布版本；当前三份 manifest 为候选构建 `0.4.0`。历史发布资产与校验记录见
> [发布清单](docs/OPEN_SOURCE_RELEASE_CHECKLIST.md)。

v0.3 的主要变化：采集从外部 ccusage sidecar 切换为 vendored 源码 + 单 EXE 隔离
worker，并对 17 个 Agent 完成非空 fixture 对照；reasoning 分类精度提升；缺价成本语义修正为
`null`（不伪造零）。验收记录见
[`docs/V0.3_PHASE5_RELEASE_CANDIDATE.md`](docs/V0.3_PHASE5_RELEASE_CANDIDATE.md)。
v0.2 已使用开放 Agent 契约与统一采集；其真实数据验收中出现的 3 个 Agent 是当时
机器上的活跃来源数，不是产品支持上限。
Antigravity 是 CAM 维护的 downstream 移植（基于未合并的上游 PR），**不是 ccusage
官方支持**。

Phase 10 已于 2026-08-28 执行完毕，记录见
[v0.2 发布验证记录](docs/V0.2_RELEASE_VERIFICATION.md)；v0.2.0 已标记为 release candidate 并按
[开源发布清单](docs/OPEN_SOURCE_RELEASE_CHECKLIST.md) 发布。详细阶段历史、
测试结果和未覆盖项见 [实施计划](docs/IMPLEMENTATION_PLAN.md) 与
[v0.2 开发验收计划](docs/V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md)。

非 ASCII Windows 用户目录完整 GUI Gate 0 对 v0.1.0 与 v0.2.0 均为
**WAIVED / NOT RUN**，不是技术验证通过。v0.2 发布的 MSI、NSIS、主程序和两个 sidecar
的 Authenticode 状态均为 `NotSigned`，Release Notes 已披露 SmartScreen/未知发布者风险；
v0.3 延续同一签名策略与披露。

**先做好一个真正愿意长期放在 Windows 托盘里的 Coding Agent Token Monitor。**

不追求功能多。

优先把启动速度、资源占用、数据准确性和使用体验做好。

---

# 当前实现

当前生产实现：

- React 只消费项目自己的 `UsageSummary`，不接触原始 Agent 日志或 JSON。
- Rust `collector/ccusage.rs` 负责调用 vendor 并转换为项目自己的类型；
  `sidecar/adapter.rs::normalize_snapshot` 负责公共契约归一化。`sidecar` 目录名是历史遗留。
- Rust 负责 worker 监督、SQLite 初始化、系统托盘和本地错误边界。
- SQLite 使用统一的 `sqlite 0.37.0` bundled/linkage 实现，只初始化
  `%APPDATA%\com.codingagentmonitor\usage-cache.sqlite3`，不创建业务表或持久化用量；
  跨重启恢复另存单条版本化的 `last-snapshot.json` 聚合快照；偏好保存在 `preferences.json`。
  所有 Agent 源数据库一律只读。相同日期范围、时区、来源与协议的并发采集共享结果，成功/失败结果在内存缓存 2 秒，
  最多保留两个查询身份；不同查询串行执行且不串用结果。读取短缓存不改写成功采集时间。
- Dashboard 与系统托盘的数据来自产品 EXE 自身的隔离 worker（v0.3 起不再有 sidecar）。
  安装包只包含一个产品可执行文件。

版本状态、阶段历史与验收文档索引见
[`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md)，开发约束见
[`AGENTS.md`](AGENTS.md)。

## 本地开发

需要 Windows 11 x64、Node.js 20+、pnpm 10.33.0、Rust stable，以及 Tauri 2 的
Windows 前置依赖（MSVC 工具链与 WebView2）。完整检查与升级说明见
[`CONTRIBUTING.md`](CONTRIBUTING.md)。

```powershell
pnpm install --frozen-lockfile
pnpm vendor:verify
pnpm lint
pnpm typecheck
pnpm test
cargo check --manifest-path src-tauri/Cargo.toml
pnpm tauri dev
```

采集内核：**vendored ccusage v20.0.20 源码 + 固定 PR commit 的 Antigravity
downstream 移植**，编译进产品 EXE，并以同一 EXE 的隔离 worker 进程执行
（v0.3 起不再下载、打包或运行任何外部 ccusage 可执行文件）。Dashboard 与托盘
显示的数据来自本机真实用量，不使用 mock fixture。

`pnpm dev` 仅启动前端开发服务器；真实采集需使用 `pnpm tauri dev`。
`pnpm tauri build` 构建安装包。首次安装 npm/Cargo 依赖可能联网，但正常构建不会
下载 ccusage 源码、可执行文件或价格。

升级审计工具（可选）：`src-tauri/tests/shadow17.rs` 可将外部固定的 ccusage 构建通过
`CAM_SHADOW_SIDECAR_EXE` / `CAM_SHADOW_ANTIGRAVITY_EXE` 环境变量传入，与 worker
做逐字段 parity 对照；默认构建/测试不要求也不查找任何 sidecar，未提供变量时
对应 sidecar 对照会跳过，不应把默认测试通过当成重新完成 parity 验证。

## 参与贡献

提交改动前请阅读 [`CONTRIBUTING.md`](CONTRIBUTING.md)。安全问题请按
[`SECURITY.md`](SECURITY.md) 私下报告。开源发布准备与安装包门禁见
[`docs/OPEN_SOURCE_RELEASE_CHECKLIST.md`](docs/OPEN_SOURCE_RELEASE_CHECKLIST.md)。

## 许可证与品牌声明

项目代码采用 [MIT License](LICENSE)。vendored ccusage 源码、Antigravity 移植与
LiteLLM / models.dev 定价快照的版权归属和许可证见
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

Claude、Anthropic、OpenAI、Codex 及其他产品名称可能是其各自所有者的商标。
本项目为独立社区项目，与相关公司不存在隶属、认可或赞助关系；这些名称仅用于描述兼容性。
