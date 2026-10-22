# AI / CFR 移植规格（v1.5.8 → Rust poker-ai）

> 状态：Phase 4 预研交付物（规格 + 黄金语料），尚未实现 `poker-ai`。
> 权威源：`reference/texas-holdem-game` @ `e0c6c6f88d3e64324e1d83bfc700dd3a77f71b90`（v1.5.8，2026-10-08）。
> 作者：ai-analyst；日期：2026-10-23。
> 配套文件：`docs/specs/ai-strategy-notes.md`（行号导航 / RNG 消耗顺序 / 覆盖矩阵 / 复现命令）、`oracle/golden-ai.js`、`tests/golden/ai/`。
> 本文所有常量、阈值、权重都标注了原文件行号；实现 Rust 时按本文 + 黄金语料即可，不需要再读 JS 全文（但必须能对照原文）。

## 0. 阅读指南

1. **先读 §1-§3**：确定哪份代码是生产权威路径。原项目有 `ai-core.js`、`ai-shortdeck-stable.js`、`ai.js` 三套 AI 代码，其中只有 **ai-core（标准）/ ai-shortdeck-stable（短牌）的 `UnifiedPokerAI.decide()` 是生产决策路径**；`ai.js` 的 `AIPlayer` 是适配器（组装 context、调用 shared brain、把结果 legalize）。
2. **§4-§7 是实现主体**：配置表、数据结构与更新公式、决策流水线、RNG 与确定性。
3. **§8 是 River CFR**，独立于通用流水线，但默认在目标配置（标准十人桌 5000/100/200）启用。
4. **§9 是短牌**：短牌不共享标准策略参数，使用 v1.5.6 冻结的稳定核心。
5. **§10 预算、§11 黄金语料、§12 风险、§13 工作量拆分、§14 未决问题**。
6. 附录 A 是常量总索引（按文件行号）；写代码时先建 §4 的常量表，用 RNG 测试向量（§7.1）锁定随机数实现。

本文对“原文件:行号”的引用均相对 `reference/texas-holdem-game/` 根目录。

## 1. 权威源与版本

| 项 | 值 | 来源 |
| --- | --- | --- |
| 原项目版本 | v1.5.8 | `AI_BENCHMARK_REPORT_v1.5.8.md:1` |
| Git commit | `e0c6c6f88d3e64324e1d83bfc700dd3a77f71b90` | 仓库 HEAD |
| 策略版本串 | `unified-v2`（写进 trace） | `js/ai-core.js:1688`、`js/ai-shortdeck-stable.js:1282` |
| 决策算法 | 公开范围加权权益 + 解析 EV + 有界混合；目标配置叠加公开范围 River CFR | `AI_BENCHMARK_REPORT_v1.5.8.md:20-28` |
| 短牌算法 | v1.5.6 冻结的 stable short-deck 策略（无 CFR） | `js/ai-core.js:1044-1046`、`js/ai-shortdeck-stable.js` 全文 |

**关键事实（容易读错的地方）：**

1. `js/ai-core.js` 构造函数在 `options.variant === 'shortdeck'` 时**直接返回** `new stableShortdeck.UnifiedPokerAI(options)`（`js/ai-core.js:1044-1046`）。因此 ai-core 里针对 shortdeck 的代码（如 `VARIANT_PROFILE_OVERRIDES.shortdeck` 为空）实际不会执行；短牌全部走 `ai-shortdeck-stable.js`。
2. `VARIANT_PROFILE_OVERRIDES.standard` 包含 `riverCfrWeight:1`（`js/ai-core.js:73`），而 `DEFAULT_PROFILE.riverCfrWeight` 是 0（`js/ai-core.js:59`）。**标准模式默认启用河牌 CFR**，但还需 `strategyFocus === 'ten-player-25bb'` 且通过 §8.1 的门控。
3. 生产路径的“评估器”由 adapter 注入（local 用 `fallbackHandEvaluator` 的 `probability-core.evaluate`；server 用 `ProbabilityCore.evaluate`；worker 用 `PokerProbabilityCore.evaluate`）。Rust 端必须用 `poker-core::evaluate`，且短牌必须传 `Variant::ShortDeck`。
4. `AIPlayer.decide()` 的 legacy 分支（`js/ai.js:369-828`）只在 `this.sharedBrain == null` 时执行；生产不会发生。**Rust 不需要移植 legacy 分支**，但需要保留 `AIPlayer` 的 adapter 语义（§3.2）。

## 2. 模块地图

| 原文件 | 行数 | 作用 | Rust 归属 |
| --- | ---: | --- | --- |
| `js/ai-core.js` | 1749 | 标准模式统一 AI 核心：配置、belief、model、权益、候选、混合、trace、River CFR 适配 | `poker-ai`（核心主体） |
| `js/ai-shortdeck-stable.js` | 1339 | v1.5.6 冻结短牌策略（自身是 ai-core 的独立分支副本） | `poker-ai::shortdeck` |
| `js/ai.js` | 1056 | `AIPlayer` 适配器 + 6 人格配置 + legacy 决策路径 + worker/服务端入口 | `poker-ai`（adapter/profile） |
| `js/river-solver.js` | 51 | 公开范围抽象 + 河牌有限树 + 调用编译 CFR + 根节点收益 | `poker-ai::river` |
| `js/compiled-cfr.js` | 45 | 有限树 CFR 编译为 typed array + 迭代 | `poker-ai::cfr` |
| `js/ai-worker.js` | 52 | Web Worker 侧 brain 池 | Rust 线程/任务封装（不需要移植） |
| `js/ai-worker-client.js` | 98 | 主线程 worker 客户端、2500ms 超时、失败回退 | Rust 异步封装/超时策略 |
| `server/server-game.js` | 675 | 服务端全部 AI bot 驱动：`chooseServerAiAction`、观察广播、摊牌学习 | `poker-server` + engine 契约 |
| `js/game.js` / `js/shortdeck.js` | 776 / 999 | 本地游戏构造 AI context（字段清单见 §5） | engine/app 契约 |
| `js/game-rules-core.js` | 87 | `wagerUnit` / `raiseBounds` / `snapRaise` / `decisionIdentity` / `buildActionMeta` | `poker-engine`（已有/待对齐） |
| `js/probability-core.js` | 318 | `evaluate`（权威牌力评估） | `poker-core`（已实现） |

## 3. 调用链与三条运行路径

### 3.1 生产调用链（标准模式）

~~~text
engine/game 构造 gameState（公开信息） --> AIPlayer.decide(gameState)      [js/ai.js:369]
   --> AIPlayer._decideShared(gameState)                                  [js/ai.js:925-965]
        |-- 注入 holeCards / heroRoundBet / heroStack / legalActions(对象) / evaluateCards
        +--> UnifiedPokerAI.decide(rawContext)                             [js/ai-core.js:1627]
             |-- normalizedContext(raw)                                     [js/ai-core.js:836]
             |-- strategyFocus 判定 + profile 选择 + 实验开关注入           [js/ai-core.js:1629-1644]
             |-- beginHand（handId 变化时）                                 [js/ai-core.js:1645]
             |-- SeededRng(seed) + activeReads                             [js/ai-core.js:1646-1651]
             |-- preflopDecision(...)  或  翻后：estimateRangeEquity --> analyzeTexture
             |     --> buildPostflopCandidates --> selectCandidate          [js/ai-core.js:1653-1659]
             |-- solveRiver（目标配置、河牌单挑、门控通过时覆盖所选动作）   [js/ai-core.js:1660-1661, 1109]
             |-- legalize（动作/金额落到合法集合与加注网格）                [js/ai-core.js:1600-1626]
             |-- HandPlan.update + SelfImage.record（跨决策状态）            [js/ai-core.js:1666-1667]
             +-- 组装 lastTrace（§6.9）并返回 {action, amount}
   --> AIPlayer._act() 返回 {action, amount}                              [js/ai.js:967]
~~~

### 3.2 三条运行路径（必须保持语义一致）

| 路径 | 入口 | brain 创建 | 评估器注入 | 观察事件来源 |
| --- | --- | --- | --- | --- |
| 本地桌面（主线程回退） | `js/game.js:546-548`、`js/shortdeck.js:854-856` | `new AIPlayer(name, style, stack, index, variant)` `js/ai.js:341-363` | `_decideShared` --> `fallbackHandEvaluator` `js/ai.js:28-51` | `engine.notifyAIsOfAction` `js/engine.js:458-491` |
| 本地桌面（Worker） | `aiWorkerService.decide` `js/ai-worker-client.js:69-86` | worker 内按 `meta.profile/style/variant` 重建 brain `js/ai-worker.js:7-21` | worker 注入 `PokerProbabilityCore.evaluate` `js/ai-worker.js:43` | 主线程经 `aiWorkerService.observe` 转发（`js/ai.js:842-860`） |
| 联机服务端 | `chooseServerAiAction` `server/server-game.js:592-649` | `createServerAi` --> `new AIPlayer(..., variant)` `server/server-game.js:651-673` | 注入 `ProbabilityCore.evaluate` `server/server-game.js:647` | `notifyAIsOfAction` `server/server-game.js:351-380` |

三条路径的共同要求（Rust 移植硬约束）：

1. 决策输入只能包含**公开信息 + 自己的底牌**（§8.1 信息边界）。
2. 同一个 `meta.profile` 必须产生同样的 `variantProfiles`（合并顺序见 §4.1）。
3. 观察事件的 `meta` 字段必须由同一套 `buildActionMeta` 语义生成（§5.2），否则 belief/response 会漂移。
4. 固定 `gameState.seed / handId / decisionId / seatId / style` 时，三条路径的决策必须一致。

### 3.3 适配器状态（AIPlayer 需要维护什么）

| 字段 | 初值 | 用途 | 原文件 |
| --- | --- | --- | --- |
| `name/style/config` | 构造参数；`config = STYLE_CONFIGS[style]` | 传给 brain 的 `options.profile` | `js/ai.js:343` |
| `stack/holeCards/chipsInPot/folded/isAllIn/hasActed/lastAction/lastDecisionTrace` | — | 决策输入与 UI 读取 | `js/ai.js:344-361` |
| `position/numPlayers/bigBlind` | 0 / 6 / 80；决策前由游戏改写 | legacy 路径与 context 组装 | `js/ai.js:346-348` |
| `isShortDeck` | `variant==='shortdeck'` | 短牌评估/短牌 brain | `js/ai.js:348` |
| `sharedBrain` | `new UnifiedPokerAI({name,style,seatId:index,profile:this.config,variant})` | 真正决策 | `js/ai.js:354-360` |
| `opponentRanges/playerModels` | `{}` | **legacy 路径专用**；shared 路径不读 | `js/ai.js:349, 834-860` |
| `_cachedEquity/_cacheKey/streetPlan` | null/''/null | legacy 路径专用 | `js/ai.js:351-353` |
| `workerKey` | `ai-<seq>-<index>-<name>` | worker 路由 key | `js/ai.js:362` |

> Rust 建议：`AIPlayer` 等价类型只保留 `style/profile/variant/holeCards/stack/chipsInPot/position/numPlayers/bigBlind/is_short_deck/shared_brain/last_decision_trace`；legacy 字段与逻辑**不移植**，但在文档和测试中保留“为什么不需要”的说明。

## 4. 配置系统：人格、风格与有效参数

### 4.1 有效 profile 的合并公式

~~~text
variantProfiles[variant] = { ...DEFAULT_PROFILE,
                             ...PROFILE_OVERRIDES[style]（若存在）,
                             ...VARIANT_PROFILE_OVERRIDES[variant]（若存在）,
                             ...options.profile（adapter 传入，即 AIPlayer.config） }
~~~

- 构造：`js/ai-core.js:1050-1058`；`Object.freeze` 后按 variant 缓存。
- `options.profile` 优先级最高，因此 `STYLE_CONFIGS[style]` 里的 `vpip/pfr/aggression/...` 会覆盖 `PROFILE_OVERRIDES` 的同名字段；未被 STYLE_CONFIGS 覆盖的字段保留 DEFAULT/OVERRIDE 值。
- `targetProfiles`（`js/ai-core.js:1059-1064`）= variantProfiles 再叠加 `COMPETENT_STYLE_OVERRIDES`，但仅在 `base.competenceWeight > 0` 时；默认 `competenceWeight=0`，所以 **targetProfiles 与 variantProfiles 完全一致**（生产上等价）。
- `professionalWeight > 0` 时把 variantProfiles 向 `PROFESSIONAL_PROFILES` 线性插值（`js/ai-core.js:1052-1056`）；默认 0，**不启用**。
- `decide()` 每次按 `strategyFocus` 选择 `this.profile`（`js/ai-core.js:1630`）：
  - `ten-player-25bb` --> `targetProfiles[variant]`
  - 其他 --> `variantProfiles[variant]`
  - 由于 target 恒等于 variant，实际差别仅在于 `decide` 后续对 context 注入的实验权重（全部默认 0，见 §4.7）。

**对 Rust 的建议**：把 `DEFAULT_PROFILE` 建模为 `AiProfile` 结构体（所有字段 `f64`），提供 `merge(style, variant, adapter_profile)`；adapter profile 用一个“部分覆盖”结构（`Option<f64>` 字段或 `serde_json::Value` 子集），保持与 JS 一致的“键存在才覆盖”语义。

### 4.2 DEFAULT_PROFILE（`js/ai-core.js:25-60`）

| 字段 | 值 | 使用位置（摘要） |
| --- | ---: | --- |
| vpip | 0.25 | 开池宽度、模型校正、preflop threshold |
| aggression | 0.55 | 风格偏移、fold 估计、selectCandidate 偏好 |
| threeBetFreq | 0.08 | 3-bet 价值 cutoff、blocker 3-bet 门槛 |
| bluffFreq | 0.10 | selectCandidate 对 credible_range_pressure / preflop_blocker_3bet 的偏好归一 |
| cbFreq | 0.58 | legacy 路径/延续下注频率（shared 路径未直接用） |
| doubleBarrel | 0.46 | legacy 路径（shared 路径未直接用） |
| checkRaiseFreq | 0.10 | legacy 路径（shared 路径未直接用） |
| riskAversion | 0.52 | `riskPenalty` 核心系数 |
| trapPreference | 0.18 | legacy 路径（shared 未直接用） |
| thinValuePreference | 0.42 | 对 callsTooMuch 对手的 deception 加成 |
| adaptationSpeed | 0.55 | legacy 路径（shared 未直接用） |
| uncertaintyTolerance | 0.42 | selectCandidate 的 gapLimit/temperature |
| imageAwareness | 0.58 | storyCredibility / deception |
| patience | 0.55 | legacy 路径（shared 未直接用） |
| rangeEvidenceWeight | 0.70（标准模式被 variant 覆盖为 0.45） | belief.comboWeight 中历史动作似然的指数 |
| calledRangeDiscount | 0.055（标准被覆盖为 0.08） | 被跟注时权益折扣 + 下注尺寸选择 |
| preflopResponseWeight | 0 | 面对加注时用翻前 foldRate 混合的权重（默认关） |
| rangePercentileWeight | 0（标准被覆盖为 1） | belief 用分位 cutoff 替代旧 sigmoid 距离的混合权重 |
| conditionalCallerEquityWeight | 0 | 被跟注条件权益修正权重（实验，默认关） |
| preflopParticipationWeight | 0（标准被覆盖为 1） | 翻前参与范围门控（标准生产开启） |
| preflopRangeSharpness | 0（标准被覆盖为 1） | belief sigmoid 斜率 +45×sharpness |
| observedPreflopRangeWeight | 0（标准被覆盖为 0.65） | 用 BehavioralModel 公开统计收缩 rangeWidth 的权重 |
| heroCallFreq | 0.15 | selectCandidate 对 bluffCatch 候选的偏好归一 |
| professionalWeight | 0 | 已否决实验开关（§4.7） |
| postflopSelectionWeight | 0 | 已否决实验开关 |
| jointCallReturnWeight | 0 | 已否决实验开关 |
| competenceWeight | 0 | 已否决实验开关 |
| targetPrecisionWeight | 0 | 已否决实验开关（会 2×/4× 样本） |
| conditionalRiskWeight | 0 | 已否决实验开关 |
| terminalRealizationWeight | 0 | 已否决实验开关 |
| rankedContinuationWeight | 0 | 已否决实验开关 |
| terminalCallWeight | 0 | 已否决实验开关 |
| riverCallWeight | 0 | 已否决实验开关 |
| riverCfrWeight | 0（标准被覆盖为 1） | River CFR 总开关 |

### 4.3 PROFILE_OVERRIDES（六人格，`js/ai-core.js:62-69`）

| 人格 | 覆盖字段 |
| --- | --- |
| TAG | riskAversion 0.67、aggression 0.50、bluffFreq 0.07、trapPreference 0.18、adaptationSpeed 0.48、patience 0.72 |
| LAG | riskAversion 0.38、aggression 0.70、bluffFreq 0.15、thinValuePreference 0.55、uncertaintyTolerance 0.58、imageAwareness 0.68 |
| SOLID | riskAversion 0.54、aggression 0.56、bluffFreq 0.10、adaptationSpeed 0.62、imageAwareness 0.66 |
| MANIAC | riskAversion 0.28、aggression 0.78、bluffFreq 0.19、uncertaintyTolerance 0.68、imageAwareness 0.48、patience 0.28 |
| TAGFISH | riskAversion 0.76、aggression 0.36、bluffFreq 0.045、trapPreference 0.12、adaptationSpeed 0.38、patience 0.78 |
| CALLING_STATION | riskAversion 0.46、aggression 0.25、bluffFreq 0.035、thinValuePreference 0.28、uncertaintyTolerance 0.52、patience 0.66 |

### 4.4 VARIANT_PROFILE_OVERRIDES（`js/ai-core.js:72-75`）

