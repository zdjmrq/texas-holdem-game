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
| poker-prob 权益/outs | `oracle/golden-equity.js` | `tests/golden/prob/` | 待办 |
| poker-ai 决策轨迹（固定 seed） | `oracle/golden-ai.js` | `tests/golden/ai/` | 待办 |
| poker-protocol 消息 | `oracle/golden-protocol.js` | `tests/golden/protocol/` | 待办 |

## 3. 复现约定

- 原仓库固定 commit `e0c6c6f88d3e64324e1d83bfc700dd3a77f71b90`，位于 `reference/texas-holdem-game`。
- 任何语料生成脚本都必须：固定 seed（`oracle/lib/prng.js` 的 mulberry32 / xorshift32）、不依赖当前时间、不依赖 `Math.random`。
- 语料文件提交到 git；超大样本只保留生成脚本和汇总哈希。
