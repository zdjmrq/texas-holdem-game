# poker-prob 规格：权益、蒙特卡洛与 outs

状态：Phase 3 第一版实现完成（2026-10-23，task-1）。
权威 oracle：原仓库 `reference/texas-holdem-game/js/probability-core.js`（commit `e0c6c6f8`，v1.5.8）。
生成脚本：`oracle/golden-equity.js`；语料：`tests/golden/prob/`；集成测试：`crates/poker-prob/tests/js_golden_prob.rs`。

本 crate 是纯计算库（无 I/O、无 async、无全局状态），所有随机性通过 `poker-core` 的
`XorShift32Rng` 注入。牌力评估不重复实现，直接使用 `poker_core::evaluate`
（该评估器已通过全部 5 张组合穷举对齐 probability-core）。

## 1. 公开 API

| 项目 | 签名 | 对应 JS |
| --- | --- | --- |
| `prepare` | `fn prepare(player: &[Card], board: &[Card], num_opponents: usize, variant: Variant) -> Result<Prepared, ProbError>` | `prepare(options)` |
| `Prepared` | 只读访问 `variant/player/board/remaining_deck/opponents/missing_board_cards` | prepare 返回对象 |
| `exact_heads_up` | `fn exact_heads_up(&Prepared) -> Result<EquityResult, ProbError>` | `exactHeadsUp(prepared)` |
| `monte_carlo` | `fn monte_carlo(&Prepared, &MonteCarloOptions) -> EquityResult` | `monteCarlo(prepared, options)` |
| `calculate` | `fn calculate(player, board, num_opponents, variant, &MonteCarloOptions) -> Result<EquityResult, ProbError>` | `calculate(options)` |
| `find_improvements` | `fn find_improvements(player: &[Card], board: &[Card], variant: Variant) -> Improvements` | `findImprovements(playerCards, communityCards, isShortDeck)` |

返回结构（字段名对应 JS）：

```text
EquityResult {
    win_prob, tie_prob, lose_prob, equity_prob : f64,   // JS winProb/tieProb/loseProb/equityProb
    samples : u64,                                      // JS samples
    method : Method { ExactHeadsUp, MonteCarlo },       // as_str() => "exact-heads-up"/"monte-carlo"
    elapsed_ms : f64,                                   // JS elapsedMs（仅诊断，非确定性契约）
    best_hand_counts : BTreeMap<String, u64>,           // JS stats.bestHandCounts
    outs : Option<Improvements>,                        // calculate 挂载；直接调用 exact/mc 为 None
}
```

`Improvements` 对应 JS outs 对象：`total_outs/strong_outs/thin_outs/by_hand_type/note`；
`ImprovementGroup { cards, count, strong_count, thin_count }`；
`ImprovementCard { rank: "2".."10"/"J"/"Q"/"K"/"A", suit: "spades"|"hearts"|"diamonds"|"clubs", value: 2..=14, quality: Strong|Thin }`，
`quality.as_str()` 为 `"strong"/"thin"`，与 JS 字符串完全一致。

错误 `ProbError` 对应 JS 的 `throw new Error` 分支：
`WrongHoleCardCount`（"Texas Hold'em needs two hole cards..."）、
`TooManyCommunityCards`、`DuplicateKnownCard`（"Duplicate known card"）、
`TooManyOpponents`（"Too many opponents for remaining cards"），
以及 `ExactRequiresTurnOrRiver`（JS 只在 4/5 张公共牌时调用 exact，没有该守卫；Rust 显式返回错误）。

## 2. 与 JS 的语义对齐

### 2.1 prepare

- 手牌必须恰好 2 张，公共牌 0..=5 张，全部已知牌不得重复（bitmask 计数，等价 JS `Set`）。
- 对手数：`num_opponents.clamp(1, isShortDeck ? 7 : 9)`，与 JS
  `Math.max(1, Math.min(..., Math.floor(Number(x) || 1)))` 等价（Rust 参数为整数 `usize`）。
- 剩余牌堆 = 变体标准牌堆（花色外层、点数升序）过滤已知牌，顺序与 JS `createDeck` 完全相同。
- 剩余牌数必须 ≥ `(5 - board.len()) + opponents * 2`。

### 2.2 exactHeadsUp（精确枚举）

- 仅当 `opponents == 1 && board.len() >= 4` 时由 `calculate` 选用。
- 4 张公共牌（转牌）：外层按剩余牌堆顺序枚举每张河牌，内层枚举其余牌的对子；
  样本数 `n * C(n-1, 2)`（标准 n=46 → 45,540；短牌 n=30 → 12,180）。
- 5 张公共牌（河牌）：只枚举对手的两张牌，样本数 `C(n,2)`（标准 990；短牌 406）。
- 枚举顺序、玩家手牌先评估、对手后评估的顺序与 JS 相同；平局 equity = `1 / (tied + 1)`，
  逐样本按相同顺序累加，因此 `equityProb` 与 JS 逐位相等。