| variant | 覆盖 |
| --- | --- |
| standard | rangeEvidenceWeight 0.45、calledRangeDiscount 0.08、preflopParticipationWeight 1、rangePercentileWeight 1、preflopRangeSharpness 1、observedPreflopRangeWeight 0.65、riverCfrWeight 1 |
| shortdeck | 无（实际使用 stable 核心自己的覆盖，见 §9.3） |

### 4.5 STYLE_CONFIGS（`js/ai.js:59-66`，adapter 传入的 options.profile）

| style key | vpip | pfr | aggression | threeBetFreq | bluffFreq | foldTo3Bet | cbFreq | doubleBarrel | floatFreq | checkRaiseFreq | heroCallFreq |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| TAG | 0.20 | 0.85 | 0.60 | 0.06 | 0.08 | 0.55 | 0.60 | 0.45 | 0.15 | 0.08 | 0.10 |
| LAG | 0.30 | 0.80 | 0.65 | 0.12 | 0.14 | 0.40 | 0.70 | 0.55 | 0.25 | 0.12 | 0.20 |
| SOLID | 0.24 | 0.75 | 0.55 | 0.08 | 0.10 | 0.48 | 0.55 | 0.48 | 0.18 | 0.10 | 0.15 |
| MANIAC | 0.42 | 0.70 | 0.75 | 0.18 | 0.22 | 0.28 | 0.72 | 0.60 | 0.30 | 0.18 | 0.25 |
| TAGFISH | 0.18 | 0.55 | 0.35 | 0.03 | 0.04 | 0.50 | 0.40 | 0.25 | 0.08 | 0.04 | 0.12 |
| CALLING_STATION | 0.40 | 0.15 | 0.22 | 0.02 | 0.03 | 0.15 | 0.25 | 0.12 | 0.05 | 0.03 | 0.30 |

（`description` 字段只是展示名，不参与计算。`foldTo3Bet/doubleBarrel/floatFreq/cbFreq/checkRaiseFreq` 在 shared 路径中**不读取**，保留是为了 worker meta 与 legacy 兼容；Rust 可原样保存但不使用。）

### 4.6 SD_STYLE_CONFIGS（`js/shortdeck.js:290-333`，短牌本地路径的 options.profile）

| style | vpip | pfr | aggression | bluffFreq | foldToRaise | foldTo3Bet | raiseRatio | cbFreq | threeBetFreq | doubleBarrel | floatFreq | checkRaiseFreq | heroCallFreq |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| TAG | 0.20 | 0.82 | 0.55 | 0.07 | 0.55 | 0.55 | 2.2 | 0.50 | 0.05 | 0.42 | 0.14 | 0.07 | 0.10 |
| LAG | 0.30 | 0.78 | 0.50 | 0.12 | 0.38 | 0.42 | 2.5 | 0.45 | 0.11 | 0.50 | 0.24 | 0.11 | 0.20 |
| TAGFISH | 0.18 | 0.50 | 0.35 | 0.04 | 0.45 | 0.52 | 1.8 | 0.30 | 0.03 | 0.22 | 0.07 | 0.04 | 0.12 |
| MANIAC | 0.38 | 0.68 | 0.60 | 0.20 | 0.25 | 0.30 | 3.0 | 0.50 | 0.17 | 0.58 | 0.28 | 0.17 | 0.25 |
| CALLING_STATION | 0.38 | 0.14 | 0.20 | 0.03 | 0.10 | 0.18 | 1.3 | 0.20 | 0.02 | 0.10 | 0.05 | 0.03 | 0.30 |
| SOLID | 0.23 | 0.72 | 0.45 | 0.08 | 0.45 | 0.48 | 2.0 | 0.40 | 0.07 | 0.42 | 0.16 | 0.09 | 0.15 |

> 注意：短牌 stable 核心只读取其中少数键（vpip/aggression/threeBetFreq/bluffFreq/riskAversion/uncertaintyTolerance/imageAwareness/thinValuePreference/heroCallFreq/rangeEvidenceWeight/calledRangeDiscount 等）；pfr/foldToRaise/foldTo3Bet/raiseRatio/cbFreq 等在 shared 决策中不读取。

### 4.7 实验开关与已否决候选（全部默认 0，**不要启用**）

| 字段 | 设计意图 | 结论 | 来源 |
| --- | --- | --- | --- |
| professionalWeight | 向 PROFESSIONAL_PROFILES 插值（更凶的六人格） | 否决，默认关 | `js/ai-core.js:52,77-85,1052-1056`；`AI_STRATEGY_SPEC_v1.5.8.md` §7 |
| competenceWeight | 向 COMPETENT_STYLE_OVERRIDES 插值（风格纪律） | 否决，默认关 | `js/ai-core.js:53,86-90,1059-1064` |
| postflopSelectionWeight | 用当前牌面成牌分布重分配继续概率 | 否决 | `js/ai-core.js:50` |
| jointCallReturnWeight | 联合“赢牌份额 × 实际付款”修正 | 否决 | `js/ai-core.js:51,967-1007` |
| conditionalCallerEquityWeight | 被跟注时条件权益 | 否决 | `js/ai-core.js:44` |
| preflopResponseWeight | 翻前 foldRate 混合 | 否决 | `js/ai-core.js:42` |
| targetPrecisionWeight | 目标桌 2×/4× 采样 | 否决（成本）；若开启：maxSamples ×(1+3w)、timeBudget ≥160ms | `js/ai-core.js:54,1638-1642` |
| conditionalRiskWeight | 按预计继续人数算风险 | 否决 | `js/ai-core.js:54` |
| terminalRealizationWeight | 全下时权益实现因子置 1 | 否决 | `js/ai-core.js:55,1259-1264` |
| rankedContinuationWeight | 按当前牌面百分位重分配继续概率 | 否决 | `js/ai-core.js:56,1420-1451` |
| terminalCallWeight / riverCallWeight | 终局跟注按净 EV 决策 | 否决 | `js/ai-core.js:57-58,1267-1276` |

**唯一默认开启的“新”能力是 `riverCfrWeight=1`（标准）。**

### 4.8 人格如何影响决策（作用点清单）

| 作用点 | 用到的 profile 字段 | 原文件 |
| --- | --- | --- |
| 翻前开池宽度/加注门槛 | vpip、pfr（openingWidth×） | `ai-core.js:1160-1162,1176-1177` |
| 翻前风格偏移 | vpip/aggression | `ai-core.js:1156` |
| 3-bet 价值/blocker 门槛 | threeBetFreq | `ai-core.js:1164-1187` |
| 翻后风险罚分 | riskAversion、uncertaintyTolerance（间接） | `ai-core.js:1452-1464` |
| 被跟注权益折扣/尺寸选择 | calledRangeDiscount | `ai-core.js:1528` |
| 故事可信度/欺骗加成 | imageAwareness | `ai-core.js:1540,1277-1291` |
| 薄价值加成 | thinValuePreference | `ai-core.js:1542` |
| 混合温度/间距 | uncertaintyTolerance | `ai-core.js:1572,1576-1577` |
| 诈唬候选偏好 | bluffFreq | `ai-core.js:1585` |
| 抓诈唬候选偏好 | heroCallFreq | `ai-core.js:1586` |
| 参与范围门控（标准） | preflopParticipationWeight | `ai-core.js:1149-1150,1174-1187` |
| belief 分位 cutoff | rangePercentileWeight/preflopRangeSharpness | `ai-core.js:371-374` |
| belief 公开统计收缩 | observedPreflopRangeWeight | `ai-core.js:369-370` |
| 历史动作似然 | rangeEvidenceWeight | `ai-core.js:387` |
| CFR 开关 | riverCfrWeight | `ai-core.js:1110` |

## 5. 输入状态与观察事件

### 5.1 原始 gameState 字段（adapter 组装）

本地/服务端 adapter 构造的 raw context 字段（三个来源的字段集合几乎一致；标 * 的仅部分路径提供）：

| 字段 | 类型 | 来源/默认 | 说明 |
| --- | --- | --- | --- |
| handId | number | game.handGeneration / handId | 一手牌标识；变化时 beginHand |
| decisionId | string | decisionIdentity(variant, phase, actionsThisRound, seat) | 决策标识（进 RNG seed） |
| seed | string | poker-table:standard / poker-table:shortdeck | 表级种子串 |
| variant | string | standard / shortdeck | 变体 |
| isShortDeck | bool * | shortdeck 路径 | 与 variant 二选一 |
| startingStack | number | 初始筹码 | 只用于 strategyFocus 与（若开启的）实验权重 |
| tablePlayerCount | number | 本桌总人数 | 只用于 strategyFocus |
| heroSeat | number * | currentPlayerIndex | 默认 currentPlayerIndex 或 0 |
| communityCards | Card[] | 公共牌 | 0/3/4/5 张 |
| publicPlayers | object[] | 见 §5.2 | 公开座位状态 |
| pot | number | 底池 | ≥0 |
| currentBet | number | 本街最高下注 | ≥0 |
| toCall | number | 需跟注额（adapter 已 min(,stack)） | ≥0 |
| yourBet | number | 本街自己已下注 | ≥0 |
| stack | number | 自己剩余筹码 | ≥0 |
| effectiveStack | number | min(自己, 最大对手剩余) | ≥0 |
| numOpponentsActive | number | 未弃牌对手数 | ≥1 |
| activeOpponentSeats | number[] | 未弃牌对手座位 | 空则用 unknown-i |
| playerCount | number | 本手参与人数 | ≥2 |
| currentPlayerIndex | number | 当前行动座位 | 进 context |
| positionFromButton | number | 距庄家顺时针距离（BTN=0/SB=1/BB=2/...） | 位置分类 |
| legalActions | string[] 或 {actions,toCall,canRaise,minRaiseTo,maxRaiseTo,wagerUnit} | 合法动作 | 见 §5.2 |
| canCheck | bool | ownBet >= currentBet | |
| canRaise | bool | 规则允许加注 | |
| minRaiseTo / maxRaiseTo | number | 最小/最大加注到 | |
| preflopRaiseCount | number | 翻前加注次数 | |
| isPreviousStreetAggressor | bool | 上一街最后进攻者是否自己 | 故事可信度 |
| isDelayedCBetCandidate | bool | 转牌延迟持续下注候选 | 故事可信度 |
| isSmallBlind / isBigBlind | bool | 位置 | |
| bigBlind | number | 大盲（短牌为 minBet） | 默认 80 |
| wagerUnit | number * | 小盲/前注 | normalized 时再按 rules.wagerUnit 计算 |
| phase | string * | 本地路径提供 | normalized 时优先 street |
| timeBudgetMs | number | 本地：river 或 ≥5 人 150，否则 80；服务端同 | 仅紧急刹车 |
| evaluateCards | function | adapter 注入 | Rust 改为直接调用 poker-core |

> 注意：**normalizedContext 不读 stack**（读 heroStack），adapter 必须显式传 heroStack；同理读 heroRoundBet（无则回退 yourBet）。

### 5.2 normalizedContext 映射（`js/ai-core.js:836-892`）

| context 字段 | 取值 |
| --- | --- |
| handId | raw.handId 或 0 |
| decisionId | raw.decisionId 或 raw.turnId 或 0 |
| variant | raw.variant==='shortdeck' 或 raw.isShortDeck 则 shortdeck，否则 standard |
| street | raw.street 或 raw.phase 或按公共牌数推导（0=preflop/3=flop/4=turn/5=river） |
| heroSeat | raw.heroSeat 或 raw.currentPlayerIndex 或 0 |
| holeCards | 归一化 {rank:String, suit:String, value:finite(value, VALUES[rank])} |
| communityCards | 同上 |
| pot | max(0, finite(raw.pot)) |
| currentBet | max(0, finite(raw.currentBet)) |
| heroRoundBet | max(0, finite(raw.heroRoundBet, raw.yourBet)) |
| toCall | max(0, finite(raw.toCall, legal.toCall)) |
| heroStack | max(0, finite(raw.heroStack, raw.stack)) |
| effectiveStack | max(0, finite(raw.effectiveStack, raw.stack)) |
| minRaiseTo | max(0, finite(raw.minRaiseTo, legal.minRaiseTo)) |
| maxRaiseTo | max(0, finite(raw.maxRaiseTo, legal.maxRaiseTo)) |
| canRaise | raw.canRaise !== false && legal.canRaise !== false |
| canCheck | raw.canCheck === true 或 actions 含 check |
| legalActions | 数组形式：raw.legalActions 是数组则直接用；否则取 legal.actions 或 raw.actions 或 []；若为空则合成：toCall>0 时 [fold,call] 否则 [check]，canRaise 且 maxRaiseTo>currentBet 时加 raise，heroStack>0 时加 allin |
| activeOpponentSeats | (raw.activeOpponentSeats 或 raw.opponents) 映射为 seatId/id/index 或原值 |
| numOpponents | max(1, finite(raw.numOpponents, finite(raw.numOpponentsActive, opponents.length 或 1))) |
| playerCount | max(2, finite(raw.playerCount, (opponents.length 或 1)+1)) |
| tablePlayerCount | max(2, finite(raw.tablePlayerCount, raw.playerCount 或 (opponents.length 或 1)+1)) |
| startingStack | max(0, finite(raw.startingStack)) |
| positionFromButton | finite(raw.positionFromButton)（默认 0） |
| isSmallBlind / isBigBlind | !!raw.* |
| preflopRaiseCount | max(0, finite(raw.preflopRaiseCount)) |
| isPreviousStreetAggressor / isDelayedCBetCandidate | !!raw.* |
| evaluateCards | raw.evaluateCards（原样） |
| timeBudgetMs | finite(raw.timeBudgetMs, opponents.length>=4 或 street==='river' ? 150 : 76) |
| mcSamples | finite(raw.mcSamples, 0) |
| bigBlind | max(1, finite(raw.bigBlind, 80)) |
| wagerUnit | rules.wagerUnit(raw)（见 §5.4） |
| publicActionHistory | raw.publicActionHistory 或 [] |
| publicPlayers | 每个 {seat, stack≥0, committed≥0, roundBet≥0, positionFromButton(可选), isSmallBlind, isBigBlind, folded, allIn} |

**重要语义**：

- toCall 用的是“已截断到自己的筹码”的 adapter 值（两处 adapter 都传 legal.toCall = min(toCall, stack)）。Rust engine 的合法动作接口必须同样返回截断值。
- publicPlayers 必须包含**全部座位**（含已弃牌/已淘汰者），因为 contestablePot、potContestGroups、CFR 门控都遍历它；漏座位会导致收益/门控错误。
- evaluateCards(cards, variant)：variant 参数只在短牌路径被传 shortdeck，standard 传 standard；实现必须按 variant 选择评估器。

### 5.3 观察事件（AI 学习公开信息）

服务端/本地引擎在每次行动后广播事件给所有 AI（`server/server-game.js:351-380`、`js/engine.js:458-491`）：

~~~text
recordOpponentAction(actorSeat, action, street, handId, meta)
   - 若 actorSeat == 自己：sharedBrain.observeSelfAction(event)（仅标准 profile riverCfrWeight>0 时记录 publicSelfBelief）
   - 否则：playerModels[seat].recordAction（legacy，不参与 shared）+ sharedBrain.observeAction(event)
recordShowdown(seat, {won, handRank, wasBluff, wasTrap})   [produced in engine/server showdown]
~~~

meta = buildActionMeta(...) + 位置字段（`js/game-rules-core.js:32-53`、`server/server-game.js:352-353`）：

| meta 字段 | 定义 | 被谁读取 |
| --- | --- | --- |
| callCost / toCallBefore | max(0, callCost 或 toCallBefore) | BehavioralModel（postflopFaced/preflopFaced）、preflopResponse |
| additionalPaid / amount | max(0, additionalPaid 或 amount) | 无（信息用） |
| raiseTo | max(0, raiseTo) | 无（betFraction 输入） |
| currentBetBefore | max(0, currentBetBefore) | 无（betFraction 输入） |
| potBefore | max(0, potBefore) | OpponentBelief.history（publicActionLikelihood 价格） |
| potAfter | max(0, potAfter) | 无 |
| aggressive | !!aggressive | BehavioralModel/OpponentBelief 分支判定 |
| betFraction | wagerSize / max(1, minimumBet, potBefore)，wagerSize = aggressive ? raiseTo-currentBetBefore : additionalPaid | sizeBucket（BehavioralModel）、OpponentBelief 观察幅度 |
| preflopRaiseCountBefore | 原样 | 3bet/4bet 归类 |
| faced3bet | 原样 | BehavioralModel.preflopRanges.faced3bet 计数（stable 不用） |
| positionFromButton / playerCount | 引擎的 getHandPositionInfo | OpponentBelief 开池位置分类（standard） |
| isSmallBlind / isBigBlind | 引擎 | 开池位置分类 |
| communityCards | 事件发生时的公共牌（拷贝） | boardComboFeatures（历史牌面权重） |
| isCbetOpportunity / facedCbet | 本地引擎提供；服务端只用 faced3bet | BehavioralModel（stable 忽略） |

> Rust 移植要求：poker-engine 产出的 action meta 必须逐字段一致（尤其 **betFraction 的分母 = max(1, minimumBet, potBefore)**，短牌 minimumBet = minBet，标准 = bigBlind）。这是 engine 与 AI 的接口契约，建议单独写契约测试（用黄金语料中的 observes 反推）。

### 5.4 规则辅助函数（Rust 由 poker-engine 提供）

| 函数 | 定义 | 文件 |
| --- | --- | --- |
| wagerUnit(input) | max(1, floor(input.wagerUnit 或 config.wagerUnit 或 (短牌?config.ante:config.smallBlind)))；无效回退 (bigBlind 或 2)/2；短牌判定 = isShortDeck 或 variant==='shortdeck' 或 (config.ante!=null && config.smallBlind==null) | `js/game-rules-core.js:8-13` |
| raiseBounds(min, max, unit) | min = ceil(min/unit)×unit；max = floor(max/unit)×unit | `js/game-rules-core.js:16-19` |
| snapRaise(amount, min, max, unit) | 若 bounds.min>bounds.max 或 amount 非有限返回 null；否则 clamp(round(amount/unit)×unit, bounds.min, bounds.max) | `js/game-rules-core.js:21-25` |
| decisionIdentity(variant, phase, ordinal, seat) | variant:phase:max(0,ordinal):seat（variant 归一 shortdeck/standard，phase 默认 preflop） | `js/game-rules-core.js:27-30` |
| buildActionMeta(input) | 见 §5.3 表 | `js/game-rules-core.js:32-53` |

