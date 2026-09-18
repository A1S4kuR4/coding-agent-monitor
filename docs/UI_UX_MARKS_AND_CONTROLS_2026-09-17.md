# 印记与控件语言:去"AI 味"设计附录(2026-09-17)

依附文档:`UI_UX_MAIN_REVIEW_2026-09-17.md`(主界面 UI/UX 评审)。
适用范围:`src/App.tsx` / `src/App.css` 中所有 agent 身份标记与胶囊(pill)控件。
素材:`src/assets/agent-marks/*.svg`;演示:`src/prototypes/optimized-main.html`(优化版已应用,当前版保留原样做 A/B 对照)。
方法:按 dual-surface-frontend-design 的不变量执行——主要命中第 6 条(层级来自排版与留白,拒绝 pill 森林)与第 2 条(颜色角色分工)。

---

## 1. 问题定性

两处"AI 味"来源:

1. **色环 + 字母缩写图标**(`AgentMark`,App.tsx:154-164):17px 彩色圆环内套两个 mono 字母(CL/CO/AN)。这是 LLM 生成界面的默认头像模式——圆环不承担信息,字母是名字的重复,彩色圆点身份在截图里尤为显眼。
2. **胶囊森林**:筛选 chips(App.css:1086-1099)、7/30 天范围(App.css:1251-1270)、"更多 agent"(App.tsx:1063-1115)全部 `border-radius: 999px`。一个界面里超过三个 pill 就会读成"生成式 dashboard 模板",且范围控件与筛选控件视觉同构,语义层级被抹平(评审 A1 已指出)。

## 2. 印记(sigil)设计

替换原则:**依各 agent 官网官方图标重绘为单笔画描边小标**,currentColor 继承 agent 色;无环、无字母。形状与官方标识保持可辨识的亲缘关系,但以本应用的描边语言统一(16 viewBox、round cap):

| Agent | 官方出处(2026-09-17 核实) | 印记 |
|---|---|---|
| Claude Code | claude.ai favicon / 产品图标:珊瑚色火花(spark) | 十条锥形放射臂,长短相间 |
| Codex | Codex CLI 官方启动画面标识:`>_` 终端提示符 | `›` + 光标横线 |
| Antigravity | antigravity.google 官方标志:彩虹渐变拱形带 | 圆角拱形 ∩(单色描边) |
| OpenCode | opencode 官方 logo-ornate SVG:像素块字标 | 方块 "o" 外框 + 下部实心内块 |
| 未识别 | (无官方对象) | 菱形轮廓,中性占位 |
| others 聚合 | (应用内部概念) | 加号 |

核实方式:WebSearch 不可用,改为直接抓取官网与官方仓库——`claude.ai/favicon.ico`、`openai/codex` 仓库 `.github/codex-cli-splash.png`、`antigravity.google/assets/image/antigravity-logo.png`、`sst/opencode` 仓库 `logo-ornate-dark.svg`。Anthropic 官网 webclip 是公司 "AI" 字标而非产品图标,故 Claude Code 取 claude.ai 产品火花。

技术约定(与现有令牌体系兼容):

- `viewBox="0 0 16 16"`,`stroke="currentColor"`(opencode 内块为 `fill="currentColor"`),组件侧用 `--mark-color: var(--agent-*)` 着色,色值仍只存在于 CSS 令牌,不进组件(App.css:42-52 的约束不变)。
- 笔宽按官方形状的视觉密度微调(spark 1.7 / `>_` 1.6 / 拱形 1.8),在 15px 渲染尺寸下视觉重量一致。
- 渲染尺寸 15px,与正文基线对齐;装饰性 `aria-hidden`,名称仍是可访问标签(与现行做法一致)。
- 六个印记剪影互相可区分:放射星 / 水平角线 / 垂直拱形 / 方块 / 菱形 / 十字——小尺寸下不靠颜色也能分辨。

## 3. 控件语言:下划线 tab 取代胶囊

**筛选(身份类)**:文本 tab——`[印记] 名称`,常规态 ink-3,hover ink-2,激活态全墨色 + 2px agent 色下划线;"全部"激活下划线用墨色。无 border、无背景、无底衬。身份色只出现在印记与激活下划线两处,一处都不多。

**范围/聚合(位置类)**:mono kicker tab(大写、0.08em 字距、10.5px),激活下划线用 indigo。依据颜色角色分工:indigo 的职责是"当前位置/焦点",范围控件正是在标记"你现在看的是哪一段";agent 色的职责是身份,两者不混。

**次级动作**(展开明细、关于、设置入口):下划线文本动作。维持动作层级二元——全窗口只有一个实心主按钮族(重试/确认类 terracotta 填充),其余皆为文本或细线按钮。

**触控与键盘**:tab 保持 `min-height: 30px` 可点区域(去胶囊不等于缩目标);`aria-pressed` 语义不变;激活态不依赖颜色单通道(下划线位置 + 文字明度双重编码);下划线过渡 120ms,已被全局 `prefers-reduced-motion` 毯式规则覆盖。

## 4. 与既有设计约束的对账(不冲突清单)

- **delta 中性**(App.css:60-63):不触碰。
- **成本三态**:不触碰。
- **Refresh 中性次级按钮**(App.css:742-784):本来就是细线矩形,保留。
- **选中日的 paper-track 底衬 + inset 环**(App.css:544-548):这是"被钉住的状态"不是控件装饰,保留。
- **day-detail / quota 卡片的边界**:属于"确需边界的内容"(钉住的披露面板、独立功能模块),符合"卡片仅用于真正需要边界处"的条款,保留。
- **agent-claude 橙粉与 terracotta 主按钮同暖族**:当前界面里主按钮极稀有(仅错误重试/首次关闭通知),不与 agent 行同屏出现;若未来在浅色面同屏,按"一个小组件内不出现两个等强 accent"复核。

## 5. 落地指引(App.tsx / App.css)

1. `AgentMark` 组件(App.tsx:154-164)改为渲染对应 SVG(`aria-hidden`),`agents.ts` 的 `agentMeta` 增加 `sigil` 字段;`agentMark()` 字母函数随之退役。注意 `AgentMark` 有四处调用:agent 行、筛选 chips、tooltip 图例、日详情——一并替换,保持"同一 agent 处处同相"。
2. `.filter-chip` / `.history-controls button` 的 pill 规则替换为 tab 规则(原型 `.filter-tab` / `.view-opt .seg` 可直接移植);"更多 agent" 溢出入口改为文本动作 `更多 (n) ▸`,菜单面板本身不变。
3. 移除 chip 的 `--chip-soft` 20% 底衬令牌用途(激活态不再有底色);`--agent-*-soft` 令牌可保留观察是否有他用。
4. 对比度:tab 常规态文字 ink-3(#a8a69c on #191612,约 7:1)满足 AA;12.5px 小字建议用 ink-2 作 hover 后的中间档,勿再降。
5. 测试:`contrast.test.ts` 涉及 chip 激活底色的断言需要更新;`App.test.tsx` 中断言 chip class 名的用例同步改名。

## 6. 未做之事(明确边界)

- 未改动浅色面令牌;浅色面(cream + terracotta)本身是另一处潜在"模板感"来源,如需处理应单独立项评审。
- 未改动图表柱形、tooltip、数据语义与任何行为逻辑。
- 印记为官方图标的**描边化重绘**(shape 亲缘、非原矢量复制),单色、15px、用于本机用量统计的指示性用途;官方标志的商标权仍归各厂商,若未来收到品牌指引要求,替换为官方资产或回退为中性图形即可。