- `bestHandCounts` 与 JS 相同：每个河牌按 `max(1, 该河牌下的对手对子数)` 计入玩家最终牌型名。

### 2.3 monteCarlo（固定 seed 逐样本一致）

JS 采样算法原样复刻：

1. `hashSeed(seed)`：对 UTF-16 code unit 做 FNV-1a（`2166136261`、`16777619`），
   结果 0 时回退 `0x9e3779b9`；空 seed 回退字符串 `"poker-probability"`。
   Rust 使用 `poker_core::XorShift32Rng::from_seed(&str)`（`hash_seed` 已与 JS 位级一致）。
2. 每样本做部分 Fisher-Yates：`swap = index + floor(rng() * (pool_len - index))`，
   `index = 0..draw_count`，`draw_count = missing + opponents*2`；样本结束后**逆序**交换还原。
3. 先评估玩家（2 张手牌 + 5 张公共牌），再依次评估每个对手（两张 pool 牌 + 同一公共牌）。
4. `resolveShowdown`：玩家大于所有对手 → 胜；小于最大对手 → 负；等于最大 → 平，
   `equity += 1/(tied+1)`；胜/负/平均按样本顺序累加。
- 手牌评估顺序不影响结果（评估器基于集合），但 RNG 调用次数与交换顺序完全一致，
  所以固定 seed 下样本序列、最终计数、`best_hand_counts`、概率值均与 JS **完全相等**（非统计近似）。

### 2.4 findImprovements（outs）

- 仅分析 3 或 4 张公共牌；其他长度返回 JS 早期值：
  `total_outs/strong_outs/thin_outs = 0`、`by_hand_type = {}`、note=`"翻牌后才显示牌型改善牌"`。
- 对剩余牌堆每张牌：评估 `player + board + card`；
  - `after.score <= before.score` 跳过；
  - 同 rank（牌型等级）且（`before.rank == 0` 或 `after.score - before.score < 1e6`）跳过；
    即高牌不提示、同类提升只保留"至少提升一个踢脚档位"的牌；
  - `after.rank > before.rank` 为 strong，否则 thin。
- 输出牌面使用 JS `decode` 的 `{rank, suit, value}`（rank 是 `"10"` 而不是 `"T"`），
  卡片顺序 = 剩余牌堆顺序（花色外层、点数升序），与 JS 完全一致。
- note 固定为 `"强提升会提高牌型等级；踢脚改善只增强同一牌型，均不代表必胜"`。

### 2.5 calculate

- `prepare` 校验 → `opponents == 1 && board.len() >= 4` 走 exact，否则走 Monte Carlo → 追加 outs。
- 与 JS 一致：`calculate` 的结果始终带 outs（即使 preflop/river 是空 outs）。

## 3. 与 JS 的有意差异

1. **时间预算 → 显式样本数**：JS `monteCarlo` 有 `timeBudgetMs`（40..2000ms）提前退出条件：
   `samples >= minSamples && samples % 32 == 0 && elapsed >= budget`。Rust 不读时钟决定样本数，
   总是执行 `MonteCarloOptions::target_samples()` 次采样，因此结果完全可复现。
   `MonteCarloOptions::min_samples` 保留以对齐 JS 参数语义并做同样的 clamp
   （`min(target, max(350, minSamples || 700))`），但确定性模式不会触发提前退出。
   黄金语料生成时 JS 侧传 `minSamples = 目标样本数`，因此 JS 也不会提前退出，两边样本数严格相等。
2. **时间单位**：JS exact 用 `Date.now()`、monteCarlo 用 `performance.now()`；Rust 统一用
   `Instant` 得到 f64 毫秒。`elapsed_ms` 是诊断字段，不参与任何确定性比对，也**不进入**
   `EquityResult` 的 `PartialEq`。
3. **`num_opponents` 类型**：Rust 用 `usize` 整数参数；JS 接受浮点并 `Math.floor`。调用方传整数即可，
   clamp 上限/下限逻辑一致。
4. **`find_improvements` 参数**：Rust 接受 `&[Card]`（与 JS 相同不校验手牌张数），但总牌数 > 7 时
   `poker_core::evaluate` 的契约会 panic；正常调用应传 2 张手牌。`calculate` 路径已由 prepare 校验。
5. **exact 错误**：`exact_heads_up` 对 board < 4 返回 `ProbError::ExactRequiresTurnOrRiver`，
   而不是产生 JS 中不可能出现的部分手牌枚举。

除此之外无数值差异：所有概率字段为同一 IEEE-754 运算序列的结果。

## 4. 黄金语料

生成：`node oracle/golden-equity.js`（读取 `REFERENCE_REPO`，默认 `../reference/texas-holdem-game`）。
输出目录 `tests/golden/prob/`：