## 6. 决策流水线

### 6.1 decide() 总流程（`js/ai-core.js:1627-1720`）

1. context = normalizedContext(rawContext)
2. strategyFocus = variant==='standard' && tablePlayerCount===10 && startingStack===5000 && bigBlind===200 && wagerUnit===100 ? 'ten-player-25bb' : 'general'（`:1629`）
3. this.profile = strategyFocus==='ten-player-25bb' ? targetProfiles[variant] : variantProfiles[variant]（`:1630`）
4. 实验开关注入（来自 profile，默认 0；仅 target 焦点启用）：jointCallReturnWeight / conditionalRiskWeight / targetPrecisionWeight / terminalRealizationWeight / rankedContinuationWeight / terminalCallWeight / riverCallWeight（`:1631-1637`）
5. 若 targetPrecisionWeight>0：mcSamples = round(base×(1+3w))，timeBudgetMs = max(...,160)（`:1638-1642`）
6. context.postflopParticipants = street!=='preflop' && (postflopSelectionWeight>0 || rankedContinuationWeight>0)
7. context.sampleResponses = conditionalCallerEquityWeight>0 || postflopParticipants
8. handId 变化或 plan 为空时 beginHand(handId)
9. **seed = rawContext.seed（默认 'table'）+ ':' + handId + ':' + seatId + ':' + decisionId + ':' + style**（字符串拼接，JS 里是模板字符串）；rng = new SeededRng(seed)
10. reads = activeReads(context)：为每个 activeOpponentSeats 建立/取回 {belief, model}
11. 若 street==='preflop'：preflopDecision(context, rng, reads)
12. 否则：estimateRangeEquity --> analyzeTexture --> buildPostflopCandidates --> selectCandidate
13. 若已选 action 后：solveRiver(context, rng, reads)，通过则覆盖 decision 并把候选替换为该决策（`:1660-1661`）
14. legalize(decision, context)
15. plan.update(...)、selfImage.record(chosen, context, plan.intent)
16. 组装 lastTrace（§6.11）并返回 {action, amount}（内部 chosen）

### 6.2 beginHand 与状态生命周期（`js/ai-core.js:1085-1091`）

- currentHandId = handId；plan = new HandPlan(handId)
- 对每个已有 belief 调 belief.beginHand()（belief 重置，BehavioralModel **不重置**）
- 若存在 publicSelfBelief 也重置；lastRiverPublicAction = null
- 观察事件带 handId 且与 currentHandId 不同时也会触发 beginHand（`:1103,1094`）

| 状态 | 跨手保留？ | 说明 |
| --- | --- | --- |
| OpponentBelief（rangeWidth/strengthShift/...） | 否（beginHand 重置） | 手内累积 |
| BehavioralModel（hands Set、vpip/pfr/fold 统计、showdowns） | 是 | 窗口 256 手 |
| SelfImage（自己公开形象） | 是（80 条动作、256 手） | 由每次决策 record |
| HandPlan | 否（每手新建） | 手内意图/连续性加成 |
| publicSelfBelief / publicSelfModel | 否/是（与 belief/model 同规则） | CFR 用“别人能观察到的自己” |

### 6.3 strategyFocus 与 profile 选择

- 判定式见 §6.1 第 2 步；只有**标准、10 人满桌、5000 起始、100/200 盲注、wagerUnit=100** 才是目标桌。
- 本地标准默认配置正是该配置（见 `docs/RUST_REWRITE_HANDOFF.md` §8.3）。
- 短牌永远不会是 target（variant 条件限制）。

### 6.4 preflopDecision（生产分支，`js/ai-core.js:1147-1193`）

当 typeof context.evaluateCards === 'function'（生产恒真）执行：

~~~text
context.preflopParticipants = profile.preflopParticipationWeight > 0   // 标准=1
context.sampleResponses |= preflopParticipants
equity = estimateRangeEquity(context, beliefs, rng)
texture = analyzeTexture([], holeCards, variant)
strength = startingHandStrength(hole0, hole1, variant, positionCategory(context))
position = positionCategory(context)
threshold = LP ? .37 : BL ? .43 : .48
facingRaises = preflopRaiseCount × .07
style = (.25 - vpip)×.40 + (.55 - aggression)×.08
credibleSteal = numOpponents===1 && 有 foldsTooMuch 对手 && strength > .28
cards = comboFeatures(hole)
openingWidth = playerCount===2 ? clamp(.55 + vpip - .24, .40, .85)
                                : clamp(vpip × (EP?.75 : LP?1.4 : BL?.95 : 1), .10, .65)
openingCutoff = startingRangeCutoff(openingWidth, variant)
openingRaiseCutoff = startingRangeCutoff(clamp(openingWidth × finite(pfr,.75), .025, .65), variant)
price = toCall / bigBlind
defenseWidth = raiseCount>=3 ? clamp(.04+(vpip-.24)×.10, .025,.08)
             : raiseCount>=2 ? clamp((LP?.12:.09)+(vpip-.24)×.15+max(0,4-price)×.015-max(0,price-5)×.005, .025,.20)
             : clamp((isBB?.45 : isSB?.20 : LP?.23 : EP?.18 : .20)+(vpip-.24)×.20+max(0,2.5-price)×.03, .10,.60)
valueCutoff = startingRangeCutoff(clamp((raiseCount>=2?.012:.025)+threeBetFreq×.15, .02,.075), variant)
candidates = buildPostflopCandidates(context, equity, texture, reads).filter(...)   // 过滤器见下
decision = selectCandidate(candidates, context, rng, equity.standardError 或 .05)
return {decision, equity, texture, reads, candidates}
~~~

候选过滤器（`:1168-1190`，按顺序）：

1. fold / check 直接保留。
2. call（或 invest<=toCall 的 allin）：若 terminalCall 保留；若活动实验 professionalWeight>=.5 且 raiseCount===0 且非盲注且存在未弃牌非 BB 对手已下注>=currentBet，则丢弃（实验开关默认 0，不触发）；若 preflopParticipants && preflopRaiseCount>0：要求 cards.strength >= startingRangeCutoff(defenseWidth)；若 preflopParticipants && raiseCount===0 && !isBigBlind：要求 cards.strength >= startingRangeCutoff(isSmallBlind ? clamp(openingWidth×1.4,.20,.65) : openingWidth)；否则保留。
3. 加注类（raise/allin 且 invest>toCall）：若 preflopParticipants && raiseCount===0 && !credibleSteal：要求 cards.strength >= max(openingCutoff, openingRaiseCutoff)；若 preflopParticipants && raiseCount>0：value = cards.strength >= valueCutoff；bluff = raiseCount===1 && position==='LP' && suited && (aceBlocker 或 kingBlocker) && response.allFold>.55 && threeBetFreq>=.06；两者都不满足则丢弃；reason 标 preflop_value_3bet / preflop_blocker_3bet。
4. 其余（fold/check 之外的兜底）：要求 strength >= threshold + facingRaises + style || credibleSteal。

> legacy 分支（`:1194-1248`）仅在无 evaluateCards 时执行（测试/异常回退）。**Rust 不移植**，但测试应验证注入评估器后不会走 legacy。

### 6.5 范围权益 estimateRangeEquity（`js/ai-core.js:692-834`）

输入：context、beliefs Map（seat key 为字符串）、rng。

~~~text
known = 自己的 holeCards + communityCards 的 cardKey 集合
deck = buildDeck(variant) 去掉 known（标准 52 / 短牌 36 张）
seats = activeOpponentSeats 或 ['unknown-0'...numOpponents-1]
samplers = 对每个 seat 用 buildWeightedCombos(deck, belief, variant, featureCache) 生成组合列表
           （belief 为 undefined 时权重全 1；belief.folded 时全部权重 0 --> 该 seat 被过滤）
若 samplers 为空 --> 返回 {mean:.5, stdDev:.22, samples:0, confidence:.05, timedOut:false}
rangeConfidence = 各 seat belief.confidence 平均值（默认 .12）
subsetGroups = potContestGroups(context) 中 0 < 人数 < samplers.length 的组
responseSamples = sampleResponses ? {seats, relations:[], ...} : null
~~~

**A. 精确河牌单挑路径**（`:720-737`，communityCards.length===5 且 samplers.length===1）：

~~~text
hero = score(hole + board)
对每个对手组合 combo：other = score(combo + board)
outcome = hero>other ? 1 : hero===other ? .5 : 0
mean = Σ outcome×w / Σ w；stdDev = sqrt(max(0, Σ outcome²×w/Σw - mean²))
返回 {mean, stdDev, standardError:0, samples:samplers[0].length, confidence:1, rangeConfidence, responseSamples, exact:true, timedOut:false}
~~~

（标准河牌单挑 45 张未知牌可组 990 个组合；短牌 29 张未知牌 406 个组合——samples 即组合数。）

**B. 蒙特卡洛路径**（`:738-833`）：

| 常量 | 值 | 说明 |
| --- | --- | --- |
| budget | clamp(timeBudgetMs, 20, 180) | 毫秒 |
| EMERGENCY_GRACE | 3 | 实际刹车阈值 = budget×3 |
| baselineSamples | numOpponents>=4 ? 240 : numOpponents>=2 ? 320 : 420 | 固定样本数 |
| riverBonus | 河牌 +40 | |
| maxSamples | max(120, floor(mcSamples 或 baselineSamples+riverBonus)) | 目标样本 |
| minimumSamples | numOpponents>=4 ? 40 : 56 | 刹车时至少样本 |

采样循环（attempt 递增，直到 samples>=maxSamples）：

1. 每轮检查 now-start >= budget×3：若已 samples>=minimumSamples 则 timedOut=true 并 break；否则继续。
2. used = new Set(known)
3. **随机化 seat 顺序**：order = samplers.slice()；对 j 从 len-1 到 1：k=rng.int(j+1)，交换 order[j]/order[k]。
4. 按 order 依次 weightedPick(sampler, rng, used)：
   - 最多 22 次尝试：target = rng.next()×totalWeight，二分找 cumulative 首个 >= target 的组合；若组合两张牌都不在 used 中则返回。
   - 22 次失败后：再 roll = rng.next()×total 线性扫描未使用组合（按数组顺序减权重，首个 roll<=0 返回）；若无可用返回 null。
   - 任一 sampler 返回 null --> 样本作废；若 samples===0 且 attempt>=5 则 stalled=true 并 break；否则 continue。
5. 补全公共牌到 5 张：每次 while，最多 30 次 guard：candidate = deck[rng.int(deck.length)]，不在 used 则选用（每次 guard 消耗 1 个 rng）；失败则样本作废。
6. hero = score(hole + board)（评估异常 --> 返回兜底 {mean:.5,...}）
7. 对每个对手：score(opponentCards + board)；若 score>hero --> beaten=true（若无需 subset/response 则提前 break）；若相等 ties++。
8. outcome = beaten ? 0 : 1/(ties+1)
9. subsetGroups：每组维护 sum；本样本 loses/split，group.sum += loses?0:1/(split+1)
10. responseSamples（仅当 context.sampleResponses）：
    - relations.push(对每个 seat：score>hero ? -1 : score===hero ? 0 : 1)
    - preflop：strengths.push(每个 seat 的 startingHandStrength(...,'MP'))
    - 非 preflop：selectionScores.push(每个 seat 的 selectionScore = clamp(value + draw×.24 + nutBlocker×.03, 0, 1))
    - 若 ranked 模式（rankedContinuationWeight>0）：currentScores/currentDraws 记录当前牌面特征
11. samples++，累加 sum/sumSquares。

返回：mean=clamp(samples?sum/samples:.5, .005,.995)；stdDev=sqrt(max(0, sumsq/samples-mean²))；standardError=sqrt(variance/max(1,samples))；rangeConfidence；subsetEquities（各 group sum/samples，默认 .5）；responseSamples；samples；confidence=clamp(samples/max(160,maxSamples),.08,.96)；timedOut；stalled。

**C. responseSamples 结构**：

| 字段 | 含义 |
| --- | --- |
| seats | 与 samplers 顺序对应的 seat 字符串 |
| relations | 每样本每个 seat：-1 输 / 0 平 / 1 赢（相对 hero） |
| strengths | 仅翻前：每样本每 seat 的起手强度（MP 位置） |
| selectionScores | 非翻前：每样本每 seat 的当前牌面 selectionScore |
| currentScores/currentDraws | ranked 实验用；默认不产生 |
| weights | 仅精确河牌路径填（组合权重） |

### 6.6 牌面纹理与组合特征

**analyzeTexture(board, hole, variant)**（`js/ai-core.js:598-647`）：

~~~text
paired    = 公共牌存在重复 rank
monotone  = 某花色在公共牌 >=3
hasFlushDraw = boardComboFeatures.flushDraw（board<5 且某花色恰好 4 张且其中含自己手牌）
openEnded = boardComboFeatures.openEnded（<=5 张时存在 4 连张且缺 1 张可成顺）
gutshot   = features.draw 且 !openEnded 且 !flushDraw
hasStrongDraw = flushDraw || openEnded
nutBlocker = boardComboFeatures.nutBlocker
connectedness = 排序后公共牌 rank 相邻差 <=2 的个数
wetness = clamp((monotone?.34 : 两同花?.17 : 0) + connectedness×.11 + (paired?.10:0), 0, 1)
pressureCard = 最后一张公共牌 value>=11 或 paired 或 monotone
~~~

**boardComboFeatures(c1,c2,board,variant)**（`js/ai-core.js:207-258`）：value 初值 .10；按成牌类型：

| 成牌 | value |
| --- | --- |
| 一对（公共牌对） | 若非自己成对 .21；自己成对且 pair>top .72；pair===top .62；否则 .38 |
| 两对 | .78 |
| 三条 | .88 |
| 顺子 | .92 |
| 同花 | .95 |
| 葫芦 | .975 |
| rank>=7（四条/同花顺） | .995 |
| 公共牌面自己参与的两对/三条（rank 相同且手牌 rank 不在公共牌） | .20 + .20×max(hole values)/14 |
| 河牌且与公共牌完全相同（打板） | .25 |

draw：flushDraw 时 .58；openEnded 时 .42；仅 gutshot 时 .24；上限 .90。nutBlocker：公共牌某花色>=2 且自己有该花色 A 加 .7 / K 加 .25，上限 1。score/rank 来自 poker-core evaluate（board<5 时为部分手牌评估）。

**publicActionLikelihood(features, event, aggression)**（`js/ai-core.js:260-275`）——belief 用历史动作反推组合权重：

~~~text
check:  value>.85 ? .65+(1-aggression)×.35 : 1.20-value×.30+draw×.18
call:   price = toCall / max(1, pot+toCall)；drawScale = price>.30 ? .40 : .85
        max(.06, .09 + value×1.20 + draw×drawScale - (fraction>.8 && value<.5 ? .18 : 0))
raise/bet/allin:
        pressure = fraction>.85 ? 1.25 : 1
        bluff = (.035 + nutBlocker×.18)×(.6+aggression)
        max(.035, value^3.2×3.0×pressure + draw×(.55+aggression) + bluff)
其他动作：1
~~~

### 6.7 对手回应模型

**responseForRaise(context,target,equity,texture,reads)**（`js/ai-core.js:1330-1378`）——通用多路回应：

~~~text
fraction = (target - heroRoundBet) / max(bigBlind, pot+toCall)
credibility = storyCredibility(...)（§6.11）
对每个 read：
  若对手 allIn 或 stack===0：{fold:0, call:1, raise:0}，allFold=0，conditionalAllFold=0，conditionalCallers+=1
  fold = model.foldRateFor(fraction)                          // postflop
       + preflopResponseWeight×(model.foldRateFor(fraction,'preflop') - fold)   // 默认 0
       + (fraction-.55)×.13
       + (credibility-.5)×.20
       - (belief.estimatedStrength()-.5)×.32
       - callsTooMuch? .13 : 0
       + foldsTooMuch? .11 : 0
       - (numOpponents>1 ? .025 : 0)
       clamp(.04,.86)
  canRaise = target < maxRaiseTo && (!player || player.roundBet+player.stack > target)
  raise = canRaise ? clamp((model.aggressionRate-.30)×.18 + max(0, belief.estimatedStrength()-.58)×.25 + fraction×.025, .015,.26) : 0
  fold = min(fold, 1-raise)；call = 1-fold-raise
  allFold *= fold；noRaise *= 1-raise
  conditionalCallers += call/(1-raise)
  conditionalAllFold *= fold/(1-raise)
  confidence += model.confidence×.55 + belief.confidence×.45
返回 {allFold, anyRaise:1-noRaise, anyCall:max(0,noRaise-allFold),
      expectedCallers: conditionalCallers / max(1e-9,1-conditionalAllFold),
      credibility, confidence: reads.length?平均: .2, opponents, fraction}
~~~

**preflopResponse(context,target,mode,reads,equityInfo)**（`js/ai-core.js:1292-1329`）：用于标准目标桌 preflopParticipants 时的 call/raise 分支。基于 responseSamples 的 strengths 用固定 cutoff 计算继续概率：

