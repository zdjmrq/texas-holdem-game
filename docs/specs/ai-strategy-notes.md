# AI/CFR 移植工作笔记（ai-strategy-notes）

> 配套 `docs/specs/ai-strategy.md`（规格，权威）；本文是执行笔记：行号索引、RNG 消耗清单、语料回放注意事项、给 task-9/10/11 的接口草案与分工。
> 作者：ai-analyst；日期：2026-10-23。

## 1. 快速导航

- **规格**：`docs/specs/ai-strategy.md`（1714 行）——先读 §0-§3 确定权威路径，再按任务查 §4-§14。
- **语料**：`tests/golden/ai/`（262 独立决策 + 4 条轨迹/27 决策；21 条 River CFR；82 条短牌）。
- **生成器**：`oracle/golden-ai.js`（Node 24，无依赖；`node oracle/golden-ai.js` 生成，`--verify` 只读回放）。
- **原实现**：`reference/texas-holdem-game` @ e0c6c6f；行号均相对该仓库根。
- **语料 SHA**：corpusSha256 = `55a0bcb69f9726f649beac5fbe28de8411885ff0cf9c85d99c391141a51d88be`。

## 2. RNG 消耗顺序（逐项 checklist）

实现 `AiRng` 后，按下面顺序逐行核对；每条黄金决策的 `expect.rngCalls` 是硬校验。

### 2.1 每个决策开始

1. `normalizedContext` 不消耗 RNG。
2. `seedText = raw.seed 或 'table' + ':' + handId + ':' + seatId + ':' + decisionId + ':' + style`。
3. `AiRng::new(hash_seed(seedText))`，state=hashSeed（u32；0 → 0x9e3779b9）。
4. `beginHand` 不消耗 RNG。

### 2.2 翻前（street==='preflop'）

```text
preflopDecision:
  estimateRangeEquity:            # 见 2.4，先于一切
  analyzeTexture:                 0
  startingHandStrength:           0
  buildPostflopCandidates:        0
  selectCandidate:                0 或 1（eligible<=1 时为 0；否则先算完 weights，最后 1 次 next）
  （无 solveRiver）
```

### 2.3 翻后（flop/turn/river）

```text
estimateRangeEquity:  精确河牌单挑（board=5 且 samplers=1）时为 0；否则见 2.4
analyzeTexture:       0
buildPostflopCandidates: 0
selectCandidate:      0 或 1
solveRiver:           门控通过时 1（策略抽样）；求解内部无 RNG
```

### 2.4 estimateRangeEquity 内部

```text
已知牌 known = hole + board（cardKey 集合）
若 5 张 board 且 samplers.length===1：纯枚举（无 RNG），返回 exact=true, samples=C(45,2)=990（标准）
否则：
  budget = clamp(timeBudgetMs,20,180)；EMERGENCY_GRACE=3
  maxSamples = max(120, floor(mcSamples || baseline+riverBonus))
      baseline = numOpponents>=4?240 : >=2?320 : 420；riverBonus = (board==5)?40:0
  minimumSamples = numOpponents>=4?40:56
  while samples<maxSamples:
      if now-start >= budget*3: 若 samples>=min 则 timedOut=true 并 break；否则继续
      used = copy(known)
      order = samplers.slice()
      for j=len-1..1: k=rng.int(j+1); swap(order[j],order[k])
      for sampler of order:
          combo = weightedPick(sampler, rng, used)
          if !combo: valid=false; break
          used += combo 两张 key
      if !valid: 若 samples===0 且 attempt>=5 --> stalled=true,break；否则 continue
      while board.length<5:
          for guard<30: card = deck[rng.int(deck.length)]; if !used.has(card) 选它（每次 guard 1 次 next）
          if !card: valid=false;break
      if !valid: continue
      hero = evaluate(hole+board)
      对每个对手：score=evaluate(opp+board)；>hero 则 beaten；===hero 则 ties++
      outcome = beaten?0:1/(ties+1)
      subsetGroups 更新 group.sum
      responseSamples：relations/strengths 或 selectionScores（+ranked 的 currentScores/currentDraws）
      sum+=outcome; sumsq+=outcome^2; samples++
```

