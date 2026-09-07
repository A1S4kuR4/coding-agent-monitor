# 文档与实现一致性审查（2026-09-06）

审查基线：`edecd257ba8f32124a8e178d45e4524940b3463c`，开始时工作树干净。
本次只修改文档、Cargo 包描述与说明注释，核对产品代码、公共契约、构建配置、脚本、CI 和既有验收记录；
通过 GitHub CLI 只读核实 v0.3.0 发布元数据。未重新执行历史人工验收或修改远端发布。

## 已修正文档漂移

| 问题 | 实现或记录依据 | 修正位置 |
| --- | --- | --- |
| v0.3.0 仍标记待发布 | GitHub Release 已于 2026-09-01 发布，非 Draft、非 Pre-release | [README](../README.md)、[发布清单](OPEN_SOURCE_RELEASE_CHECKLIST.md)、[发布决策](V0.3_RELEASE_GATE_DECISION.md) |
| 校验示例仍用含空格资产名 | `SHA256SUMS.txt` 与 GitHub 资产名均使用 `Coding.Agent.Monitor`，digest 一致 | [Release Notes](V0.3_RELEASE_NOTES.md)、[发布决策](V0.3_RELEASE_GATE_DECISION.md) |
| 贡献指南要求已删除的 `fetch:sidecar` | `package.json` 无该命令；Tauri 无 `externalBin`，生产启动 `current_exe()` worker | [CONTRIBUTING](../CONTRIBUTING.md)、[README](../README.md)、[SECURITY](../SECURITY.md) |
| Agent 指南仍限定 Claude/Codex + 外部 sidecar | `AgentKind::ALL` 注册 17 个 Agent；`collector/ccusage.rs` 转换 vendor 输出，`normalize_snapshot` 组装公共契约 | [AGENTS](../AGENTS.md) |
| Cargo 包描述仍只列 Claude/Codex，依赖注释称生产 sidecar 路径不变 | `commands/usage.rs` 只调用 worker runner | [Cargo.toml](../src-tauri/Cargo.toml) 的描述和注释（未改依赖） |
| 总计划把已完成 Phase 10 列为下一任务，仍声称 manifest 为 0.1.0 | v0.2 验收已记录；三份 manifest 均为 0.3.0 | [总计划](IMPLEMENTATION_PLAN.md)、[v0.2 计划](V0.2_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md) |
| v0.3 计划仍为“待实施”，指向未创建的验收文件 | 各阶段已交付；发布验证实际记在 Phase 5 RC 与 Release Gate 文档 | [v0.3 计划](V0.3_DEVELOPMENT_AND_ACCEPTANCE_PLAN.md)、Phase 4A/4B 的后续状态提示 |
| README 仍以骨架和 Claude/Codex 简图概括现有 UI | `src/App.tsx` 有动态 Agent、模型展开、趋势筛选、较昨日变化、成本/缓存和 stale 恢复 | [README](../README.md) |
| 把 Agent 支持描述为从 3 个扩展到 17 个 | v0.2 发布源码 `d18c02c` 已用统一 `--by-agent` 报告和开放 Agent 列表；3 个仅是当时真实机器的活跃来源数 | [README](../README.md) |
| 支持范围不统一；含空格安装路径一行误标“中文/空格用户路径 PASS” | 最终 waiver 仅声明 Win11 x64；含空格 ASCII 安装目录不证明非 ASCII 用户 profile GUI | [CONTRIBUTING](../CONTRIBUTING.md)、[AGENTS](../AGENTS.md)、[Phase 5](V0.3_PHASE5_RELEASE_CANDIDATE.md) |

历史阶段的测试计数、hash、旧调用链和原始范围保留；新增状态说明指向后续记录。
waiver、NOT RUN 与 PASS 继续区分，不因版本已发布而补写技术验证结果。

## 需单独处理的代码维护观察

- `scripts/export-token-usage.mjs` 仍固定调用
  `src-tauri/target/release/ccusage.exe` / `ccusage-antigravity.exe`，且默认结束日期
  固定为 `2026-08-25`。v0.3 正常构建不再产生这些文件；该脚本不是可用的 v0.3
  导出入口。贡献指南已标明其历史用途。删除或迁移脚本需单独的代码任务，本次未运行
  该脚本，也没有导出真实用量。
- `.github/workflows/ci.yml` 当前运行前端 lint/typecheck/unit/build 和 Rust
  fmt/clippy/test，未包含 `vendor:verify` 或 Playwright。贡献指南列出的完整本地检查
  不等同于现有 CI 已自动覆盖；本次未修改工作流。

## 本次验证

- `pnpm vendor:verify`：**PASS**，源码清单、补丁、定价与单一 SQLite 链接校验通过。
- 文档引用检查：**PASS**，15 份新增/修改 Markdown 的 78 个相对链接及相关标题锚点
  均可解析；README/CONTRIBUTING 的 PowerShell 示例没有引用不存在的 pnpm script。
- `cargo metadata --manifest-path src-tauri/Cargo.toml --no-deps --format-version 1 --offline`：
  **PASS**，manifest 可解析；三份产品版本均为 `0.3.0`，Rust/npm 包描述一致。
- GitHub Release 元数据与本地 SHA-256 对照：**PASS**，证据见发布决策 §6。
- `git diff --check`：**PASS**。

本次没有运行前端 lint/typecheck/unit/E2E/build、Cargo check/test/clippy 或 Tauri
打包与人工 GUI 验收：运行时代码、依赖和公共契约未变，只修改文档与包说明。
历史发布验收结果不作为本轮测试结果。