~~~text
fraction = betFractionForTarget
对每个 read（按 reads 顺序）：
  forced = 对手 allIn/stack===0，或 mode==='call' 且对手 roundBet>=currentBet
  raised = belief.history 中存在翻前 aggressive 动作
  width = variant==='shortdeck'? .30 : .22
  if raiseCount>0: width = mode==='call' ? .20 : raised ? .13 : .095
  if raiseCount>=2: width = raised ? .065 : .05
  if player 有 positionFromButton 且 !raised：
      pos = positionCategory({...player, playerCount})
      width *= isBB?1.2 : isSB?.85 : pos==='LP'?1.15 : pos==='EP'?.85 : 1
  size = target / max(bigBlind, currentBet)
  width -= max(0, size-(raiseCount?3:2.5))×.02
  width += (model.vpip-.27)×model.confidence×.45
  if callsTooMuch: width += .08
  width = clamp(width,.025,.55)
  canRaise = target<maxRaiseTo && (!player || player.roundBet+player.stack>target)
  raise = forced||!canRaise ? 0 : clamp(.025+max(0,model.pfr-.18)×model.confidence×.25,.015,.12)
  opponents.push({seat, raise, forceContinue:forced, continuationCutoff:forced?undefined:startingRangeCutoff(width), width})
遍历每个样本：probabilities = responseCallProbabilities（优先 forceContinue / continuationCutoff sigmoid：(strength-cutoff)×24；否则 call/(call+fold)）
allFoldGivenNoRaise = 样本平均 Π(1-p)
opponents[j].call = (1-raise)×mean_prob；fold = (1-raise)×(1-mean_prob)
noRaise = Π(1-raise)；anyCall = noRaise×(1-allFoldGivenNoRaise)
expectedCallers = Σmean / max(1e-9, 1-allFoldGivenNoRaise)
~~~

**postflopResponse**（`js/ai-core.js:1379-1419`）：默认路径因为 postflopSelectionWeight=0 且 rankedContinuationWeight=0 时进入。先 responseForRaise，再基于 selectionScores 做二分偏置校准（12 次迭代）把条件继续概率校准到 prior（=call/(call+fold)）：

~~~text
postflopCutoff = .28 + .26×price + min(.08,(numOpponents-1)×.025) + (river? .035:0)
price = cost / max(1, pot + max(0,target-heroRoundBet) + cost)
cost = min(player.stack, max(0,target-player.roundBet))
rawMean = Σ weight × sigmoid((selectionScore-cutoff)×14) / totalWeight
targetMean = prior + (rawMean-prior)×postflopSelectionWeight     // 默认 0 --> 直接 prior
二分 bias ∈ [-12,12] 12 次，使 Σ weight×clamp(sigmoid((score-cutoff)×14+bias),.002,.998)/totalWeight ≈ targetMean
opponent.continuationBias = bias
随后用 responseCallProbabilities（continuationBias sigmoid）重新计算 allFold/anyCall/anyRaise/expectedCallers
~~~

> 默认配置下 postflopSelectionWeight=0，因此 targetMean=prior，bias 会校准回原 fold/call 先验；但**数值上仍会经过一次校准与重算**，Rust 必须照做否则数值不同。

**rankedPostflopResponse**（`:1420-1451`）：仅 rankedContinuationWeight>0 时使用（默认关）；按 currentScores 加权百分位（同分取概率质量中点）重分配继续概率，二分 16 次。

**conditionalCalledEquity(equityInfo, response, eligibleSeats)**（`:937-961`）：对 responseSamples 逐样本用 responseCallProbabilities 做“未被打败概率 × 平局多项式”计算，返回被跟注条件下的 hero 权益；denominator<=1e-9 时回退 mean。

**jointAdditionalReturn**（`:967-1007`）：实验路径（默认关），联合“赢牌份额×实际付款”与边池资格。

### 6.8 翻后候选收益 buildPostflopCandidates（`js/ai-core.js:1499-1552`）

输入：context、equityInfo（§6.5 返回）、texture、reads。

~~~text
equity = equityInfo.mean
realization = realizationFactor(context, texture, reads)   // §6.8.1
候选：
A. fold（toCall>0 且合法）：ev=0, risk=0, utility=0, reason='preserve_stack'
B. check（toCall===0 且 canCheck）：
   future = contestableReturn(context, 0, equityInfo) × realization
   ev=future, utility=future, reason='realize_equity_or_control_pot'
C. call（toCall>0 且合法）：
   invest = min(toCall, heroStack)
   terminalCall = terminalCallState(context)               // §6.8.2
   response = preflopParticipants ? preflopResponse(...) : postflopParticipants ? postflopResponse(...) : null
   info = response ? participantEquityInfo(context, equityInfo, response) : equityInfo
   factor = terminalCall ? 1 : realizationFactor({...context, numOpponents:max(1,response.expectedCallers) 若 response}, texture, reads, invest)
   extra = response ? expectedAdditionalCalls(context, currentBet, response) : 0
   payment = response ? info.mean×extra + (jointCallReturnWeight ? w×(jointAdditionalReturn - info.mean×extra) : 0) : 0
   calledEv = contestableReturn(context, invest, info)×factor + payment - invest
   ev = response && !terminalCall ? (1-response.anyRaise)×calledEv - response.anyRaise×invest×.6 : calledEv
   riskContext = conditionalRiskWeight ? {...context, numOpponents: numOpponents+(max(1,expectedCallers)-numOpponents)×w} : context
   risk = terminalCall ? 0 : riskPenalty(riskContext, invest, info, texture, 'call')   // §6.8.3
   candidate: action = invest>=heroStack ? 'allin':'call'；amount=invest；invest；ev；risk；terminalCall；calledEquity=info.mean；
              utility=ev-risk；bluffCatch = street==='river' && equity<.52
              reason = terminalCall?'terminal_call_payoff' : response?'price_and_participating_ranges':'price_and_range_equity'
D. raise/allin（合法）：遍历 raiseTargets(context, texture)（§6.8.4）每个 target：
   invest = target - heroRoundBet
   response = preflopParticipants ? preflopResponse(...) : postflopParticipants ? postflopResponse(...) : responseForRaise(...)
   expectedCall = expectedAdditionalCalls(context, target, response)
   selection = clamp((response.fraction-.20)×calledRangeDiscount + (1-allFold-anyRaise)×.025, 0, .16)
   conditional = preflopParticipants||postflopParticipants
   info = conditional ? participantEquityInfo(...) : equityInfo
   baseEquity = conditional ? info.mean : equity + conditionalCallerEquityWeight×(conditionalCalledEquity(equityInfo,response)-equity)
   calledEquity = conditional ? baseEquity
                  : clamp(baseEquity - selection×(1-baseEquity) - (expectedCallers>1.15 ? .025:0), .001,.995)
   factor = preflopParticipants ? realizationFactor({...context,numOpponents:max(1,expectedCallers)},texture,reads,invest) : 1
   payment = calledEquity×expectedCall + (conditional && jointCallReturnWeight ? w×(jointAdditionalReturn-target - calledEquity×expectedCall) : 0)
   calledEv = min(contestablePot(context,invest)+invest, contestableReturn(context,invest,info)×(calledEquity/max(.001,info.mean)))×factor + payment - invest
   reraisedEv = calledEquity>.68 ? calledEv×.72 : -invest×(0.44+anyRaise×0.24)
   ev = allFold×contestablePot(context,invest) + anyCall×calledEv + anyRaise×reraisedEv
   risk = riskPenalty(riskContext, invest, {...info,mean:calledEquity}, texture, target>=maxRaiseTo?'allin':'raise')
          × (1 - (conditionalRiskWeight||0)×response.allFold)
   deception = (credibility-.5)×pot×.035×imageAwareness
              - (equity<.32 && !hasStrongDraw ? pot×.025 : 0)
              + (有 callsTooMuch && equity>.57 ? pot×.022×thinValuePreference : 0)
   action = target>=maxRaiseTo ? 'allin':'raise'；amount = allin ? heroStack : target
   utility = ev - risk + deception
   reason = calledEquity>.62?'value_pressure' : texture.hasStrongDraw?'semi_bluff_pressure':'credible_range_pressure'
最后：candidate.planBonus = plan ? plan.continuityBonus(candidate, context, pot) : 0；utility += planBonus
~~~

> **重要**：默认（experimental weights=0）下，call/raise 的 response 取决于 context.preflopParticipants / context.postflopParticipants：标准目标桌翻前用 preflopResponse，翻后（默认）用 responseForRaise（postflopParticipants 需要 postflopSelectionWeight 或 ranked 权重>0，默认 0）。Rust 必须复刻这两个开关语义。

#### 6.8.1 realizationFactor（`js/ai-core.js:1249-1266`）

~~~text
river --> 1
factor = .88
+ .10  若 positionCategory==='LP'
- min(.18,(numOpponents-1)×.045)  若 numOpponents>1
+ .06  若 texture.hasStrongDraw
- .06  若 texture.wetness>.55 且 effectiveStack > pot×5
+ .035 若存在 passivity> .62 的 read
clamp(factor,.66,1.12)
若 terminalRealizationWeight>0 且 (invest>=heroStack&&heroStack>0 || 所有活跃对手 allIn/stack===0)：
   factor += (1-factor)×terminalRealizationWeight       // 默认 0
~~~

#### 6.8.2 terminalCallState（`js/ai-core.js:1267-1276`）

~~~text
若 toCall<=0 || heroStack<=0 || activeOpponentSeats 空 --> null
对手座位在 publicPlayers 找不到 --> null
allIn = 所有对手 allIn 或 stack===0
若 terminalCallWeight>0 且 (toCall>=heroStack || allIn) --> 'allin'      // 默认 0
若 riverCallWeight>0 且 street==='river' && currentBet>0 && 所有对手 allIn/stack===0/roundBet>=currentBet --> 'river-close'
否则 null
~~~

（两个权重默认 0 → 恒为 null；Rust 保留空实现/字段即可。）

#### 6.8.3 riskPenalty（`js/ai-core.js:1452-1464`）

~~~text
若 invest<=0 --> 0
stackRatio = invest / max(1, heroStack)
deepRatio = effectiveStack / max(bigBlind, pot)
uncertainty = clamp((standardError 或 stdDev/sqrt(max(1,samples)))×2 + (1-(rangeConfidence 或 .3))×.12, .03,.35)
penalty = invest × stackRatio × riskAversion × (0.18 + uncertainty×0.42)
若 numOpponents>1：penalty ×= 1+(numOpponents-1)×.10
若 deepRatio>5 且 !hasStrongDraw 且 equity.mean<.62：penalty += invest×.045
若 stackRatio>.65 且 equity.mean<.64：penalty += invest×(stackRatio-.65)×.35
若 action==='call' 且 wetness>.62 且 equity.mean<.52：penalty += invest×.035
~~~

#### 6.8.4 raiseTargets（`js/ai-core.js:1465-1498`）

~~~text
若 !canRaise 或 maxRaiseTo<=currentBet --> []
potAfterCall = max(bigBlind, contestablePot(context,toCall) + toCall)
live = 未弃牌且非 hero 的 publicPlayers
ceiling = live.length ? min(maxRaiseTo, max(currentBet, max(live roundBet+stack))) : maxRaiseTo
fractions: river [0.33,0.68,1.08]；wetness>.52 [0.52,0.75,0.98]；否则 [0.30,0.55,0.76]
targets = fractions.map(f => heroRoundBet + toCall + round(potAfterCall×f))
若 preflop：
   targets = []
   base = max(bigBlind, currentBet)
   outOfPosition = position==='LP' ? 0 : .35
   multipliers = raiseCount ? [2.7+oop, 3.4+oop] : [2.25, 2.8]
   targets.push(round(base×m)..., round(currentBet + potAfterCall×.75))
   若 target 焦点 && raiseCount===0 && 15 < effectiveStack/bigBlind <= 40：
       limpers = live 中非 BB 且 roundBet>=currentBet 的数量
       targets = [round(bigBlind×(2+limpers)), round(bigBlind×(2.5+limpers))]
geometric：streets = flop?3:turn?2:1；geometric = (pow(1+2×effectiveStack/potAfterCall, 1/streets)-1)/2
   若 .25<=geometric<=1.5：targets.push(currentBet + round(potAfterCall×geometric))
除非 focusOpen（target 焦点 && 翻前 && raiseCount===0 && 15<depth<=40）：targets.push(ceiling)
若 preflopParticipants && effectiveStack/bigBlind>40 && raiseCount<2：
   sizingCap = max(minRaiseTo, max(bigBlind,currentBet)×(raiseCount?4.5:4))；过滤 target<=sizingCap
对每个 target：若 target>=maxRaiseTo && ceiling===maxRaiseTo 取 maxRaiseTo，否则 snapRaise(min(target,ceiling), minRaiseTo, ceiling, wagerUnit)
最终过滤 null、target>currentBet、target>heroRoundBet、去重、升序
~~~

**contestablePot / contestableReturn / expectedAdditionalCalls**：

- contestablePot(context, invest)（`:899-905`）：无 publicPlayers 时返回 pot；找 hero 行；cap=hero.committed+invest；返回 min(pot, Σ min(p.committed, cap))（**包含已弃牌玩家的 dead money，但被 hero 可争夺上限截断**）。
- contestableReturn(context, invest, equityInfo)（`:1022-1040`）：按 publicPlayers 的 committed（hero 额外加 invest）分层，每层金额 = (level-prev)×Σ(commitments>=level)，层权益取 subsetEquities[该层有资格对手组] 或 mean；hero 不存在时回退 mean×(pot+invest)。
- expectedAdditionalCalls(context,target,response)（`:907-923`）：无 publicPlayers 或无 opponents 时 needed×expectedCallers（needed=max(0,target-currentBet)）；否则按对手容量 min(player.stack, max(0,target-player.roundBet)) 加权 call/(call+fold) 后除以 anyCall/(1-anyRaise)（条件化到“有人跟且没人加”）。
- potContestGroups(context)（`:1008-1021`）：对每个 publicPlayers 的 committed 与 committed+1 两个层级，收集仍可能竞争该层的对手集合（!folded 且 committed+stack>=level），key=按字符串排序的 seat 列表；用于 subsetEquities。

### 6.9 selectCandidate（`js/ai-core.js:1557-1599`）

~~~text
若存在 terminalCall==='allin' 候选：只在 [该候选, fold] 中按 ev 排序取最高（不混合、不按 utility）——仅实验开关触发
过滤掉 terminalCall 且 ev<0 的候选（terminalCall 默认不存在）
若无候选 --> fallback {action: toCall>0?'fold':'check', amount:0, utility:0, ev:0, reason:'fallback'}
按 utility 降序；best = candidates[0]
若 best.utility<0：优先 fold/check 中的第一个作为 best
gapLimit = max(bigBlind×.18, pot×(0.03 + uncertaintyTolerance×.018))
eligible = utility >= best.utility-gapLimit 且 (utility >= -gapLimit×.35 或 action∈{fold,check})
若 eligible.length<=1 --> best
temperature = max(bigBlind×.10, pot×.012) × (0.75 + uncertainty×uncertaintyTolerance)
对 eligible 每个候选：
   balance = 1 / sqrt(1 + spotFrequencies[key]×.18)
   key = street + ':' + (toCall>0?'facing':'open') + ':' + action + ':' + sizeBucket(invest/max(bigBlind,pot+toCall))
   preference = 1
   若 reason==='credible_range_pressure' 且 calledEquity<.45，或 reason==='preflop_blocker_3bet'：preference = clamp(bluffFreq/.10, .3,2.2)
   若 candidate.bluffCatch：preference = clamp(heroCallFreq/.15, .6,2)
   weight = exp((utility-best.utility)/max(.01,temperature)) × balance × preference
roll = rng.next()×Σweight；按顺序减 weight 选第一个 roll<=0（否则 best）
spotFrequencies[key(chosen)] += 1
~~~

uncertainty 参数：翻前 = equity.standardError 或 .05；翻后 = equity.standardError（或 stdDev×(1-confidence)）。

### 6.10 legalize（`js/ai-core.js:1600-1626`）

~~~text
action = decision.action 或 (toCall>0?'fold':'check')
amount = max(0, round(finite(decision.amount)))
若 action 不在 legalActions：
   若 action==='allin' && legalActions 含 raise && maxRaiseTo>currentBet --> action='raise', amount=maxRaiseTo
   否则若 toCall>0 && 含 call --> action='call', amount=min(toCall,heroStack)
   否则若含 check --> action='check', amount=0
   否则 --> action='fold', amount=0
若 action==='call'：amount = min(toCall,heroStack)
若 action==='raise'：
   amount = clamp(amount, minRaiseTo, maxRaiseTo)
   若 amount>=maxRaiseTo && legalActions 含 allin --> action='allin', amount=heroStack
   否则 snapRaise(...)：若 null --> 回退 call/check/fold（金额相应）
若 action==='allin'：amount = heroStack
若 fold/check：amount=0
返回 {...decision, action, amount}
~~~

> adapter AIPlayer._legalize（`js/ai.js:973-1001`）还会处理 check 遇到 toCall>0 → fold 等；shared 路径下 decide 已 legalize 一次，adapter 再 legalize 一次（幂等）。Rust 建议只在核心 legalize 一次，adapter 仅做 allin/call/check 到规则引擎动作名的映射。

### 6.11 跨决策更新与 trace

**HandPlan.update(context, decision, equity, texture, response, profile)**（`js/ai-core.js:572-595`）：

| 条件（按顺序） | intent |
| --- | --- |
| equity>0.68 且 aggressive | value |
| equity>0.68 且非 aggressive | trap |
| equity>0.52 且 aggressive | thin_value |
| equity>0.52 且非 aggressive | pot_control |
| texture.hasStrongDraw 且 aggressive 且 street!=='river' | semi_bluff |
| aggressive | bluff |
| toCall>0 且 action==='call' | bluff_catch |
| 否则 | pot_control |

其余字段：maxRisk = round(heroStack×(value?.78 : semi_bluff?.46 : .32))；preferredSizes = aggressive?[amount]:[]；favorableRunouts 依 flushDraw/straightDraw；unfavorableRunouts 依 equity<.55；continueConditions 依 intent==='bluff'；abortConditions=['large_resistance']；responseToRaise 依 equity>.70 / hasStrongDraw；confidence=clamp(.35+|equity-.5|×.8+response.confidence×.2,.25,.92)。

