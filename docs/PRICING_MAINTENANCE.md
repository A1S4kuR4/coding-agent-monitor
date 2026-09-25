# 内置模型价格的获取与维护

更新记录：2026-09-25。此文档适用于 v0.5.1；已发布安装包的价格不会自动改变。

## 选择

CAM 继续使用**随安装包固定的离线价格快照**。运行时不请求定价服务，也不读取账户价格或账单。估算的是按模型公开 API 单价计算的参考成本；订阅、平台积分、折扣、地域、Fast/Batch 等实际结算规则可能不同。没有可确认价格的模型继续返回成本不可用，不把缺价写成零。

| 来源 | 用途 | 维护判断 |
| --- | --- | --- |
| 服务商官方价格页 | 核对主流模型、缓存和长上下文费率 | 最高优先级，但没有覆盖所有 Agent/模型的统一机器可读表 |
| [LiteLLM 模型价格数据](https://github.com/BerriAI/litellm/blob/main/model_prices_and_context_window.json) | ccusage 的主要内置价格来源 | 广、可按提交固定；字段及别名需复核 |
| [models.dev](https://github.com/anomalyco/models.dev) 经 ccusage 生成的快照 | 补足 LiteLLM 未覆盖的模型及分段价格 | 使用同一 ccusage 提交的 catalog rules；避免把按图像/秒收费的价格当作 Token 单价 |

上述选择基于现有 ccusage 定价流程，避免 CAM 再维护一套独立计算器。两份社区数据都可能滞后或有错误，所以价格变更应作为一次可审查的数据更新，而不是应用启动时静默联网覆盖。

## 本次补充

- 解析器版本保持 ccusage v20.0.20。LiteLLM 数据固定在提交 `2dccc0dc79143043889bfaf2a9ecb315e5b197e8`，models.dev 派生快照及 catalog rules 固定在 ccusage 提交 `60377f71a96b185be209ef8ad1d7725944a6486a`。原始文件与合并后的 SHA-256 见 [`pricing-manifest.json`](../src-tauri/vendor/ccusage/pricing/pricing-manifest.json)。
- 新快照的同名条目优先；旧快照独有的模型 ID 保留，避免旧日志突然缺价。LiteLLM 表从 3,039 增至 4,509 项，models.dev 表从 2,275 增至 3,164 项。新版重新计算历史数据时会使用新版单价，因此它仍不是历史账单。
- 特别复核了 [OpenAI GPT‑5.6 Sol 的降价及长上下文倍率](https://developers.openai.com/api/docs/models/gpt-5.6-sol) 和 [Anthropic Claude Opus 5 Fast 价格](https://platform.claude.com/docs/en/build-with-claude/fast-mode)。相关定价断言随快照更新；未知的 Antigravity 内部模型变体仍不得凭名称自行编造价格。

## 后续更新流程

1. 在计划发布前检查 ccusage、LiteLLM、models.dev 的定价更新；优先核对实际活跃模型。价格可能随时变化，建议每月至少检查一次，但只有通过审查后才发布新表。
2. 固定上游完整提交 SHA 和下载文件 SHA-256。更新 `scripts/vendor-ccusage-import.mjs` 的 `PRICING_REFRESH`，按相同提交取 models.dev 快照与 catalog rules；绝不在构建时抓取浮动的 `main`。
3. 生成两张合并表及清单，检查新增、移除、同名改价和模型别名；对主流模型逐项对照服务商页面，尤其是缓存读写、长上下文、地域与 Fast 费率。按资产计价或无可靠单价的模型不加入估算。
4. 运行 `cargo test --manifest-path src-tauri/vendor/ccusage/rust/Cargo.toml`、`cargo test --manifest-path src-tauri/Cargo.toml` 和 `pnpm vendor:verify`。`scripts/vendor-ccusage-import.mjs --check-staged` 可在提交前确认固定来源可重建；正常构建保持完全离线。
5. 在价格清单和 `THIRD_PARTY_NOTICES.md` 记录来源与许可证；发布说明注明价格快照日期。必要时增加针对实际缺价模型的测试，不把 Token 记录与平台账单不一致误报成采集失败。

当前不足：快照只给出“现在这版软件采用的参考价”，没有按每条记录时间选历史费率；本地日志也未必包含套餐、区域、缓存时长或全部计费维度。若将来需要账单级对账，应另立任务，不能仅靠扩充价格表推断。