**weightedPick**：最多 22 次循环，每次 1 个 next（target=next()*totalWeight 后二分）；22 次失败后再 1 个 next 做线性扫描。
**采样顺序洗牌**：samplers.length-1 次 rng.int。
**注意**：estimator 的精确路径不产生 responseSamples 的 weights/strengths（只有 relations），标准与非标准版本都如此；只有 exact 河牌单挑会填 weights。

### 2.5 selectCandidate

```text
best = max utility（sort 后 candidates[0]）
gapLimit = max(bb*0.18, pot*(0.03+uncertaintyTolerance*0.018))
eligible = ...
若 eligible<=1：直接返回 best（0 次 next）
否则：
  temperature = max(bb*0.10, pot*0.012) * (0.75 + uncertainty*uncertaintyTolerance)
  weights[i] = exp((u_i-best.u)/max(0.01,temp)) * balance_i * preference_i
  roll = rng.next() * total       # 唯一一次 next
  选第一个 roll<=0
```

### 2.6 solveRiver 的动作抽样

```text
solved = river::solve(...)        # 无 RNG
node = solved.policy['0:'+bucket+':']
if 不存在: throw（正常不应发生）
roll = rng.next()                 # 唯一一次 next
for i: roll -= prob[i]; if roll<=0 选 actions[i]
```

### 2.7 计数校验

语料 `expect.rngCalls` 按街统计：preflop 2673-7181（mean 3919）、flop 1301-3383（mean 1929）、turn 854-3044（mean 1438）、river 0-3149（mean 529）。若 Rust 某条决策 rngCalls 差 1，优先检查 selectCandidate 是否多/少一次 next、weightedPick 重试次数、board 补牌 guard、CFR 策略抽样。

## 3. 黄金语料回放注意事项

1. **设置 aiState 顺序**：先构造 AIPlayer（参数 name/style/startingStack/seatId/variant 或 ShortDeckAIPlayer），再设置 holeCards/stack/chipsInPot/position/numPlayers/bigBlind/isShortDeck，再回放 observes，最后 decide。
2. **observes 事件路由**：`op:'observe'` --> AIPlayer::record_opponent_action(seatId, action, street, handId, meta)；若 seatId==自己则内部转 observe_self_action。`op:'showdown'` --> record_showdown(seatId, info)。
3. **beginHand 时机**：不要显式 beginHand；第一条带 handId 的 observe/decide 自动触发。独立决策的 observes 可能从上一手开始（showdown 无 handId），按输入顺序回放即可。
4. **evaluateCards**：语料 context 中不含；Rust 直接注入 `poker_core::evaluate(cards, variant)`。
5. **JSON 缺省**：context 中可选字段（positionFromButton/tablePlayerCount 等）可能缺失，serde 用默认值（`Option` 或 `#[serde(default)]`），默认值与规格 §5.2 一致（bigBlind 80 等）。
6. **observes 截断**：`observesTruncated=true` 表示只保留最近 20 条；回放时从空 brain 开始即可（语料是按同样语义生成的）。
7. **selfImage/plan 初始状态**：独立决策是“新 brain + 观测前缀”，不含该 AI 之前决策；不要试图从其他行推断。轨迹文件必须按 ops 顺序完整回放（含自己的 observe 与 decision）。
8. **时间**：语料生成时 vm 内 `performance.now()=0`；Rust 回放测试必须按样本数完成，不要用真实耗时截断。
9. **比较**：action/amount/rngCalls/samples 硬一致；数值容差见规格 §11.4。建议测试输出按 tag 分组统计，并打印 boundary 清单。

## 4. 覆盖矩阵（按 tag 细分，最终语料）