**HandPlan.continuityBonus**（`js/ai-core.js:561-571`，作为 candidate.planBonus 加到 utility）：

| 条件 | 加成 |
| --- | --- |
| intent=value 且 aggressive | +pot×.025 |
| intent=semi_bluff 且 aggressive 且 street!=='river' | +pot×.018 |
| intent=bluff 且 aggressive 且 confidence>.55 | +pot×.012 |
| intent=pot_control 且 action∈{check,call} | +pot×.020 |
| intent=trap 且 toCall>0 且 aggressive | +pot×.040 |
| abortConditions 含 large_resistance 且 toCall>pot×.8 且 aggressive | -pot×.09 |

**SelfImage**（`js/ai-core.js:502-542`）：record 保存最近 80 条 {handId,street,action,amount,aggressive,intent}（hands Set 上限 256）；snapshot 返回：

~~~text
recent = actions 最近 28 条
tightness = clamp(1-(voluntary+3)/(len+12), .05,.95)
aggression = clamp((aggressive+3.2)/(len+8), .05,.95)
riverAggression = clamp((riverAgg+0.5)/(riverActions+4), 0,1)
shownBluffRate = (shownBluffs+1.6)/(showdowns+7)
recentlyBluffed = actions 最近 10 条中存在 intent==='bluff'
~~~

**storyCredibility**（`js/ai-core.js:1277-1291`）：action 非 raise/allin → .5；否则 score=.42 +（上街进攻者 +.12）+（延迟 cbet 且 pressureCard +.10）+（monotone 或 paired +.04）+ nutBlocker×.08 +（equity>.62 或 hasStrongDraw +.10）+（selfImage.tightness>.58 ? +.10×imageAwareness : 0）-（selfImage.aggression>.68 ? .07×imageAwareness : 0）-（plan.intent==='bluff' 且无 pressureCard 且无强听牌 ? .09:0）+（所有 read.capped ? +.08:0）；clamp(.12,.90)。

**lastTrace 字段**（`js/ai-core.js:1687-1718`；黄金语料逐字段记录，可直接作为 Rust 的单测断言对象）：

| 字段 | 内容 |
| --- | --- |
| strategyVersion | 'unified-v2' |
| parameters | variant、strategyFocus、rangeEvidenceWeight、calledRangeDiscount、preflopParticipationWeight、preflopRangeSharpness、observedPreflopRangeWeight、professionalWeight、postflopSelectionWeight、jointCallReturnWeight、competenceWeight（仅 target 焦点）、targetPrecisionWeight、conditionalRiskWeight、terminalRealizationWeight、rankedContinuationWeight、terminalCallWeight、riverCallWeight |
| handId / decisionId / intent | |
| equity | mean/stdDev/confidence/samples/timedOut/standardError/rangeConfidence/exact（4 位小数） |
| potOdds | toCall/max(1,pot+toCall) 4 位小数 |
| selfImage | snapshot() |
| opponents | 每 read：seat、rangeWidth(3 位)、estimatedStrength(3 位)、confidence(3 位，belief+model 平均)、foldsTooMuch、callsTooMuch |
| candidates | action、amount(round)、ev/risk/utility(2 位)、reason、terminalCall?、calledEquity?、response?{allFold,anyCall,anyRaise,expectedCallers} —— 按 utility 降序 |
| riverCfrEnabled | strategyFocus==='ten-player-25bb' && profile.riverCfrWeight>0 |
| riverSolver? | 见 §8.6 |
| chosen | {action, amount, utility(2 位)} |
| nextPlan | {id,intent,maxRisk,continueConditions,abortConditions,responseToRaise} |


## 7. RNG 与确定性

### 7.1 两个 RNG 不要混淆

| RNG | 算法 | 种子 | 使用位置 | Rust 现状 |
| --- | --- | --- | --- | --- |
| 概率核心 RNG | xorshift32（state ^= state<<13; state ^= state>>>17; state ^= state<<5） | `hashSeed(value)`（FNV-1a，输入 String(value || 'poker-probability')） | 概率/MC 抽样（poker-prob） | `poker_core::XorShift32Rng` 已实现 |
| AI RNG | **mulberry32 变体**（state += 0x6D2B79F5；双 imul 混合） | `hashSeedAI(text)`（FNV-1a 32 位，输入 String(text ?? 'poker-ai')） | AI 决策内部（estimateRangeEquity 抽样、selectCandidate 混合、CFR 策略抽样） | **需要新增**（不能复用 XorShift32Rng） |

**hashSeedAI（`js/ai-core.js:101-109` / stable `:69-77` 完全相同）**：

~~~text
hash = 2166136261 (u32)
for i in 0..text.length-1:
    hash = hash XOR text.charCodeAt(i)          // UTF-16 code unit（emoji 会占 2 个单元）
    hash = Math.imul(hash, 16777619)            // 32 位有符号乘法，结果按 u32 记忆
return hash >>> 0, 若为 0 则 0x9e3779b9
~~~

**SeededRng（`js/ai-core.js:111-125`）**：

~~~text
state: u32 = hashSeedAI(seedText)
next():
    state = state +u32 0x6D2B79F5               // JS 里是 this.state += inc（随后位运算隐式 ToInt32，等价 u32 环绕）
    t = state
    t = Math.imul(t XOR (t >>> 15), t OR 1)
    t = t XOR (t + Math.imul(t XOR (t >>> 7), t OR 61))
    return ((t XOR (t >>> 14)) >>> 0) / 4294967296
int(max): floor(next() × max(1, max))
normal(): u = max(1e-9, next())；v = next()；sqrt(-2×ln(u)) × cos(2×PI×v)   // 生产路径未使用
~~~

> JS 里 state 是普通 Number（可能短暂 >2^32），但下一次位运算会 ToInt32；由于每个决策都新建 RNG，单次决策的 next 调用量 < 1 万，state 不会超过 2^53。Rust 用 `u32` + `wrapping_add` 与 `wrapping_mul`（或 `i32.wrapping_mul` 再转 u32）逐位等价。

**测试向量**（由 `oracle/golden-ai.js` 从原实现导出，见 `tests/golden/ai/manifest.json` 的 `rngVectors`；Rust 单测必须逐值复现）：

| seedText | hashSeedAI |
| --- | ---: |
| `table:1:0:1:SOLID` | 2490085092 |
| `poker-table:standard:12:3:standard:river:5:3:TAG` | 3525023011 |
| `poker-table:shortdeck:4:2:shortdeck:flop:1:2:LAG` | 1746015351 |

seedText = `table:1:0:1:SOLID` 的前 8 个 next()：

~~~text
0.5340791516937315
0.8908965035807341
0.13685401086695492
0.11160188005305827
0.8373328370507807
0.8148538516834378
0.9530264236964285
0.6702359593473375
~~~

### 7.2 seed 来源（决策级）

~~~text
seedText = (rawContext.seed 若存在，否则 'table')
         + ':' + context.handId
         + ':' + this.seatId
         + ':' + context.decisionId
         + ':' + this.style
~~~

- `rawContext.seed`：本地 `'poker-table:standard'` / `'poker-table:shortdeck'`（`js/game.js:488`、`js/shortdeck.js:802`）；服务端同（`server/server-game.js:612`）。
- `context.handId`：本地 `handGeneration`；服务端 `game.handId`（递增）。
- `this.seatId`：brain 的座位；adapter 在每次 decide 前设 `brain.seatId = currentPlayerIndex ?? position`。
- `context.decisionId`：`decisionIdentity(variant, phase, actionsThisRound, seatId)`，例如 `standard:river:5:3`。
- `this.style`：人格 ID 字符串，例如 `SOLID`。
- **没有任何时间、进程、全局计数器参与**（worker 重建 brain 也用同一公式），所以固定输入 = 固定随机流。

### 7.3 每个决策的 RNG 消耗顺序（逐位复刻的关键）

**翻前决策**（street==='preflop'）：

1. `estimateRangeEquity`：
   - 精确河牌单挑路径不适用于翻前；
   - MC 路径：per sample 消耗 = 洗牌 `(samplers.length-1)` 次 `int` + 每 seat 的 `weightedPick` 尝试 + 补公共牌 guard 次数。
2. `analyzeTexture`：0。
3. `buildPostflopCandidates`：0（无随机）。
4. `selectCandidate`：0（eligible<=1）或 1（一次 `next()`，仅在需要混合时；注意先算 weights 再消耗随机数）。
5. 无 CFR（非河牌）。

**翻后决策**（flop/turn/river）：

1. `estimateRangeEquity`（同上；河牌单挑精确路径为 0 消耗）。
2. `analyzeTexture`：0。
3. `buildPostflopCandidates`：0。
4. `selectCandidate`：0 或 1。
5. `solveRiver`（仅门控通过）：求解本身**不消耗 rng**（CFR 无随机），仅在选出动作时消耗 1 次 `next()` 做策略抽样。

**weightedPick 的精确消耗**（`js/ai-core.js:649-670`）：
最多 22 次循环，每次 1 个 `next()`（算 target）；若 22 次全部撞牌失败，再进行 1 次 `next()` 做线性扫描。因此成功时通常是 1 个 next，撞牌时更多。

**补公共牌**（`:779-788`）：while board<5，每次 guard 循环 `candidate = deck[rng.int(deck.length)]`，直到抽到未使用牌；一般 1 个 next，撞牌则多次。

**seat 顺序洗牌**（`:764-765`）：`order = samplers.slice()`；对 j 从 len-1 到 1：`k = rng.int(j+1)`，swap。

**黄金语料记录**：每条决策的 `expect.rngCalls` 是本次 decide 期间 `SeededRng.next()` 的调用总数（由 exporter 在原型层计数）。Rust 测试建议同样暴露 `rng_calls()` 计数器并逐条对比；这是定位“某次 next 多/少一次”的最快手段。

语料统计（262 条决策，最终版本）：

| street | rngCalls min | max | mean |
| --- | ---: | ---: | ---: |
| preflop | 2673 | 7181 | 3919.2 |
| flop | 1301 | 3383 | 1928.7 |
| turn | 854 | 3044 | 1438.2 |
| river | 0 | 3149 | 529.0 |

（river 最小 0 = 精确河牌单挑/或选候选无需混合；target 桌单挑河牌精确路径 samples=990 且不消耗 rng。）

### 7.4 Math.random 的边界

生产决策路径**不使用 Math.random**。Math.random 只用于：
- 洗牌（engine/game/shortdeck/server 的 deck shuffle）；
- 人格/名字随机分配（createAIPlayers、createServerAi、房间补 AI）；
- AI 思考延迟（game.js/shortdeck.js 的 setTimeout 计算）；
- legacy AI 路径（`js/ai.js:387` 的 `const rng = Math.random()`，仅在 sharedBrain 缺失时执行）。

因此 golden-ai.js 仍然按黄金语料规则先 patch Math.random（mulberry32(0x9e3779b9)），但牌桌洗牌另用 mulberry32(固定 seed) 作为 ServerPokerGame 的 random；AI 决策完全由 `SeededRng(seedText)` 决定。
**Rust 要求**：引擎洗牌用注入 `RandomSource`；AI 决策内部用独立的 `AiRng`（mulberry32 变体）从 seedText 构造，两者不共享流。

### 7.5 时间预算与确定性

- `timeBudgetMs` 默认：`opponents.length>=4 || street==='river' ? 150 : 76`（normalizedContext），本地 adapter 传 150/80（`js/game.js:524`）。
- `estimateRangeEquity` 里 `budget = clamp(timeBudgetMs,20,180)`，**仅当耗时 >= budget×3 才提前 break**（紧急刹车），并且会置 `timedOut=true`；正常情况按 `maxSamples` 采样完毕。
- 原项目基准：连续筹码全场平均 8.47ms、P95 17.76ms；18 个河牌 Worker 场景平均 53.91ms、最大 99.10ms（`AI_BENCHMARK_REPORT_v1.5.8.md:42-46`）。
- **Rust 移植硬要求**：固定 seed 的测试/对拍必须用“按样本数”完成（虚拟时钟或禁用时间刹车），否则机器负载会改变决策。生产上保留时间刹车，但 trace 记录 `timedOut`，且只在极端负载触发。

## 8. River CFR（公开范围、有限树）

### 8.1 触发门控（`js/ai-core.js:1109-1115`）

`solveRiver` 只在**全部条件**满足时返回非 null；否则返回 null（通用决策保留）：

1. `profile.riverCfrWeight > 0`（标准默认 1；短牌 stable 无此字段 → 恒 false）
2. `context.strategyFocus === 'ten-player-25bb'`
3. `context.street === 'river'`
4. `context.activeOpponentSeats.length === 1`
5. `riverSolver` 模块已加载（Rust 恒可用）
6. hero 与 villain 都能在 `publicPlayers` 找到（hero=context.heroSeat，villain=唯一对手）
7. `!hero.allIn && !villain.allIn && hero.stack > context.toCall && villain.stack > 0`
8. `holeCards.length === 2 && communityCards.length === 5`
9. `publicPlayers` 中不存在第三个未弃牌且不是 hero/villain 的玩家（边池/多人保留通用算法）
10. `hero.committed - hero.roundBet === villain.committed - villain.roundBet`（本街之前双方投入相等）
11. `contestablePot(context, context.toCall) === context.pot`（没有 hero 无法争夺的死钱层）
12. `context.pot - hero.roundBet - villain.roundBet > 0`

### 8.2 公开范围 weights（`js/ai-core.js:1116-1124`）

- self = `this.publicSelfBelief`（若存在，按手重置）否则新建 `OpponentBelief(seatId)`；随后强制设置 `evidenceWeight/percentileWeight/preflopSharpness/adaptiveRangeWeight/publicStats`（来自 profile 与 publicSelfModel）。
- opponent = `reads[0].belief`。
- `weights(a,b) = [self.comboWeight(a,b,'standard',cache), opponent.comboWeight(a,b,'standard',cache)]`；cache 在同一 solve 内共享。
- **只排除公共牌**（river-solver 内部），不排除 hero 实际底牌（否则会泄漏给对手策略）。hero 自己的底牌只在求解结束后用于查自己的 bucket。

### 8.3 ranges()（`js/river-solver.js:5-25`）

~~~text
输入 {board, deck, weights, buckets=6}
校验 board 长度 5 且互不相同
known = board 的 cardKey 集合；available = deck 去掉 known
combos = 所有 i<j 的 [available[i], available[j]]：
    w = weights(cards)；要求 length===2 且两个权重有限且 >=0，否则抛错
    bitset：low/high 32 位掩码记录两张牌在 available 中的下标（用于精确撞牌排除）
    score = evaluate([...cards, ...board], false).score
    {cards, low, high, score, weights:w, bucket:0}
sorted = combos 按 score 升序
mass = Σ(w0+w1)；若 mass<=0 抛错
分桶：遍历同分组合组，group = Σ(w0+w1)，bucket = min(buckets-1, floor((before+group/2)/mass × buckets))；同分组合不拆桶；bucketByScore[score]=bucket；before += group
联合分布：cells 6×6 + signed 6×6（Float64Array）
  对每对 (a,b)（a 为 self 组合、b 为 opponent 组合）：
     若 (a.low & b.low) || (a.high & b.high) 则跳过（撞牌，精确排除）
     w = a.weights[0] × b.weights[1]
     cell = a.bucket×6 + b.bucket；cells[cell]+=w；signed[cell]+=w×sign(a.score-b.score)；total+=w
若 total<=0 抛错（无兼容私有组合）
deals = cells>0 的 36 格：{scores:[floor(i/6), i%6], weight:cells[i]/total, showdownSign:signed[i]/cells[i]}
返回 {deals, bucketByScore, combinations: combos.length}
~~~

标准河牌：available = 52-5 = 47 张，combinations = C(47,2) = 1081（与 river-solver.test 的断言一致）；deals ≤ 36。

### 8.4 game() 有限河牌树（`js/river-solver.js:26-41`）

~~~text
输入 {pot, stacks:[s0,s1], paid:[p0,p1], currentBet, minRaiseTo, unit, minBet, checkedBefore, canRaise, deals, maxRaises=1}
base = pot - p0 - p1                      // 本街之前的公共底池
ceiling = min(s0+p0, s1+p1)               // 有效可下注上限
校验：base>0；paid>=0；stacks>0；currentBet===max(p0,p1)；currentBet<=ceiling
每个 deal 的初始状态：
  {scores, sign, actor:0, paid:[p0,p1], bet:currentBet,
   lastRaise: currentBet ? minRaiseTo-currentBet : minBet,
   raises:0, checks: checkedBefore?1:0, history:[], end:null}
actions(s):
  call = s.bet - s.paid[s.actor]
  menu = call>0 ? ['fold','call'] : ['check']
  若 s.bet<ceiling && (s.history.length>0 || canRaise) && (!call || s.raises<maxRaises)：
     minimum = call>0 ? s.bet + s.lastRaise : minBet
     afterCall = base + paid[0]+paid[1] + call
     targets = [.33,.67,1].map(f => s.bet + round(afterCall×f/unit)×unit)；再 push ceiling
     对去重升序后的 target（clamp 到 [ceil(minimum/unit)×unit, ceiling]）：
        若 target>s.bet 且 (target===ceiling 或 target%unit===0)：menu.push((call>0?'raise':'bet') + ':' + target)
child(s,a):
  actor 翻转；复制 paid/history
  fold  --> end='fold', winner=1-s.actor
  call  --> paid[actor]=bet, end='showdown'
  check --> checks++；若 checks===2 --> end='showdown'
  下注/加注 --> target=Number(动作后缀)；paid[actor]=target；lastRaise=target-s.bet；raises+= (s.bet>0?1:0)；bet=target；checks=0
terminal(s) = s.end!==null；player(s)=actor
key(s,p) = p + ':' + s.scores[p] + ':' + s.history.join('|')     // 行动者自己的 bucket + 公开行动序列
payoff(s):
  fold: winner===0 ? base/2+paid[1] : -(base/2+paid[0])
  showdown: sign × (base/2 + paid[0])
