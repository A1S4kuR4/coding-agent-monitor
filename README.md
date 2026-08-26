# Coding Agent Monitor

一款面向 Windows 的轻量级 Coding Agent Token 使用量监控工具。

基于 **Tauri 2 + React + TypeScript + Rust + SQLite + ccusage** 构建，第一阶段主要支持 **Claude Code** 和 **OpenAI Codex**。

目标很简单：

> 不打开命令行，也能随时知道 Claude Code 和 Codex 今天用了多少 Token。

> **当前状态：v0.1.0 Release Candidate（预发布）**  
> 源码已完成主要自动化验证，但正式发布安装包前仍需关闭
> [非 ASCII Windows 用户目录的人工 GUI 验收门禁](docs/RELEASE_VERIFICATION.md#9-gate-0-chinese-profile-full-gui-re-checked-2026-08-25--not-complete--blocked-by-environment)。

## 隐私与安全

- 所有统计都在本机完成，程序不要求登录或云端账号。
- `ccusage` 以 `--offline` 方式运行；React 前端只接收聚合结果，不读取原始日志。
- 项目不包含遥测。真实 Token 导出、成本明细、日志和本机截图均被 Git 忽略。
- 请勿在 Issue 中上传真实 Agent 日志、用量导出、数据库或包含隐私信息的截图。

---

## 核心功能

### 1. 自动统计 Coding Agent Token

启动后自动检测本机已有的 Coding Agent 数据，例如：

```text
Claude Code    8.42M Tokens
Codex          5.17M Tokens

今日合计      13.59M Tokens
```

第一阶段支持：

- Claude Code
- OpenAI Codex

底层优先复用 `ccusage` 已有的数据解析能力，避免重复实现不同 Agent 的日志解析逻辑。

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

第一版只关注：

- 今日 Token
- Claude Code / Codex 使用占比
- 最近 7 天使用趋势

不设计复杂 BI Dashboard。

---

### 3. Windows 系统托盘

程序可以长期运行在 Windows 系统托盘中。

无需一直打开主窗口。

点击托盘图标即可快速查看：

```text
Coding Agent Monitor
────────────────────

今日          13.59M

Claude Code    8.42M
Codex          5.17M

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
Claude Code
    │
    ├────────┐
    │        │
Codex       │
    │        │
    └────────▼
           ccusage
              │
              ▼
        JSON Adapter
              │
              ▼
           SQLite
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
| Usage Parser | ccusage |
| Platform | Windows 10 / 11 |

---

# 为什么使用 ccusage

`ccusage` 已经能够解析 Claude Code、Codex 等 Coding Agent 的本地使用记录。

因此 Coding Agent Monitor 第一阶段不重复实现这些 Parser，而是：

```text
ccusage
   ↓
JSON
   ↓
Coding Agent Monitor
```

应用内部再统一转换为自己的数据结构。

这样可以把开发重点放在：

- Windows 桌面体验
- 系统托盘
- 轻量可视化

而不是重新解决 Agent 日志解析问题。

---

# MVP

第一版只实现三个核心功能，当前均已完成：

- [x] 自动检测并统计 Claude Code / Codex Token
- [x] 今日用量 + 最近 7 天趋势可视化
- [x] Windows 系统托盘快速查看

其他功能暂不进入 MVP。

---

# v0.2 后续开发方向

在完成 v0.1.0 的中文用户路径发布验收后，v0.2 按以下顺序推进：

1. 将两次 focused sidecar 调用收敛为一次
   `ccusage daily --json --offline --by-agent` 统一调用。
2. 将固定的 Claude Code / Codex 数据契约改为动态 Agent 契约。
3. 增加今日预估成本、缓存输入占比、最后更新时间和刷新失败降级。
4. 在不增加路由、卡片堆叠和外部字体依赖的前提下，采用轻量的
   Reading Surface 视觉语言并完成紧凑视口适配。

统一命令的目标是减少外部进程数量、统一快照和消除 Agent 命令硬编码；
它不承诺扫描耗时或系统 I/O 减半。模型明细、月度统计、Session 分析、
Burn Rate、提醒和复杂设置仍不进入 v0.2。

完整的开发任务、数据契约、测试矩阵与发布门禁见
[`docs/V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md`](docs/V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md)。

---

# 项目状态

> v0.1.0 Release Candidate / v0.2 Phase 6、Phase 7、Phase 8、Phase 9 已完成

`src-tauri/src/sidecar` 已从两次 focused（Claude、Codex）ccusage 调用收敛为一次
`ccusage daily --json --offline --by-agent` 统一快照（v0.2 Phase 6，2026-08-25）。
随后落成动态 Agent 契约（v0.2 Phase 7，2026-08-25）：封闭的 Claude/Codex 枚举与前端
固定 label map 移除，改为开放字符串 `id` + Rust 产出的 `displayName`，每个日期按实际活跃
Agent 动态聚合、稳定排序并对未知 Agent 做 title-case fallback；今日列表与托盘摘要随之动态。
Claude/Codex 显示名与数值不回退。

再落成三项轻量信息（v0.2 Phase 8，2026-08-25）：Rust adapter 在日级聚合出可空预估成本
`estimatedCostUsd` 与缓存输入占比 `cacheReadShare`（缺失值不伪装为 `$0.00`、零分母为不可用），
runner 在成功采集后打上 `collectedAt`；Dashboard 主值近旁显示 `Est. cost $X`、次级元数据显示
`~Y% cached input`、页脚显示 `Updated Xm ago`（60s 只刷新相对文案），托盘刷新成功后向窗口推送同一
快照事件（`usage-updated`），刷新失败保留旧数据并显示 stale 状态与重试；per-agent token 构成只参与
聚合不下发前端（§3 建议契约的经确认偏差，见 `V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md §10`）。

再完成纯展示层重构（v0.2 Phase 9，2026-08-25）：仅改动 `src/App.css`，抽出 paper/ink/Terracotta/
Indigo/Moss/hairline/spacing CSS tokens，建立系统 Serif/Sans/Mono 三轨字体栈（无外部字体请求），
今日核心值与主操作用暖 Terracotta、趋势条用冷 Indigo、Moss 仅作成功态预留；为低高度增加 compact
media query（680×700 目标无滚动条）、`:focus-visible` Indigo focus ring、reduced-motion，200% 文本
缩放与高 Agent 数下自然滚动不裁切（未用 `overflow:hidden` 掩盖）。数据、刷新与托盘行为不变。

v0.1.0 唯一剩余的中文 Windows 用户路径 GUI 验收（Gate 0）仍未关闭、须在发布前由人工完成；
Phase 6/7/8/9 按用户明确指示先于该门禁实施。下一阶段是 v0.2 Phase 10（v0.2 发布验证；
结果写入新的 `docs/V0.2_RELEASE_VERIFICATION.md`）。

Phase 7–9 审校发现的 8 项缺陷已修复（v0.2 审校修复，2026-08-25）：420×560 / 高 DPI 横向溢出与裁切、
暗色主按钮与 stale Retry 的 WCAG AA 对比、Rust u64→JS 安全整数策略显式校验（超 `Number.MAX_SAFE_INTEGER`
返回稳定错误而非静默丢精度）、focus/托盘事件的异步注册泄漏与卸载后 setState、统一日级 `totalTokens` 与
`agents[]` 求和的校验（不一致返回稳定错误而非静默显示 0）、0 token 日趋势条高度归 0、核心统计走 Mono 轨道，
并补齐 Phase 8 必需的组件/状态测试（状态机抽为纯 `viewReducer`）。前端 lint/typecheck/test/build 与
Rust fmt/clippy/check/test 全部通过。实际命令结果、未运行的真机 GUI 项与仍开放的门禁见
`docs/V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md §12`。

二次审校的 2 项 P2 也已闭环（§12.6）：重复日期的 `totalTokens` 校验按**逐行**（而非首见保底）与其自身
`agents[]` 合计比对，再做跨行聚合，新增对应单测；并为 focus/托盘监听补上"注册 Promise 晚于卸载 resolve"的
竞态测试（mock 支持延迟注册）。前端 7 文件/32 测试、Rust 26 passed + 1 ignored、lint/typecheck/build、
fmt/clippy 均通过；真实数据 smoke 亦通过。

Antigravity 数据缺口随后以本地固定兼容桥闭环：官方 ccusage 20.0.20 继续提供统一快照，另行固定
上游 PR #1487 commit `c58c1b3aab2eacc82add250c8229bb6192e4489b`，仅运行 focused
`antigravity daily`；Rust 在 sidecar adapter 边界合并两者，并在官方统一报告未来原生出现
Antigravity 时按日期优先采用官方行，避免重复统计。这样不把整套 ccusage 回退到 PR 的 20.0.18 基线。

**先做好一个真正愿意长期放在 Windows 托盘里的 Coding Agent Token Monitor。**

不追求功能多。

优先把启动速度、资源占用、数据准确性和使用体验做好。

---

# 当前实现

仓库现已包含 Tauri 2 + React + TypeScript + Rust 的最小可运行骨架：

- React 只消费项目自己的 `UsageSummary`，不解析 ccusage 原始 JSON。
- Rust 负责 ccusage adapter、SQLite 初始化、系统托盘和本地错误边界。
- SQLite 当前只初始化本地数据库文件，不创建业务表。
- Dashboard、系统托盘与安装包均已接通**真实 ccusage sidecar**：官方 20.0.20 读取统一
  Agent 用量，固定 PR commit 的兼容 sidecar 补充 Antigravity；不再使用 mock data fixture。发布构建、安装/启动/卸载的
  验收记录见 [`docs/RELEASE_VERIFICATION.md`](docs/RELEASE_VERIFICATION.md)。

详细的后续实施顺序与验收条件见
[`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md)，开发约束见
[`AGENTS.md`](AGENTS.md)。