| tag | 数量 | tag | 数量 |
| --- | ---: | --- | ---: |
| standard | 180 | shortdeck | 82 |
| ten-player-25bb | 120 | general | 60 |
| preflop | 118 | flop | 57 |
| turn | 39 | river | 48 |
| facing | 139 | open | 109 |
| heads-up | 89 | three-way | 42 |
| multiway | 108 | short-stack | 22 |
| allin-opponent | 28 | preflop-3bet-plus | 12 |
| river-cfr | 21 | river-cfr-rejected | 2 |
| harvested | 224 | synthetic | 38 |
| style-TAG | 66 | style-LAG | 58 |
| style-SOLID | 37 | style-MANIAC | 24 |
| style-CALLING_STATION | 32 | style-TAGFISH | 19 |
| profile-shortdeck-local | 50 | profile-shortdeck(server) | 23 |

| 指标 | 值 |
| --- | --- |
| 决策总数 | 262（106 target + 45 general + 73 shortdeck + 38 synthetic） |
| 轨迹 | 4 条 × 6-8 决策 = 27 |
| 动作分布 | fold 85 / check 77 / raise 58 / allin 27 / call 15 |
| equity.samples 分布 | 240:89、280:3、320:62、360:12、406:3、420:63、435:1、990:29 |
| exact=true（精确河牌单挑） | 29 条 |
| CFR infoSets/nodes | 12-204 / 180-3564 |
| timedOut | 0 |
| raw 未取整数据 | 262/262 条携带 |

## 5. 建议的 poker-ai Rust 接口（task-9 冻结用）

### 5.1 Cargo.toml / 依赖

```toml
[package]
name = "poker-ai"
[dependencies]
poker-core = { workspace = true }
poker-engine = { workspace = true }   # 仅用规则辅助类型；若 engine 未就绪可先本地实现 rules 模块再切换
serde = { workspace = true, features = ["derive"] }
thiserror = { workspace = true }
[dev-dependencies]
serde_json = { workspace = true }
```

（不建议 poker-ai 依赖 poker-prob：AI 的范围权益是自有采样，不需要 prob crate；避免循环/接口耦合。）

### 5.2 文件划分（A/B/C 写作用域）

| 文件 | owner | 内容 |
| --- | --- | --- |
| src/lib.rs | A | 公共 re-export、错误类型、`Brain` trait、`pub mod` 声明 |
| src/profile.rs | A | `Style`、`AiProfile`（含全部 f64 字段）、`ProfileOverrides`、`ProfileSource`、merge 逻辑、六人格/变体表 |
| src/rng.rs | A | `AiRng`（hash_seed + mulberry32 变体 + int/normal + rng_calls 计数）、测试向量 |
| src/input.rs | A | `DecisionContext`、`PublicPlayer`、`LegalActions`、`Card` 别名、`normalized_context`、`street` 派生、`RiverRequest`/`RiverDecision`（B 的输入输出类型，冻结） |
| src/math.rs | A | position_category、starting_hand_strength、starting_range_cutoff、combo_features、board_combo_features、analyze_texture、public_action_likelihood、contestable_pot/return/pot_contest_groups/expected_additional_calls |
| src/belief.rs | A | `OpponentBelief`（observe/combo_weight/estimated_strength） |
| src/model.rs | A | `BehavioralModel`（observe/posterior/fold_rate_for/判定） |
| src/self_image.rs | A | `SelfImage` |
| src/plan.rs | A | `HandPlan` |
| src/equity.rs | A | `estimate_range_equity`（精确/MC/response samples） |
| src/preflop.rs | A | `preflop_decision`（含参与范围/防守/3bet 过滤） |
| src/postflop.rs | A | response_for_raise/preflop_response/postflop_response、realization_factor/risk_penalty/raise_targets/build_postflop_candidates/select_candidate/legalize |
| src/trace.rs | A | `DecisionTrace`、`TraceCandidate`、`RiverTrace`、`digest`（与 JS lastTrace 字段对齐） |
| src/adapter.rs | A | `AIPlayer`（decide/record_opponent_action/record_showdown/legalize 语义）+ standard brain 组装 |
| src/shortdeck.rs（或 shortdeck/） | C | 短牌 stable 决策核心 + `ShortDeckAIPlayer` 等价；实现 `Brain` trait |
| src/river/mod.rs | B | `solve_request`、`ranges`、`game`、`solve` 的公共入口（用 A 的 `RiverRequest/RiverDecision`） |
| src/river/cfr.rs | B | `CompiledCfr`（编译+迭代+policy） |
| src/river/tree.rs（可选） | B | game() 树/动作菜单/payoff |
| tests/golden_ai.rs | A | 262 决策 + 4 轨迹回放、按 tag 统计、容差、boundary 报告 |
| tests/river_cfr_golden.rs | B | 21 条 CFR + ranges/game 结构单测 + Kuhn 均衡 |
| tests/shortdeck_golden.rs | C | 82 条短牌 + 短牌 core 差异单测 |