~~~

> payoff 是相对量（零和、按 hero 视角）；solve() 最后补 `base/2 + paid[0]` 偏移还原为净筹码 EV。

### 8.5 CompiledCfr（`js/compiled-cfr.js:4-42`）

**编译**（一次）：从每个 deal 的初始状态深度优先遍历，把每个 (state) 展开为节点记录；终止节点存 payoff；非终止节点要求 player∈{0,1}（机会节点已全部展开为 deals），按 `game.key(state, player)` 聚合信息集（info），记录 actions；同一 info 的 actions/player 必须一致，否则抛错；每个 info 分配 `offset`（在 regrets/average/sigma/delta 中的槽位）。roots = [(rootNodeIndex, dealWeight)]。随后把 chance 概率从根传播到所有子节点。

数组结构（Rust 可直接照抄；规模万级）：

| 数组 | 类型 | 含义 |
| --- | --- | --- |
| infoIndex | Int32Array(nodes) | 节点所属 info，-1=终止 |
| childOffset/childCount | Int32Array(nodes) | children 区间 |
| children | Int32Array(edges) | 子节点下标 |
| values | Float64Array(nodes) | 终局 payoff 或迭代中的节点值 |
| r0/r1 | Float64Array(nodes) | 双方到达概率 |
| chance | Float64Array(nodes) | 机会（deal 权重）到达概率 |
| regrets/average/sigma/delta | Float64Array(slots) | 每个 info-action 一份 |

**迭代**（`:25-40`）：

~~~text
1. regret matching：sigma[offset+j] = max(0,regrets[offset+j]) / Σ max(0,regrets)（全 0 时均匀）
2. 前向：r0/r1 从根按 sigma 传播（自己行动乘 sigma，对手行动乘 1）；average[offset+j] += chance[i] × (player0 ? r0[i] : r1[i]) × sigma_j
3. 后向：values[i] = Σ_j sigma_j × values[child_j]
   weight = chance[i] × (player0 ? r1[i] : -r0[i])
   delta[offset+j] += weight × (values[child_j] - values[i])
4. regrets += delta
~~~

**run(n)**：要求安全整数且 1<=n<=100000。**policy()**：对每个 info 归一化 average（全 0 时均匀）。

### 8.6 solve() 与根收益（`js/river-solver.js:42-49`）

~~~text
abstract = ranges(options)
g = game({...options, deals:abstract.deals})
solver = new CompiledCfr(g).run(options.iterations || 800)
policy = solver.policy()
// 用平均策略重新计算各节点值（后向）：
values = solver.values.slice()
for i from nodes-1 down to 0:
    if infoIndex[i]<0: continue
    node = policy[infos[infoIndex[i]].key]
    values[i] = Σ_j node.probabilities[j] × values[children[childOffset[i]+j]]
// 根节点收益（按 deal 到达概率加权）：
for (i,p) of solver.roots:
    info = infos[infoIndex[i]]；key = info.key
    rootMass[key] += p
    rootValues[key][j] += p × values[children[...]]
最后对每个 key：rootValues[key][j] = rootValues[key][j]/rootMass[key] + (pot-paid0-paid1)/2 + paid0
返回 {policy, rootValues, bucketByScore, iterations, informationSets, treeNodes, elapsedMs, scope}
~~~

- `iterations` 固定 800（`js/ai-core.js:1124`）。
- `scope` 字符串固定：'heads-up river; six strength buckets; exact public-range card removal; at most one future raise; blockers within a bucket are averaged'。
- 规模实测（本语料 21 条 CFR 场景）：informationSets 12–204，treeNodes 180–3564。

### 8.7 solveRiver 适配与动作选择（`js/ai-core.js:1109-1136`）

~~~text
solved = riverSolver.solve({board, deck: buildDeck('standard'), weights, pot: context.pot,
    paid:[hero.roundBet, villain.roundBet], stacks:[hero.stack, villain.stack],
    currentBet: context.currentBet, minRaiseTo: context.minRaiseTo,
    minBet: context.bigBlind, unit: context.wagerUnit, checkedBefore, canRaise: context.canRaise,
    iterations: 800})
score = evaluate([...holeCards, ...communityCards], false).score
bucket = solved.bucketByScore[score]
node = solved.policy[ key(player=0, bucket, history='') ]   // 形如 '0:5:'
若 node 缺失：抛错 'river solver misses hero information set'（正常不应发生）
// 抽样平均策略：
roll = rng.next()；selected = node.actions[last]
for i in 0..actions.length-1: roll -= node.probabilities[i]；若 roll<=0 取 actions[i] 并 break
[kind, target] = selected.split(':')
ev = solved.rootValues[key][node.actions.indexOf(selected)]
effectiveCap = min(hero.stack+hero.roundBet, villain.stack+villain.roundBet)
amount = (Number(target)===effectiveCap && effectiveCap<hero.stack+hero.roundBet)
         ? ceil(effectiveCap/context.wagerUnit)×context.wagerUnit
         : (Number(target) || 0)
返回 decision {action: kind==='bet'?'raise':kind,
             amount: kind==='call'?context.toCall:amount,
             ev, utility:ev, reason:'public-range-river-cfr'}
返回 trace {iterations, informationSets, treeNodes, elapsedMs, bucket, checkedBefore, policy:node, actionValues, scope}
~~~

**checkedBefore 计算**（`:1118-1122`）：

~~~text
order(p) = p.positionFromButton===0 ? context.playerCount : p.positionFromButton
checkedBefore = context.currentBet===0 && (
     lastRiverPublicAction
        ? lastRiverPublicAction.seat===villain.seat && lastRiverPublicAction.action==='check'
        : Number.isFinite(hero.positionFromButton) && Number.isFinite(villain.positionFromButton) && order(hero)>order(villain)
)
~~~

（lastRiverPublicAction 由 observeAction/observeSelfAction 在河牌事件时更新；位置推断用于没有观察记录的场景。）

**奇数筹码向上取整**：当 solver 选择的下注目标 == 有效上限（cover 对手剩余），而该上限小于自己“本街已下注+剩余”，金额向上取整到 wagerUnit 网格（`ceil(effectiveCap/unit)×unit`），保证覆盖对手而不是留下零头；未被跟注的多余筹码由引擎退款规则退回。

### 8.8 CFR 适用范围与限制（必须写进 Rust 文档与测试）

1. 仅**单挑河牌**、双方都未全下、双方本街之前投入相等、无第三人、无 hero 无法争夺的死钱层。
2. 仅标准牌（短牌没有 CFR）。
3. 6 个私有强度桶：同分组合归同桶，桶内阻断牌信息被平均（明确限制）。
4. 公开下注菜单：约 1/3、2/3、1 倍底池 + 有效全下；最多一次未来再加注；不是完整无限注。
5. 对手策略信息集只含对手自己的桶分与公开行动序列；不接收实际底牌/牌堆/人格。
6. 每次行动重新求解（每决策独立 800 次迭代），不是持续重求解。
7. 求解本身无随机；只有最终按平均策略抽样消耗 1 次 rng。
8. 若求解后 selected 动作在 legalize 中被改写（例如金额被网格吸附/回退），以 legalize 结果为准；trace.riverSolver 保留求解信息。

### 8.9 性能与内存预算

- 原项目 18 个河牌 Worker 场景平均 53.91ms、最大 99.10ms；连续筹码全场平均 8.47ms、P95 17.76ms（`AI_BENCHMARK_REPORT_v1.5.8.md:42-46`）。
- 求解器 1000 次迭代的独立 lab 约 2.44 秒（`AI_STRATEGY_SPEC_v1.5.8.md` §14），实时 Bot 用 800 次 + 编译数组。
- 内存：强制 GC 后堆仅多约 0.36 MiB，但 RSS 约多 92 MiB（独立 Node 进程探针，`AI_BENCHMARK_REPORT_v1.5.8.md:46`）。
- Rust 目标（来自交接文档 Phase 4 建议）：CFR P95 < 50ms、max < 100ms（Windows 实测后调整）；信息集/树规模见 §8.6，可用 `Vec<f64>` 或复用 arena；**建议每次决策分配独立 arena，避免跨线程共享可变状态**。

### 8.10 信息集 key 与平局/分池语义

- key 形如 `'player:bucket:action1|action2|...'`，只含行动者自己的桶和公开行动；不含对手 bucket、不发公共牌、不含人格。
- payoff 用 `showdownSign`（该格内 self score - opponent score 的条件平均符号）：同桶内真实胜负以权重乘积条件化，不用“同桶一律平局”。
- 平局通过 sign=0 贡献 0 收益；分池（split pot）由 base/2 的对称结构体现；不做 ICM（当前游戏净筹码收益）。


## 9. 短牌策略（v1.5.6 冻结核心）

### 9.1 短牌游戏结构差异（影响 context）

| 项 | 标准 | 短牌 | 来源 |
| --- | --- | --- | --- |
| 牌数 | 52（2-A） | 36（6-A） | `js/ai-core.js:19-22` / stable `:17-20` |
| 强制下注 | SB/BB | 全员前注 + 庄家双倍前注 | `js/shortdeck.js:480-500, 676-700` |
| 有效单位（wagerUnit） | smallBlind | ante | `js/shortdeck.js:834` |
| bigBlind 字段 | 大盲 | minBet（默认 2×ante） | `js/shortdeck.js:833` |
| 位置 | BTN=0/SB=1/BB=2/... | 庄家=0，无 SB/BB 标记（isSmallBlind/isBigBlind 恒 false） | `js/shortdeck.js:799-838` |
| 牌型强弱 | 同花 < 葫芦 | **同花 > 葫芦**；A-6-7-8-9 顺子 | poker-core 已实现 |
| AI 核心 | ai-core | ai-shortdeck-stable | `js/ai-core.js:1044-1046` |
| River CFR | 目标配置默认启用 | **无** | stable 无 river-solver 依赖 |

短牌 adapter（`js/shortdeck.js:365-439`）：
- `ShortDeckAIPlayer extends AIPlayer`：构造后把 `config` 换成 `SD_STYLE_CONFIGS[style]`，并用同一份 config 以 `variant:'shortdeck'` 重建 `sharedBrain`（保证主线程/Worker/服务端 profile 同源——但见 §9.4 的服务端分叉）。
- `_decideShared` 强制注入 `shortDeckAICards`（`PokerProbabilityCore.evaluate(cards, true)`；不可用时回退 `evaluateSDHand`），**不接受调用方传入的标准评估器**。
- legacy 方法 `evaluateHandStrength/evaluatePreFlop` 只在 sharedBrain 缺失时用；Rust 不移植。

### 9.2 短牌 stable 核心与标准核心的逐项差异

以 `js/ai-shortdeck-stable.js` 为准，与 `js/ai-core.js` 对比：

| 模块 | 差异 |
| --- | --- |
| DEFAULT_PROFILE | 仅标准字段子集（无实验权重、无 rangePercentileWeight/preflopParticipationWeight 等）；rangeEvidenceWeight=0.70、calledRangeDiscount=0.055、preflopResponseWeight=0、conditionalCallerEquityWeight=0（`stable:23-43`） |
| PROFILE_OVERRIDES | 与标准完全相同（`stable:45-52`） |
| VARIANT_PROFILE_OVERRIDES | 只定义 `standard: {rangeEvidenceWeight:.45, calledRangeDiscount:.08}`；短牌无覆盖（`stable:54-58`）。由于短牌只用 variantProfiles.shortdeck，此覆盖对短牌无效 |
| positionCategory | **不同算法**：count===2 && distance===0 --> LP；isSB/isBB --> BL；distance===0 或 distance>=count-2 --> LP；distance<=max(1,floor(count×.32)) --> MP；否则 EP（`stable:107-117`） |
| OpponentBelief | 少 publicStats/preflopSharpness/adaptiveRangeWeight；observe() 翻前开池宽度固定 .24（标准按位置 .18/.24/.32）；无 rangeAction/openingPosition；comboWeight 无 observed 统计收缩与分位 cutoff，sigmoid 固定 ×15（`stable:246-351`） |
| BehavioralModel | 无 preflopRanges（普通/3bet/4bet 统计）；其余统计相同（`stable:360-446`） |
| SelfImage / HandPlan | 相同（`stable:448-542`） |
| analyzeTexture / boardComboFeatures / publicActionLikelihood / weightedPick / buildWeightedCombos | 相同（`stable:544-636`） |
| estimateRangeEquity | 精确河牌单挑路径存在但 responseSamples 只记录 {seats,relations}（无 weights/strengths/selectionScores）；采样预算/常量与标准相同（`stable:638-756`） |
| normalizedContext | 无 tablePlayerCount/publicPlayers 的 positionFromButton/isSB/isBB 字段；其余相同（`stable:758-810`） |
| expectedAdditionalCalls | 对手容量公式不同：`min(needed, max(0, player.stack - max(0, currentBet-player.roundBet)))`（`stable:827-841`） |
| conditionalCalledEquity | 只支持“对手 call/(call+fold)”概率，无 forceContinue/selection 分支（`stable:842-868`） |
| UnifiedPokerAI 结构 | 无 targetProfiles/professional/competence；无 publicSelfBelief/observeSelfAction/lastRiverPublicAction；**无 solveRiver、无 CFR**；无 postflopResponse/rankedPostflopResponse/terminalCallState（`stable:903-1313`） |
| preflopDecision（生产分支） | 只做 estimateRangeEquity + 简单候选过滤：fold/check/call/(invest<=toCall 的 allin) 保留，其余要求 strength >= threshold+facingRaises+style 或 credibleSteal；**没有**标准核心的 participation/开池范围/3-bet 过滤器（`stable:950-1019`） |
| realizationFactor | 相同但无 terminalRealizationWeight 分支（`stable:1020-1030`） |
| responseForRaise | 相同（preflopResponseWeight 存在但默认 0）（`stable:1046-1093`） |
| riskPenalty | 相同（`stable:1094-1106`） |
| raiseTargets | 相同但无 target-focus 的翻前 open 特例、无 deepPreflop sizingCap（`stable:1107-1133`） |
| buildPostflopCandidates | 简化版：call 用 contestableReturn×realization-invest，response 用 responseForRaise；raise 的 calledEquity 修正用 conditionalCallerEquityWeight（默认 0）；无 joint/participant/terminal 分支（`stable:1134-1175`） |
| selectCandidate | 相同但**没有** bluffFreq/heroCallFreq 偏好（只用 temperature×balance）（`stable:1180-1212`） |
| legalize | 相同（`stable:1213-1239`） |
| decide | profile=variantProfiles[variant]；sampleResponses=conditionalCallerEquityWeight>0；seed 公式相同；无 CFR 覆盖；trace.parameters 只有 {variant, rangeEvidenceWeight, calledRangeDiscount}；trace 无 riverCfrEnabled/riverSolver（`stable:1240-1312`） |

> **移植建议**：不要为了“代码复用”把 stable 核心与标准核心合并成一套参数化实现——两者在 positionCategory、belief 更新、preflop 候选过滤、response 路径、trace 结构上都不同。Rust 建议两个独立模块（`ai::standard`、`ai::shortdeck`），共享底层工具（牌力、牌面特征、RNG、Beat 结构），但决策流水线各自实现。

### 9.3 短牌 profile 来源分叉（原项目既有问题，需决策）

| 路径 | AIPlayer 类 | options.profile | 结果 |
| --- | --- | --- | --- |
| 本地桌面 | `ShortDeckAIPlayer` | `SD_STYLE_CONFIGS[style]` | stable brain 使用短牌专用风格参数 |
| 联机服务端 | `AIPlayer(..., 'shortdeck')` | `STYLE_CONFIGS[style]`（标准表） | stable brain 使用标准风格参数 |
| Worker | 主线程发 meta.profile（上面两者之一） | 同主线程 | worker 与主线程一致，但与另一路径可能不一致 |

两侧差异字段：vpip/aggression/threeBetFreq/bluffFreq/cbFreq/doubleBarrel/heroCallFreq 等（例如 SOLID：本地 aggression 0.45 vs 标准 0.55；LAG：本地 aggression 0.50 vs 标准 0.65）。
**黄金语料同时覆盖 local 与 server 两种 profile 来源**（`tests/golden/ai/decisions-shortdeck.jsonl` 中 `ai.type` + `ai.profileSource` 区分）。
**Rust 需要 Lead/产品决策**：是否统一为短牌专用表（推荐，本地为准）并接受联机短牌行为的小变化；若要求与联机旧行为完全一致则必须按入口选择 profile。见 §14 未决问题。

### 9.4 短牌 AI 延迟

与标准相同：`timeBudgetMs` 150/80，normalized 默认 76/150，紧急刹车 budget×3；Worker 路径 2500ms 超时回退主线程（`js/ai-worker-client.js:4`）。短牌 6 人桌、36 张牌使 sampler 组合数更小（C(29,2)=406 等），通常比标准更快。

## 10. AI 延迟与性能预算

### 10.1 原项目实测（v1.5.8）

| 场景 | 数值 | 来源 |
| --- | --- | --- |
| 18 个针对性河牌 Worker 场景 | 平均 53.91ms、最大 99.10ms | `AI_BENCHMARK_REPORT_v1.5.8.md:44` |
| 连续筹码全场（混有非 CFR） | 平均 8.47ms、P95 17.76ms | 同上 |
| 目标桌筛选阶段全场平均 | 7.0 → 7.5ms | 同上 |
| 25,770 次决策兼容测试 | 错误/超时 = 0 | `AI_BENCHMARK_REPORT_v1.5.8.md:36` |
| 1536 手连续筹码 | 16,331 决策，错误/超时 0，触发河牌求解 156 次 | 同上 |
| 每手强制河牌求解 180 次探针 | 平均决策开销显著上升；RSS +92MiB（独立进程） | `AI_BENCHMARK_REPORT_v1.5.8.md:46` |

### 10.2 Rust 目标（交接文档 Phase 4 建议值）