## 本地开发

需要 Node.js、pnpm、Rust stable，以及 Tauri 2 的 Windows 前置依赖。

```powershell
pnpm install --frozen-lockfile
pnpm fetch:sidecar
pnpm lint
pnpm typecheck
pnpm test
cargo check --manifest-path src-tauri/Cargo.toml
pnpm tauri dev
```

当前 ccusage 状态：**官方 20.0.20 unified + 固定 PR commit 的 Antigravity focused
兼容桥**。Dashboard 与托盘显示的数据来自本机真实用量，不再使用 mock fixture。

`pnpm fetch:sidecar` 会按固定版本、提交和哈希下载或构建两个本地 sidecar；生成的
EXE 不进入 Git。详细供应链说明见
[`src-tauri/binaries/README.md`](src-tauri/binaries/README.md)。

## 参与贡献

提交改动前请阅读 [`CONTRIBUTING.md`](CONTRIBUTING.md)。安全问题请按
[`SECURITY.md`](SECURITY.md) 私下报告。开源发布准备与安装包门禁见
[`docs/OPEN_SOURCE_RELEASE_CHECKLIST.md`](docs/OPEN_SOURCE_RELEASE_CHECKLIST.md)。

## 许可证与品牌声明

项目代码采用 [MIT License](LICENSE)。`ccusage` sidecar 与 LiteLLM 定价快照的
版权归属和许可证见 [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)。

Claude、Anthropic、OpenAI、Codex 及其他产品名称可能是其各自所有者的商标。
本项目为独立社区项目，与相关公司不存在隶属、认可或赞助关系；这些名称仅用于描述兼容性。