**写冲突约束**：B 只写 `src/river/**` 与 `tests/river_cfr_golden.rs`；C 只写 `src/shortdeck*` 与 `tests/shortdeck_golden.rs`；A 写其余文件 + Cargo.toml。B/C 只读 A 冻结的 lib.rs/input.rs/profile.rs/rng.rs/trace.rs/adapter.rs。

### 5.3 关键类型草案

```rust
pub enum Action { Fold, Check, Call, Raise, AllIn }

pub struct Decision { pub action: Action, pub amount: u64 }

pub struct DecisionContext {
    pub hand_id: u64, pub decision_id: String, pub seed: String,
    pub variant: poker_core::Variant, pub street: Street,
    pub hero_seat: usize,
    pub hole_cards: [poker_core::Card; 2],
    pub community_cards: Vec<poker_core::Card>,
    pub pot: f64, pub current_bet: f64, pub to_call: f64, pub hero_round_bet: f64,
    pub hero_stack: f64, pub effective_stack: f64,
    pub min_raise_to: f64, pub max_raise_to: f64,
    pub can_raise: bool, pub can_check: bool,
    pub legal_actions: Vec<Action>,
    pub active_opponent_seats: Vec<usize>,
    pub num_opponents: usize, pub player_count: usize, pub table_player_count: usize,
    pub starting_stack: f64, pub position_from_button: i32,
    pub is_small_blind: bool, pub is_big_blind: bool,
    pub preflop_raise_count: u32,
    pub is_previous_street_aggressor: bool, pub is_delayed_cbet_candidate: bool,
    pub big_blind: f64, pub wager_unit: f64, pub time_budget_ms: f64, pub mc_samples: f64,
    pub public_players: Vec<PublicPlayer>,
}

pub trait Brain {
    fn decide(&mut self, ctx: &DecisionContext) -> DecisionWithTrace;
    fn observe_action(&mut self, event: &ObservedAction);
    fn observe_self_action(&mut self, event: &ObservedAction);
    fn observe_showdown(&mut self, seat: usize, result: &ShowdownResult);
    fn begin_hand(&mut self, hand_id: u64);
}

pub struct RiverRequest { /* pot, paid, stacks, current_bet, min_raise_to, min_bet, unit, checked_before, can_raise, board, deck, weights: Vec<(f64,f64)> ... */ }
pub struct RiverDecision { pub action: Action, pub amount: u64, pub ev: f64, pub trace: RiverTrace }
```

（以上字段与 JS `normalizedContext`/`solveRiver` 一一对应；B/C 不需要读 JS 即可实现。）

## 6. 已知 JS 怪癖 / 不一致（实现时保留或显式记录）