| 指标 | 目标 |
| --- | --- |
| 非 CFR 决策 P95 | < 16ms（单核；可用 rayon 并行采样） |
| CFR 决策 P95 / max | < 50ms / < 100ms（Windows 实测后调整） |
| 非法动作/筹码错误/采样超时 | = 0 |
| 统计强度 | 相同 seed/座位交换下 BB/100 与 JS v1.5.8 统计等价（不要求逐手一致） |

### 10.3 Rust 实现建议

1. **决策同步 API**：`decide(&mut self, ctx: &DecisionContext, rng: &mut AiRng) -> Decision`；上层（engine/app/server）用线程池/async 包装，保持 worker 语义（超时回退 + 结果一致性）。
2. **采样可并行但需保序**：若用 rayon 并行 MC，必须保持与串行一致的 RNG 抽取顺序（例如先生成 per-sample 随机种子序列，再并行消费）；否则固定 seed 语料无法对拍。**建议第一阶段串行**，性能不达标再改为“种子预生成 + 并行评估”。
3. **CFR 分配**：每次决策独立 arena（Vec<f64>/Vec<i32>），避免全局缓存；求解本身单核 ~毫秒级，先不做并行。
4. **profile 缓存**：按 (style, variant, adapter profile hash) 预计算 6×2 个 `AiProfile`，避免每次决策重建 Map。
5. **trace 可选**：黄金语料需要 trace；生产可只保留精简 trace（避免每决策大对象分配）。建议 `DecisionTrace` 用 `Option<Box<...>>` 由 feature/配置开关。


## 11. 黄金语料规格（tests/golden/ai/）

### 11.1 文件清单与哈希（最终版本）

| 文件 | 记录数 | 字节 | SHA256 |
| --- | ---: | ---: | --- |
| `decisions-standard-target.jsonl` | 106 | 1,113,865 | `5ddf9719746e40342c14cf31a9e87142bbf1cd023362ae2d217df96aaa6d259a` |
| `decisions-standard-general.jsonl` | 45 | 408,705 | `dd8c8fb55a478a6ea43e8dc07fd9e80909a5efed758a00ab64939042b2cead88` |
| `decisions-shortdeck.jsonl` | 73 | 673,709 | `b3687075bd7cc32b5f172f210427b02aade810a3ad1aa5534bffa9526ee4d9b7` |
| `decisions-synthetic.jsonl` | 38 | 260,328 | `10e32c447684db22587175cdeab5d596ebce4399887f3074062620997c6a707a` |
| `trajectories.jsonl` | 4 | 248,670 | `eeeab1c4dc7ab1a552c13eeed3bf32c7cdf4912e405e434e2ada70a0d766924a` |
| `manifest.json` | — | 4,864 | `6c0b942163530dc1c85f5618dedc886d146b9fb75f1d7d3b7bea3dbd88c0403f` |

**corpusSha256 = `55a0bcb69f9726f649beac5fbe28de8411885ff0cf9c85d99c391141a51d88be`**
（= 对上表前 5 个数据文件 sha256 按文件名顺序以换行拼接后再 sha256。）

复现（两次独立运行必须字节一致）：

