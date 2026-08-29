# Coding Agent Monitor

一款面向 Windows 的轻量级 Coding Agent Token 使用量监控工具。

基于 **Tauri 2 + React + TypeScript + Rust + SQLite + ccusage** 构建，通过动态 Agent
契约展示官方 ccusage 统一快照，并用固定兼容桥补充 Antigravity。

目标很简单：

> 不打开命令行，也能随时查看本机 Coding Agent 今天用了多少 Token。

> **当前状态：v0.2.0 已发布**
> 非 ASCII Windows 用户目录下的完整人工 GUI Gate 0 对 v0.1.0 与 v0.2.0 均由维护者标记为
> **WAIVED / NOT RUN**，不是 PASS。该场景仍是已知覆盖缺口；详见
> [v0.2 发布验证记录](docs/V0.2_RELEASE_VERIFICATION.md)。发布安装包仍未 Authenticode 签名，
> 安装时可能出现 SmartScreen/未知发布者提示，请核对 Release 附带的 SHA-256 校验值。

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

当前用户可见支持范围：

- 官方固定 ccusage 20.0.20 二进制提供的统一 Agent 快照；
- 开放字符串 ID + Rust `displayName` 的动态 Agent 列表，未知 Agent 可安全显示；
- Claude Code 与 OpenAI Codex 的既有统计；
- 固定上游提交构建的 Antigravity focused 兼容 sidecar。

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
Local Agent Records
        │
        ├── official pinned ccusage unified snapshot
        └── pinned Antigravity compatibility bridge
              │
              ▼
          Rust Adapter
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

`ccusage` 已经能够解析多种 Coding Agent 的本地使用记录。

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

- [x] 自动检测并统计动态 Coding Agent Token
- [x] 今日用量 + 最近 7 天趋势可视化
- [x] Windows 系统托盘快速查看

其他功能暂不进入 MVP。

---

# 项目状态

> v0.2.0 已发布；manifest 版本为 `0.2.0`。

内部规划中的 Phase 6–9 已实施：统一官方 ccusage 快照、动态 Agent 契约、预估成本/
缓存占比/更新时间与 stale 降级，以及 Reading Surface 响应式精修。这里的阶段名称是内部规划记录。
Antigravity 由固定上游提交构建的 focused sidecar 补充，
Rust 会与官方统一报告合并并避免重复统计。

Phase 10 已于 2026-08-28 执行完毕，记录见
[v0.2 发布验证记录](docs/V0.2_RELEASE_VERIFICATION.md)；v0.2.0 已标记为 release candidate 并按
[开源发布清单](docs/OPEN_SOURCE_RELEASE_CHECKLIST.md) 发布。详细阶段历史、
测试结果和未覆盖项见 [实施计划](docs/IMPLEMENTATION_PLAN.md) 与
[v0.2 开发验收计划](docs/V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md)。

非 ASCII Windows 用户目录完整 GUI Gate 0 对 v0.1.0 与 v0.2.0 均为
**WAIVED / NOT RUN**，不是技术验证通过。发布 MSI、NSIS、主程序和两个 sidecar 的
Authenticode 状态均为 `NotSigned`，Release Notes 已披露 SmartScreen/未知发布者风险。

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