1. 短牌本地/服务端 profile 分叉（SD_STYLE_CONFIGS vs STYLE_CONFIGS）——建议统一，需 Lead 决策（规格 §9.3、§14）。
2. `VARIANT_PROFILE_OVERRIDES.standard` 同时作用于 variantProfiles/targetProfiles，所以“标准模式默认 riverCfrWeight=1”，但只有 target 焦点 + 门控才真正求解。
3. `preflopResponseWeight` 只在 `postflopParticipants` 路径的 responseForRaise 中生效；标准目标桌翻前用 preflopResponse（另一个函数）。
4. `AIPlayer._legalize` 与 core legalize 双重调用（幂等）；adapter 不要再次改变 solver 金额（除非规则引擎要求）。
5. `recordOpponentAction` 对 self 事件走 observeSelfAction；Rust adapter 必须同样路由，否则 publicSelfBelief/CFR 范围不同。
6. `lastRiverPublicAction` 在 observeAction（对手）与 observeSelfAction（自己）都更新；beginHand 清空。
7. `checkedBefore` 的位置推断用 `context.playerCount`（本手参与人数），不是 tablePlayerCount。
8. `estimateRangeEquity` 的 exact 河牌路径只有单挑；多人河牌仍走 MC（numOpponents>=2 用 320 样本）。
9. `riverSolver.solve` 的 `iterations` 用 `options.iterations || 800`；ai-core 固定传 800。
10. 短牌 stable `trace.parameters` 没有 strategyFocus 字段（语料 tag 用 shortdeck-general）；trace 也没有 riverCfrEnabled/riverSolver。
11. `hashSeed` 用 UTF-16 code unit（charCodeAt）；Rust 种子字符串都是 ASCII，但实现按 JS 语义。
12. `SeededRng.state` 在 JS 是普通 Number（可短暂超 u32）；Rust 用 u32 wrapping 等价（每决策都新建 RNG，不会到 2^53）。
13. `weightedPick` 的 22 次上限与 fallback 线性扫描都必须复刻；否则 blocker 场景样本分布不同。
14. 语料 `trace.candidates` 的 ev/risk/utility 已按 toFixed(2) 圆整；`raw.candidates` 才是未取整值——数值对拍优先用 raw。
15. `legalize` 在 `snapRaise` 返回 null 时会回退 call/check/fold；Rust 需要同样兜底（规则引擎接口返回 None 时）。

## 7. 交付运行记录（2026-10-23）

```bash
# 生成（两次用于确定性对比）
node oracle/golden-ai.js
node oracle/golden-ai.js --out=/tmp/ai-corpus-run2
# 回放校验
node oracle/golden-ai.js --verify
```

- 生成耗时：首次 ~83s，第二次（系统高负载）~175s；--verify ~25.5s。
- 两次运行 6 个文件 sha256 完全一致；corpusSha256=55a0bcb6...
- 10 张牌桌全部 errors=0（decisions 总数 1967 次驱动决策，最终挑选 262 条入语料）。
- `tests/golden/ai/` 总大小 2,766,941 字节（约 2.6 MiB）。

## 8. 后续任务建议（供 Lead 安排）

1. **task-9（A）**：先冻结接口（lib.rs/input.rs/profile.rs/rng.rs/trace.rs/adapter.rs + Cargo.toml），`cargo check` 通过后通知 Lead spawn B/C；然后实现 profile/rng/math/belief/model/equity/preflop/postflop/adapter + golden harness。
2. **task-10（B）**：river ranges/game/cfr/solve + RiverRequest 适配；先写结构单测（1081 组合、36 deals、菜单/payoff），再做 21 条 CFR 语料。
3. **task-11（C）**：shortdeck stable 决策核心（对照 spec §9.2 差异表逐项实现）+ 82 条短牌语料。
4. 两侧完成后由 A 集成：跑 `cargo test -p poker-ai`，输出按 tag 的通过率报告；再用 poker-cli 的确定性自对局 harness 做统计交叉验证（BB/100、非法动作=0、延迟）。