~~~bash
cd texas-holdem-rs
node oracle/golden-ai.js                 # 生成（约 60-180s，取决于机器负载）
node oracle/golden-ai.js --verify        # 只读回放校验（约 25s）
node oracle/golden-ai.js --out=/tmp/ai2  # 生成到其它目录，用于两次哈希对比
sha256sum tests/golden/ai/*.jsonl tests/golden/ai/manifest.json
~~~

**确定性证据（2026-10-23 实测）**：
- 两次独立运行（默认目录 vs `/tmp/ai-corpus-run2`）：6 个文件逐个 sha256 完全一致；corpusSha256 均为 `55a0bcb6...`。
- `--verify`：`verify ok: 262 decisions + 27 trajectory decisions`（25.5s），即 262 条独立决策 + 27 条轨迹决策都能从语料字段重放并得到完全相同的 action/amount/rngCalls/trace/raw。
- 语料不含任何时间戳/随机数/机器相关字段；vm 沙箱内 `performance.now() === 0`。

### 11.2 生成方式（可审计）

`oracle/golden-ai.js` 的流程：

1. **先 patch Math.random** 为 `mulberry32(0x9e3779b9)`（黄金语料规则），然后按 index.html 顺序在 vm 中加载 13 个浏览器模块（含 `compiled-cfr.js`、`river-solver.js`，保证 CFR 激活）；sandbox 提供 `performance.now()=0`。
2. 另外 `require` 原仓库 `server/server-game.js`（ServerPokerGame + chooseServerAiAction）作为**驱动牌桌**的权威引擎（不参与策略计算）。
3. 10 张固定牌桌（种子、人数、筹码、盲注/前注、人格偏移见下）：

| table id | variant | aiVariant | seats | startingStack | blinds/ante | hands | seed | styleOffset |
| --- | --- | --- | ---: | ---: | --- | ---: | --- | ---: |
| std-target-t1 | standard | standard | 10 | 5000 | 100/200 | 20 | 0x5e1e0001 | 0 |
| std-target-t2 | standard | standard | 10 | 5000 | 100/200 | 20 | 0x5e1e0002 | 2 |
| std-target-t3 | standard | standard | 10 | 5000 | 100/200 | 20 | 0x5e1e0003 | 4 |
| std-target-t4 | standard | standard | 10 | 5000 | 100/200 | 20 | 0x5e1e0004 | 1 |
| std-target-t5 | standard | standard | 10 | 5000 | 100/200 | 20 | 0x5e1e0005 | 5 |
| std-general-t1 | standard | standard | 6 | 20000 | 50/100 | 12 | 0x5e1e0011 | 1 |
| std-general-t2 | standard | standard | 4 | 20000 | 40/80 | 12 | 0x5e1e0012 | 3 |
| std-general-t3 | standard | standard | 9 | 20000 | 50/100 | 10 | 0x5e1e0013 | 5 |
| sd-local-t1 | shortdeck | shortdeck-local | 6 | 5000 | ante 40 / minBet 80 | 12 | 0x5e1e0021 | 0 |
| sd-local-t2 | shortdeck | shortdeck-local | 6 | 20000 | ante 100 / minBet 200 | 10 | 0x5e1e0022 | 3 |
| sd-server-t1 | shortdeck | shortdeck | 6 | 5000 | ante 40 / minBet 80 | 12 | 0x5e1e0031 | 1 |
| sd-server-t2 | shortdeck | shortdeck | 6 | 20000 | ante 100 / minBet 200 | 10 | 0x5e1e0032 | 4 |

4. 每张桌的 AI 由 `new AIPlayer/ShortDeckAIPlayer(..., style, stack, seat, variant)` 构造，style 按 `STYLE_ORDER[(styleOffset+seat)%6]` 固定分配（保证六人格覆盖）；牌桌洗牌用 `mulberry32(seed)`。
5. 驱动循环与 `tools/ai-endurance.js` 相同：`startHand --> chooseServerAiAction --> game.act`，每桌 hand 数与 guard 上限固定；全部 10 张桌 errors=0。
6. 对每个决策点记录：raw gameState（去掉 evaluateCards）、AI 状态（holeCards/stack/position/numPlayers/bigBlind/isShortDeck）、该 AI 自上次决策以来收到的观测事件、JS 决策 + lastTrace（去掉 elapsedMs）+ 未取整的 raw equity/candidates。
7. 按规则从 harvested 池中挑选（每条规则有数量上限、按牌桌/座位/动作顺序确定性挑选），并用 inputSha256 去重；再追加 synthetic 场景与 trajectories。
8. 写 JSONL + manifest，并立即用 `--verify` 逻辑自检；两次运行输出一致才视为成功。

### 11.3 JSONL 行格式

#### A. 独立决策（4 个 decisions-*.jsonl）

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| id | 是 | 确定性 ID，例如 `std-target-t4-h6-standard-river-0-3-s3` |
| category | 是 | harvested-standard-target / harvested-standard-general / harvested-shortdeck-local / harvested-shortdeck-server / synthetic |
| tags | 是 | 覆盖标签（§11.5）；Rust 测试可按 tag 分组统计 |
| table | 是 | 牌桌参数（variant/aiVariant/seats/startingStack/blinds/ante/seed），仅元数据 |
| ai | 是 | {type: AIPlayer/ShortDeckAIPlayer, variant, style, seatId, name, profileSource: STYLE_CONFIGS/SD_STYLE_CONFIGS, startingStack} |
| aiState | 是 | {name, holeCards, stack, chipsInPot, position, numPlayers, bigBlind, isShortDeck, startingStack} —— 回放时设置到 AIPlayer |
| observes | 是 | 观测事件数组（最多 20 条，截断时 observesTruncated=true）：{op:'observe', event:{seatId,action,street,handId,meta}} 或 {op:'showdown', seatId, info} |
| observesTruncated | 否 | 是否截断了更早事件（默认最近 20 条） |
| context | 是 | 完整 raw decide 输入（字段见 §5.1；无 evaluateCards） |
| expect | 是 | {action, amount, rngCalls, trace, raw}，见 §11.4 |
| inputSha256 | 是 | sha256(JSON of {ai,aiState,observes,context})，用于跨文件去重 |

#### B. 轨迹（trajectories.jsonl）

| 字段 | 说明 |
| --- | --- |
| id / category='trajectory' / tags | 元数据 |
| table | 牌桌参数 |
| ai | 与 A 相同的 brain 描述（决定用哪个类与 profile） |
| ops | 有序操作数组：{op:'observe',event} | {op:'showdown',seatId,info} | {op:'decide', aiState, context, expect} |
| inputSha256 | 对 ops 的 decision 部分 + observations 的哈希 |

轨迹从该 AI 在第 1 手牌开始收到的第一条事件开始，按真实顺序覆盖 3 手牌（std-target-t1 seat3 / std-general-t1 seat1 / sd-local-t1 seat2 / sd-server-t1 seat0）；每条轨迹 6-8 次决策。回放时必须**顺序执行 ops**：observe --> recordOpponentAction / showdown --> recordShowdown / decide --> 设置 aiState 后调用 decide；这样能验证 HandPlan/SelfImage/BehavioralModel 跨决策状态。

#### C. manifest.json

- formatVersion=1、algoVersion=v1.5.8、strategyVersion=unified-v2、referenceCommit、generator/generateCommand/verifyCommand。
- determinism：seed 策略、虚拟时钟、Math.random patch、无时间戳声明。
- sourceSha256：8 个权威源文件 sha256（ai-core/ai/ai-shortdeck-stable/river-solver/compiled-cfr/probability-core/game-rules-core/server-game）。
- rngVectors：hashSeed 3 个 + 每个 seed 前 8 个 next()（§7.1）。
- files：每个文件 records/bytes/sha256；counts：总数/分类/街道/人格/动作统计；corpusSha256。

### 11.4 expect 字段与比较容差

| 字段 | 内容 | 建议判定 |
| --- | --- | --- |
| action | 最终动作（fold/check/call/raise/allin） | **必须精确一致**（硬失败）；边界例外见下 |
| amount | 最终金额（整数） | **必须精确一致** |
| rngCalls | 本决策 SeededRng.next 调用次数 | **必须精确一致**（定位 RNG 顺序问题的最强信号） |
| trace | JS lastTrace（elapsedMs 已删除）：parameters/equity(4位)/opponents/candidates(2位)/chosen/nextPlan/riverSolver | 逐字段比较；浮点容差见下 |
| raw | 未取整：{resolvedByRiverCfr, equity(mean/stdDev/standardError/rangeConfidence/subsetEquities/samples/confidence/exact/timedOut/stalled/responseSamples 计数), preLegalize, candidates[ev/risk/utility/calledEquity/response...]} | 数值诊断的主依据（比 trace 更精确） |

**建议容差（Rust 实现与 JS 结构对齐时）**：

| 量 | 容差 | 说明 |
| --- | --- | --- |
| action / amount / rngCalls / samples / exact / timedOut / 动作枚举 | 0 | 硬一致 |
| raw.equity.mean / stdDev / standardError / rangeConfidence | ≤1e-12（顺序完全一致时）；否则 ≤1e-6 且需解释 | 抽样路径对浮点顺序敏感 |
| raw.candidates[*].ev / utility / risk / calledEquity | 相对 1e-6 或绝对 1e-6 | f64 |
| trace.parameters / intent / reason / selfImage | 0 | 硬一致 |
| riverSolver.iterations / informationSets / treeNodes / bucket / checkedBefore / policy.actions | 0 | 硬一致（编译树结构必须一致） |
| riverSolver.policy.probabilities / actionValues | ≤1e-9（概率）/ 相对 1e-6（EV） | CFR 迭代顺序必须一致 |
| trace.opponents | width/strength ≤1e-9，bool 精确 | belief 公式 |

**边界差异规则**：若 action 不一致，但满足任一条，记为 `boundary`（单独计数，不算硬失败）：
1. 两者 top-2 candidate utility 差 < 1e-6（混合采样边界）；
2. equity.mean 差 > 1e-9 且涉及 fold/call 边界（说明抽样/分支实现有实质差异，需人工审查，不能简单放过）。

**初始通过率目标（task-9/后续实现阶段）**：
- 262 条独立决策：action+amount 精确一致 ≥95%（目标 ≥99%）；
- 数值在容差内 ≥99%；
- 21 条 CFR：action/amount 一致且 policy 全在 1e-9 内（CFR 是确定性的，应 100%）；
- 4 条轨迹 27 次决策：action+amount 精确一致（应 100%，否则说明跨决策状态机有 bug）；
- 先记录 baseline（按 tag 分组的通过率），再逐步提升；不把 100% 作为第一版硬 gate。

### 11.5 覆盖矩阵

| 维度 | 覆盖 | 数量/说明 |
| --- | --- | --- |
| variant | standard / shortdeck | 180 / 82 |
| 牌桌配置 | target 10人25BB / general 4-9人 | 120 条 target 焦点（含 CFR），其余 general |
| street | preflop / flop / turn / river | 118 / 57 / 39 / 48 |
| 动作 | fold/check/call/raise/allin | 85/77/15/58/27 |
| 人格 | 六种全覆盖 | TAG 71、LAG 65、SOLID 41、CALLING_STATION 36、MANIAC 28、TAGFISH 21 |
| 对手数 | 单挑/3人/多人 | heads-up 89、three-way 42、multiway 108 |
| 面对下注 vs 主动 | facing / open | 139 / 109（按 harvested tag） |
| 短牌 profile 来源 | local(SD_STYLE_CONFIGS) / server(STYLE_CONFIGS) | 58 / 24（含 synthetic） |
| River CFR | 触发 | 21 条（8 条真实牌桌 harvested + 13 条 synthetic：6 牌面 × checked/first + 1 facing bet） |
| CFR 拒绝路径 | 多人 / general focus | 2 条（验证 solver 不触发且走通用算法） |
| 边界 | short-stack / allin-opponent / cannot-raise / check-only / 3bet+ | short-stack 22、allin-opponent 28、cannot-raise 1、check-only 1、preflop-3bet-plus 12 |
| 精确河牌单挑 | exact=true samples=990（标准）/406-435（短牌） | 29 条 |
| 观测模型 | foldsTooMuch / callsTooMuch 先验差异 | 2 条 synthetic（synthetic-response-vs-folder/caller），另 harvested 中大量真实观测 |
| 跨决策状态 | trajectory | 4 条 × 6-8 决策 |

**语料局限（必须在 Rust 测试里认知）**：
1. 独立决策只保留“最近 20 条观测”，且不含该 AI 之前的决策（selfImage/plan 从空开始）；因此它们验证 belief/model/response/equity，但**不验证跨决策状态**——后者由 trajectories 覆盖。
2. 实验开关全为 0：terminalCall / riverCall / ranked / targetPrecision / jointReturn / conditionalRisk 的额外分支没有语料；实现这些分支时需按 §4.7 自行加单测（原仓库有对应测试可参考）。
3. CFR synthetic 场景的双方范围来自**空历史**（uniform range + 空 publicSelfBelief），因此只覆盖 solver 数学/结构；真实观测下的范围权重由 8 条 harvested CFR 覆盖（有真实 preflop/flop/turn 观测）。
4. 短牌只有 local/server 各一组筹码配置；更深的短牌筹码未覆盖。
5. JS 端以虚拟时钟跑满固定样本；语料不覆盖 timedOut 分支（真实生产极端负载才会出现）。

## 12. 移植风险 Top 5

### 风险 1：RNG 序列与浮点顺序（最高风险）

- **症状**：action 在不同决策点随机漂移；rngCalls 对不上；equity.mean 有微小差异导致混合/边界翻转。
- **根因**：AI RNG 是自定义 mulberry32 变体 + FNV 种子；每个决策一个流；加权抽样/洗牌/board 补牌的消耗顺序严格；候选 EV 的乘加顺序影响 1e-16 级差异，可能在 utility 接近时改变选择。
- **缓解**：
  1. 先实现 `AiRng` 并通过 §7.1 测试向量（hashSeed + next8）；
  2. 对每条黄金决策比较 `rngCalls`（最灵敏）；
  3. 逐函数对拍：estimateRangeEquity（mean/samples/responseSamples 计数）、buildPostflopCandidates（候选 EV/utility/response）、selectCandidate（是否混合）；
  4. 决策 action 不同的 case 单独基线化，按 tag 统计，不静默忽略。
- **验收**：262 条中 rngCalls 精确一致 ≥99%，action/amount 精确一致 ≥95%（目标 99%）。

### 风险 2：engine ↔ AI 的 action meta 契约

- **症状**：belief rangeWidth/foldRate 与 JS 不同 → equity/response 全线漂移；不报错但行为劣化。
- **根因**：Observe 事件的 meta（尤其 betFraction 分母 `max(1,minimumBet,potBefore)`、toCallBefore、aggressive、preflopRaiseCountBefore、position 字段）由 engine 生成；Rust engine 若字段语义不同，AI 学习结果不同。
- **缓解**：
  1. 把 `buildActionMeta` 的字段定义写成 engine 契约测试（§5.3）；
  2. 用语料 observes 反推：Rust 生成同样的 action 序列时必须产出逐字段相同的 meta（可在 golden 测试里先回放 observe 再断言 belief 状态摘要）；
  3. 三路径（local/worker/server）共用同一 adapter，避免 profile 来源分叉（短牌 local/server 分叉是已知例外，见风险 4）。

### 风险 3：River CFR 的数学/结构/性能

- **症状**：CFR case 的 bucket/policy/actionValues 不一致；或求解超时破坏延迟预算。
- **根因**：ranges() 的 6 桶分配（含同分组合不拆桶、按质量中点）、撞牌排除（bitset 精确）、树结构（下注菜单/最多一次再加/checkedBefore）、CompiledCfr 迭代顺序（前向 reach + 后向 regret）、solve() 的 rootValues 还原公式、solveRiver 的奇数筹码向上取整与 legalize 后的动作改写，任何一处不同都会改变策略/EV。
- **缓解**：
  1. 先做独立 oracle：用递归 tabular CFR（tools/tabular-cfr.js 的 Rust 等价）对照 CompiledCfr；Kuhn 均衡值 -1/18 单测；
  2. ranges()/game() 用固定输入断言（combinations=1081、deals 数量/权重、actions 菜单、payoff 值）；
  3. 黄金语料 21 条 CFR 要求 policy ≤1e-9、action 精确；每条记录都带 actionValues 便于定位；
  4. 性能：先串行，用 criterion 记录 800 迭代耗时；若 P95 超标，再考虑 arena/预分配/缓存 bucketByScore，而不是改变迭代顺序。

### 风险 4：路径/profile 分叉与信息边界

- **症状**：同一手牌本地与联机决策不同；或隐藏信息泄漏（对手底牌/人格进入决策）。
- **根因**：
  - 短牌 local 用 `SD_STYLE_CONFIGS`、server 用 `STYLE_CONFIGS`（原项目既有分叉）；
  - Worker 用 meta.profile 重建 brain，如果 Rust adapter 序列化 profile 不完整就会漂移；
  - publicSelfBelief/publicSelfModel 只允许来自公开行动；CFR ranges 不能删除 hero 真实底牌（必须只排除公共牌）。
- **缓解**：
  1. 把 profile 合并与来源写成显式 `ProfileSource` 枚举（StandardStyle / ShortDeckLocalStyle / ShortDeckServerStyle），语料已分别覆盖；
  2. Rust 单测：替换对手底牌/牌堆/人格不影响决策（原 river-solver.test 的范式）；
  3. 三条路径各自构造 context 的 adapter 必须有同一份字段清单与序列化测试。

### 风险 5：时间预算 vs 固定样本确定性

- **症状**：同一 seed 两次运行决策不同；CI flaky。
- **根因**：JS 用 `performance.now` 做紧急刹车；Rust 若直接用墙钟截断采样，机器负载会改变样本数 → 决策变化。
- **缓解**：
  1. 默认按 `maxSamples` 采样完毕；时间预算只做安全阀并在 trace 标记 `timedOut`；
  2. 黄金测试用虚拟时钟/禁用刹车（与 exporter 相同）；
  3. 若未来改为“时间可配置预算”，必须把实际 samples 记入 trace 并允许 golden test 允许 samples 差异但要求决策在容差内——不建议第一版这样做。

## 13. Rust 实现工作量拆分（建议）

按 1 名熟悉 Rust 的工程师、单人串行估算（并行 agent 可压缩墙钟时间，但接口/审查仍需串行）：

| # | 任务 | 交付 | 预估 person-day |
| --- | --- | --- | ---: |
| 1 | 公共骨架：lib/profile/rng/input/normalizedContext/AiProfile 合并、六人格表、positionCategory、起手强度、startingRangeCutoff、boardComboFeatures、analyzeTexture | 单元测试 + RNG 向量测试 | 3.0 |
| 2 | 状态模型：OpponentBelief（observe/comboWeight/estimatedStrength）、BehavioralModel、SelfImage、HandPlan | 单测 + 与语料 trace.opponents/selfImage 对拍 | 2.5 |
| 3 | 范围权益：精确河牌单挑、MC 采样（顺序/预算/responseSamples/potContestGroups/subsetEquities） | 与整条决策的 equity 对拍 | 4.0 |
| 4 | 翻前决策：preflopDecision（参与范围/防守范围/3bet 过滤器/profile 开关） | preflop 语料 118 条对拍 | 3.0 |
| 5 | 翻后候选与回应：responseForRaise/postflopResponse/preflopResponse、realizationFactor/riskPenalty/raiseTargets/buildPostflopCandidates、selectCandidate、legalize | flop/turn/river 语料对拍、candidate EV 容差 | 4.5 |
| 6 | River CFR：ranges/game/CompiledCfr/solve/solveRiver 适配 + 独立 oracle 对照 | 21 条 CFR 语料 + Kuhn 均衡 + 结构单测 | 5.0 |
| 7 | 短牌：stable 核心（可作为独立模块；与标准共享底层工具）、短牌 profile 分叉、ShortDeckAIPlayer 语义 | 82 条短牌语料 | 3.5 |
| 8 | adapter/trace/worker 语义：AIPlayer 适配、recordOpponentAction/recordShowdown、trace 结构、超时回退策略 | 轨迹 4 条全绿 | 2.0 |
| 9 | 黄金测试 harness + 差异报告：读 JSONL、回放、按 tag 统计、boundary 分类、CI 集成 | 测试工程 + baseline 报告 | 2.5 |
| 10 | 基准与自对局：criterion（决策/CFR）、ai-lab 等价 harness（BB/100、seed、座位交换）、性能优化 | benches + 报告 | 4.0 |
| 合计 | | | **34 person-day** |

**串行关键路径**：1 → 2 → 3 → 5（最长）；4/6/7 可在 1/3 完成后并行。并行 3 人时墙钟约 3-4 周（含集成/验收/性能调优），与交接文档 Phase 4 “2-4 周”乐观值相比偏慢，主要因为要让“数值可对拍”而不只是“能跑”。

**验收里程碑**：
- M1（骨架+1~3）：262 条语料可回放，equity/samples 数值基线报告；
- M2（4~5）：非 CFR 决策 ≥95% 动作一致；
- M3（6）：21 条 CFR 全绿（policy 1e-9）；
- M4（7~9）：短牌 + 轨迹全绿，总通过率 ≥99%；
- M5（10）：BB/100 统计等价、延迟目标、criterion 报告。

## 14. 未决问题（需 Lead/用户拍板）

| # | 问题 | 建议默认 | 影响 |
| --- | --- | --- | --- |
| 1 | AI 验收门槛：262 条黄金语料中动作精确一致率要求多少？ | 第一版 ≥95% 硬 gate、≥99% 目标；boundary 分类单独报告 | task-9/Phase 4 验收 |
| 2 | 短牌联机路径的 profile 是否统一为 SD_STYLE_CONFIGS（本地为准）？ | **已决策（Lead 2026-10-23）**：统一为 SD_STYLE_CONFIGS；联机旧行为（STYLE_CONFIGS）差异记录为已知变化，语料保留 server-profile 用例作为回归对照 | 短牌模块接口 |
| 3 | 是否移植 ai.js 的 legacy 决策路径（sharedBrain 缺失时）？ | 不移植（生产不可达）；只保留 adapter 语义 | 可省 ~1 人日 |
| 4 | MC 采样是否允许 rayon 并行？ | 第一版串行；若并行必须保序或改为独立子流 + 重新生成黄金语料 | 性能/确定性 |
| 5 | trace 是否默认开启/保留哪些字段？ | 默认精简（决策+equity 摘要），调试/黄金测试开启完整 trace | 内存/UI 调试面板 |
| 6 | 时间预算策略：是否保留 JS 的 150/76ms 默认与 ×3 紧急刹车？ | 保留语义，但 Rust 默认按样本数；预算仅安全阀 | 延迟/CI 稳定性 |
| 7 | 服务端 AI 延迟（aiDelay 800/1800/3200ms）与 AI 计算时间的关系 | 沿用协议层延迟；AI 计算同步完成后由 server 控制节奏 | poker-server 集成 |

### 14.1 已知语料缺口（task-9 实现时发现，后续重新生成语料时修复）

1. **synthetic 用例的 `ai.seatId` 与 `aiState.position` 缺失**：`oracle/golden-ai.js` 的 `buildSyntheticCase` 从 context 取 `seatId`，但 synthetic context 使用 `heroSeat/currentPlayerIndex`，导致这两个字段为 undefined 且被 JSON 丢弃（harvested 用例字段完整）。Rust golden harness 已用 fallback：`ai.seatId → aiState.position → context.heroSeat/currentPlayerIndex → 0`。不影响语料语义（hero 座位可由 context 推出），但重新生成语料时应在 generator 中补上。
2. **synthetic CFR 场景的 `ai.seatId` 同样缺失**，river/task-10 的测试如需构造 `AIPlayer` 也应使用同样 fallback；solver 入参本身与 `ai` 字段无关（从 `context` + `observes` 构造），不受影响。
3. **语料生成器版本与语料哈希**：若重新生成（修复上述字段），`corpusSha256` 会变化，需同步更新 `tests/golden/ai/manifest.json`、本规格 §11.1 与 task-4 报告；在 task-9~11 验收前**不重新生成**，以已冻结哈希为准。

## 附录 A：常量与行号索引

### A.1 ai-core.js（标准核心）

| 常量/函数 | 行号 |
| --- | --- |
| MODEL_HAND_LIMIT=256 | 23 |
| DEFAULT_PROFILE | 25-60 |
| PROFILE_OVERRIDES | 62-69 |
| VARIANT_PROFILE_OVERRIDES（standard 7 项） | 72-75 |
| PROFESSIONAL_PROFILES（否决） | 77-85 |
| COMPETENT_STYLE_OVERRIDES（否决） | 86-90 |
| clamp/finite | 92-99 |
| hashSeed / SeededRng | 101-125 |
| normalizeCard / buildDeck | 127-137 |
| positionCategory | 139-148 |
| startingHandStrength | 150-179 |
| comboFeatures | 181-194 |
| startingRangeCutoff | 196-205 |
| boardComboFeatures（value 表/draw/nutBlocker） | 207-258 |
| publicActionLikelihood | 260-275 |
| OpponentBelief（observe/comboWeight/estimatedStrength） | 277-394 |
| sizeBucket | 396-401 |
| BehavioralModel | 403-500 |
| SelfImage | 502-542 |
| HandPlan（continuityBonus/update） | 544-596 |
| analyzeTexture | 598-647 |
| weightedPick / buildWeightedCombos | 649-685 |
| estimateRangeEquity（精确/MC/预算/responseSamples） | 692-834 |
| normalizedContext | 836-892 |
| betFractionForTarget / contestablePot / expectedAdditionalCalls | 894-923 |
| responseCallProbabilities / conditionalCalledEquity / participantEquityInfo / jointAdditionalReturn / potContestGroups / contestableReturn | 925-1040 |
| UnifiedPokerAI 构造/profile 合并 | 1042-1073 |
| ensureOpponent / beginHand / observeSelfAction / observeAction | 1074-1108 |
| solveRiver | 1109-1136 |
| observeShowdown / activeReads | 1137-1146 |
| preflopDecision（生产分支/legacy） | 1147-1248 |
| realizationFactor / terminalCallState / storyCredibility | 1249-1291 |
| preflopResponse / responseForRaise | 1292-1378 |
| postflopResponse / rankedPostflopResponse | 1379-1451 |
| riskPenalty / raiseTargets | 1452-1498 |
| buildPostflopCandidates | 1499-1552 |
| spotKey / selectCandidate | 1553-1599 |
| legalize / decide / trace | 1600-1720 |
| 导出 | 1723-1749 |

### A.2 ai.js / ai-shortdeck-stable.js / river-solver.js / compiled-cfr.js

| 常量/函数 | 文件:行号 |
| --- | --- |
| AI_STYLES | ai.js:54 |
| STYLE_CONFIGS | ai.js:59-66 |
| RANGE_TEMPLATES / OpponentRange / PlayerModel（legacy） | ai.js:72-242 |
| analyzeBoardTexture / getPositionCategoryFromButton / getPreflopStrength（legacy） | ai.js:248-333 |
| AIPlayer 构造（sharedBrain） | ai.js:341-363 |
| decide legacy | ai.js:369-522 |
| _estimateEquity / _estimateFoldEq / _getRaiseSizes / _applyHeuristics / _preflopDecide（legacy） | ai.js:528-828 |
| recordOpponentAction / decideAsync / recordShowdown / reset / dispose | ai.js:834-922 |
| _decideShared / _act / _legalize | ai.js:925-1001 |
| _riverBlockerScore | ai.js:1003-1016 |
| STYLE_CONFIGS 导出 | ai.js:1042-1056 |
| stable DEFAULT_PROFILE / PROFILE_OVERRIDES / VARIANT_PROFILE_OVERRIDES | stable:23-58 |
| stable hashSeed/SeededRng | stable:69-93 |
| stable positionCategory | stable:107-117 |
| stable startingHandStrength/boardComboFeatures/publicActionLikelihood | stable:119-244 |
| stable OpponentBelief | stable:246-351 |
| stable BehavioralModel / SelfImage / HandPlan | stable:360-542 |
| stable analyzeTexture/weightedPick/estimateRangeEquity | stable:544-756 |
| stable normalizedContext | stable:758-810 |
| stable expectedAdditionalCalls/conditionalCalledEquity/contestableReturn | stable:825-901 |
| stable UnifiedPokerAI（构造/observe/preflop/response/risk/raise/candidates/select/legalize/decide） | stable:903-1313 |
| river-solver key | river-solver.js:4 |
| ranges() | river-solver.js:5-25 |
| game() | river-solver.js:26-41 |
| solve() | river-solver.js:42-49 |
| CompiledCfr 构造/编译 | compiled-cfr.js:4-24 |
| CompiledCfr.iteration | compiled-cfr.js:25-40 |
| CompiledCfr.run/policy | compiled-cfr.js:41-42 |
| ai-worker DECIDE_TIMEOUT_MS=2500 | ai-worker-client.js:4 |
| ai-worker importScripts/消息类型 | ai-worker.js:3-52 |
| createServerAi / chooseServerAiAction | server-game.js:592-673 |
| notifyAIsOfAction / notifyAIsOfShowdown | server-game.js:351-380, 534-554 |
| 本地/服务端 gameState 构造 | game.js:485-525；shortdeck.js:799-838 |
| wagerUnit/raiseBounds/snapRaise/decisionIdentity/buildActionMeta | game-rules-core.js:8-53 |
| SD_STYLE_CONFIGS / ShortDeckAIPlayer / createSDAIPlayers | shortdeck.js:290-450 |

## 附录 B：ai-core 导出函数清单（Rust 建议对应关系）

| JS 导出 | Rust 建议 |
| --- | --- |
| DEFAULT_PROFILE / PROFILE_OVERRIDES / VARIANT_PROFILE_OVERRIDES / MODEL_HAND_LIMIT | profile.rs 常量 |
| SeededRng | rng.rs::AiRng |
| OpponentBelief / BehavioralModel / SelfImage / HandPlan | belief.rs / model.rs / self_image.rs / plan.rs |
| UnifiedPokerAI | brain（standard core）：decision.rs 或 brain.rs |
| analyzeTexture / boardComboFeatures / startingHandStrength / startingRangeCutoff | math.rs |
| contestablePot / contestableReturn / expectedAdditionalCalls / conditionalCalledEquity / jointAdditionalReturn | math.rs 或 pot.rs |
| estimateRangeEquity | equity.rs |
| normalizedContext | input.rs |
| positionCategory / buildDeck / hashSeed | math.rs / rng.rs |
| （stable 导出） | shortdeck.rs |

## 附录 C：术语

| 术语 | 含义 |
| --- | --- |
| belief | 对某对手当前范围的单次决策内估计（OpponentBelief） |
| model | 跨手公开统计（BehavioralModel）：vpip/pfr/foldRate/aggressionRate 等 |
| strategyFocus | 'ten-player-25bb'（目标桌）或 'general' |
| CFR | counterfactual regret minimization；本项目为有限河牌树的 tabular CFR |
| bucket | 河牌范围内按牌力分数质量中点分的 6 个私有强度组 |
| realizationFactor | 权益实现率（位置/多人/听牌/深度修正） |
| terminal call | 跟注即全下/关闭行动的终局跟注（实验开关，默认关） |
| profileSource | 人格参数来源：STYLE_CONFIGS（标准/联机短牌）或 SD_STYLE_CONFIGS（本地短牌） |

## 附录 D：本任务要求对照（自检）

| task-4 要求 | 本文/交付 |
| --- | --- |
| 模块职责 | §2、§3 |
| 人格/风格表（全部数值） | §4.2-§4.6 |
| PlayerModel/OpponentRange 数据结构与更新公式 | §4.5 相关 + §6.5/§6.6（生产用 ai-core 的 BehavioralModel/OpponentBelief）；legacy 明确不移植（§3.3、§12 风险 4） |
| 决策流水线（范围权益→合法动作收益→风险/对手回应→混合采样） | §6 全节 |
| RNG 消耗顺序与 seed 来源 | §7 |
| CFR（公开范围/撞牌排除/回应树/800 迭代/混合策略/compiled 表结构） | §8 |
| 短牌策略差异 | §9 |
| AI 延迟预算 | §10 |
| 所有魔数常量（含行号） | 正文各节 + 附录 A |
| 固定 seed 决策黄金语料（可复现/哈希） | §11 + tests/golden/ai/ |
| 语料规模/耗时/决策分布/两次运行一致性 | §11.1、§11.5；manfiest counts |
| 移植风险 Top5 | §12 |
| 工作量拆分 | §13 |