| 文件 | 行数 | 内容 | sha256 |
| --- | ---: | --- | --- |
| `exact-heads-up.jsonl` | 6 | 精确枚举（标准+短牌，转牌/河牌；含皇家同花顺公共牌全平局） | 6845 B |
| `monte-carlo.jsonl` | 17 | 固定 seed MC（1/2/3/7/9 对手、0 对手 clamp、低/高样本 clamp、空/Unicode seed、标准+短牌） | 17405 B |
| `improvements.jsonl` | 14 | findImprovements（preflop/river 隐藏、翻牌/转牌、strong/thin、标准+短牌） | 18992 B |
| `manifest.json` | — | 生成器、oracle commit、行数、样本总数、每个文件 sha256 | — |

- 精确枚举总样本 105,646；固定 seed MC 总样本 19,800。每个文件的完整 sha256 与字节数记录在
  `manifest.json`（也可直接 `sha256sum tests/golden/prob/*.jsonl`）。
- 语料只包含确定性字段：JS 返回的 `elapsedMs` 被刻意剔除，因此脚本重复运行输出**逐字节相同**
  （已用两次运行 `diff -r` 验证），满足 `docs/golden-corpus.md` 的可复现要求。
- MC 每一行在脚本内执行两次并断言序列化结果完全一致；exact 行断言 method 与闭式样本数。
- JSONL 行格式示例：

```json
{"id":"standard-turn-flush-draw","variant":"standard","player":["Ah","Kh"],"board":["Qh","Jh","2c","7d"],"numOpponents":1,"result":{"winProb":0.6333772507685551,"tieProb":0.00691699604743083,"loseProb":0.35970575318401404,"equityProb":0.6368357487922706,"samples":45540,"method":"exact-heads-up","stats":{"bestHandCounts":{"一对":15840,"高牌":17820,"顺子":2970,"同花":7920,"皇家同花顺":990}},"outs":{"totalOuts":28,"strongOuts":28,"thinOuts":0,...}}}
```

（实际文件为单行 JSON，这里为可读性截断。）

## 5. 逐字段比对规则（Rust 集成测试）

`crates/poker-prob/tests/js_golden_prob.rs` 读取三个 JSONL，逐行构造 `Card`/`Variant` 后调用 Rust API：

| 字段 | 精确枚举 | 固定 seed MC | improvements |
| --- | --- | --- | --- |
| win/tie/lose/equityProb | **完全相等**（`f64 ==`） | **完全相等** | — |
| samples | 完全相等（且等于闭式值） | 完全相等（且等于 clamp 后目标） | — |
| method | `"exact-heads-up"` | `"monte-carlo"` | — |
| stats.bestHandCounts | 完全相等（map 相等） | 完全相等 | — |
| outs 全部字段 | 完全相等（含 cards 顺序/quality） | 完全相等 | 完全相等 |
| elapsedMs | 不比较（非确定性） | 不比较 | — |

同时测试会对 exact/MC 语料额外调用 `exact_heads_up` / `monte_carlo` 直接 API，并与 `calculate` 的
结果比较（除 outs 外完全相等），确保两条调用路径都对齐 JS。

> 注意：解析语料时 `serde_json` 必须开启 `float_roundtrip` feature（已在
> `crates/poker-prob/Cargo.toml` 的 dev-dependencies 中启用），否则默认浮点解析可能差 1 ulp，
> 导致"完全相等"比对误报。

## 6. 验证命令（当前结果）

```bash
export CARGO_TARGET_DIR=/tmp/target-prob-dev   # 团队约定，避免 cargo 锁竞争
cd /home/dsh/projects/texas-holdem-rs

cargo fmt -p poker-prob --check
cargo clippy -p poker-prob --all-targets -- -D warnings
cargo test -p poker-prob
```

结果（2026-10-23）：fmt 通过；clippy 0 warning；`cargo test -p poker-prob` 共 **22 项通过**
（19 单元测试 + 3 黄金语料集成测试，另有 0 doctest），debug 模式总耗时约 1.2s，
其中黄金语料 0.19s。

## 7. 后续项与风险

- **性能**：当前实现为正确性优先，未做查表/批量/并行优化；评估器本身是 poker-core 的逐手实现。
  后续 Phase 3 收尾可考虑：样本批量评估、`rayon` 并行、7 张牌查表、按种子分片后合并计数。
  （crate 依赖中已预留 `rayon`，但本版未使用。）
- **范围加权 / 条件摊牌**：JS probability-core 本版没有范围加权；poker-prob 后续如果被 AI/UI 需要，
  应新增独立 API，不要改动本文件已对齐的纯语义。
- **UI 集成**：分析面板需要 f64 概率 + outs 列表；`elapsed_ms` 可用于调试面板，不要用于确定性断言。
- **风险**：JS 的 timeBudget 分支在真实游戏里可能截断样本；Rust 用固定样本数替代，行为差异只影响
  UI 的"快速近似"体验，不影响确定性测试。若未来需要复刻时间预算，可在 `MonteCarloOptions` 增加
  `Option<Duration>` 字段，并保持默认 `None`。
- **JSON 浮点解析**：任何后续新增读取本语料的测试 crate 都必须启用 `serde_json/float_roundtrip`。
