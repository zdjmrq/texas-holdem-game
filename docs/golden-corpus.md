# 黄金语料（Golden Corpus）

黄金语料的唯一用途：证明 Rust 重写在**规则、牌力、合法动作、底池、退款、结算、权益、AI 决策**上与 JS v1.5.8 等价。
原则：oracle 永远是原仓库（只读）中的 JS 实现；语料由 `oracle/*.js` 脚本可重复生成。

## 0. 权威 oracle 的确定（2026-10-23）

原项目有 4 套牌力评估器。`oracle/compare-evaluators.js` 做了穷举对比：

| 对比 | 范围 | 结果 |
| --- | --- | --- |
| `probability-core.evaluate` vs `server/poker-rules.evaluateHand` vs `js/cards.js evaluateHand`（标准） | **全部 2,598,960 个 5 张组合** | 0 差异 |
| `probability-core.evaluate` vs `server/poker-rules.evaluateHand` vs `shortdeck.js evaluateSDHand`（短牌） | **全部 376,992 个 5 张组合** | 0 差异 |
| 上述三个评估器，随机 6/7 张（各 20,000 手/变体） | 采样 | 0 差异 |
| 2 张手牌部分评估（legacy `cards.js` vs `probability-core`） | 采样 | **有意差异**：`cards.js`/`shortdeck.js` 对同花 2 张有 +10,000 的展示加成，`probability-core` 没有 |

结论：**`js/probability-core.js` 是唯一权威评估器**（也用于 AI、服务端结算、概率面板）。完整报告见
`tests/golden/eval/evaluator-equivalence.json`。Rust `poker-core::evaluate` 对齐 probability-core 语义，
包括 <5 张的部分评估规则。

## 1. 评估器语料

生成：`node oracle/golden-eval.js`（读取环境变量 `REFERENCE_REPO`，默认 `../reference/texas-holdem-game`）

| 文件 | 内容 |
| --- | --- |
| `tests/golden/eval/standard-cases.jsonl` | 7,526 条：全部牌型边界 + 随机 5/6/7 张 + 2/3/4 张部分手牌，标准牌 |
| `tests/golden/eval/shortdeck-cases.jsonl` | 7,526 条，短牌（36 张） |
| `tests/golden/eval/*-five-card-hash.json` | 对**全部** 5 张组合的顺序敏感哈希 + 牌型直方图，用于穷举校验 |

JSONL 行格式：

```json
{"cards":["As","Ks","Qs","Js","Ts"],"rank":9,"score":90000000000,"name":"皇家同花顺"}
```

- `cards`：紧凑牌面，rank token + suit code（`2..9,T,J,Q,K,A` + `s,h,d,c`）。
- `rank`：变体内部牌型序号（短牌同花/葫芦互换）。
- `score`：JS 评分 `rank * 10^10 + Σ v_i * 10^(8-2i)`（整数，f64/u64 精确）。
- `name`：JS 中文牌型名。

穷举哈希实现（JS 与 Rust 必须一致）：

```text
h0 = 14695981039346656037
for each 5-card combination in deck order (suit-major, rank ascending):
    h = (h XOR rank_index) * 1099511628211  (mod 2^64)
    h = (h XOR score)      * 1099511628211  (mod 2^64)
```

对应测试：`crates/poker-core/tests/js_golden_eval.rs`（`cargo test -p poker-core`）。

## 2. 后续语料（按阶段补齐）

| 阶段 | 脚本 | 输出 | 状态 |
| --- | --- | --- | --- |
| poker-engine 规则/下注/底池/退款 | `oracle/golden-rules.js` | `tests/golden/rules/` | 进行中 |
| poker-prob 权益/outs | `oracle/golden-equity.js` | `tests/golden/prob/` | 完成（见 §2.1） |
| poker-ai 决策轨迹（固定 seed） | `oracle/golden-ai.js` | `tests/golden/ai/` | 待办 |
| poker-protocol 消息 | `oracle/golden-protocol.js` | `tests/golden/protocol/` | 待办 |

### 2.1 poker-prob 权益/outs 语料（完成，2026-10-23）

生成：`node oracle/golden-equity.js`（输出目录 `tests/golden/prob/`）。语料全部来自
`js/probability-core.js` 的 `calculate` / `findImprovements`，覆盖标准牌与短牌。

| 文件 | 行数 | 内容 |
| --- | ---: | --- |
| `tests/golden/prob/exact-heads-up.jsonl` | 6 | 精确枚举：标准/短牌 × 转牌/河牌；含公共牌皇家同花顺全平局 |
| `tests/golden/prob/monte-carlo.jsonl` | 17 | 固定 seed MC：1/2/3/7/9 对手、0 对手 clamp、低/高样本 clamp、空/Unicode seed |
| `tests/golden/prob/improvements.jsonl` | 14 | `findImprovements`：preflop/river 隐藏、翻牌/转牌、strong/thin、标准/短牌 |
| `tests/golden/prob/manifest.json` | — | 生成器、oracle commit、行数、样本总数、每个文件 sha256 |

- 总样本：精确枚举 **105,646** 个 showdown；固定 seed Monte Carlo **19,800** 个样本。
- 行格式（每个 JSONL 行一部份字段，完整结构见 `docs/specs/prob.md`）：

  ```json
  {"id":"standard-turn-flush-draw","variant":"standard","player":["Ah","Kh"],"board":["Qh","Jh","2c","7d"],"numOpponents":1,"result":{"winProb":0.6333772507685551,"samples":45540,"method":"exact-heads-up","stats":{"bestHandCounts":{...}},"outs":{...}}}
  ```

- **确定性**：语料不包含 JS 的 `elapsedMs`（墙钟字段），因此脚本重复运行输出逐字节一致
  （已两次 `diff -r` 验证）；MC 每行在脚本内执行两次并断言序列化结果相同；exact 行断言
  method 与闭式样本数（标准转牌 45,540 / 河牌 990；短牌转牌 12,180 / 河牌 406）。
- **复现**：`node oracle/golden-equity.js && sha256sum tests/golden/prob/*.jsonl`，
  期望哈希见 `tests/golden/prob/manifest.json`。
- **测试**：`crates/poker-prob/tests/js_golden_prob.rs`（`cargo test -p poker-prob`），
  逐字段完全相等（精确枚举逐位相等；固定 seed MC 逐样本一致）。
  读取语料的测试必须启用 `serde_json` 的 `float_roundtrip` feature，否则浮点解析可能差 1 ulp。

## 3. 复现约定

- 原仓库固定 commit `e0c6c6f88d3e64324e1d83bfc700dd3a77f71b90`，位于 `reference/texas-holdem-game`。
- 任何语料生成脚本都必须：固定 seed（`oracle/lib/prng.js` 的 mulberry32 / xorshift32）、不依赖当前时间、不依赖 `Math.random`。
- 语料文件提交到 git；超大样本只保留生成脚本和汇总哈希。
