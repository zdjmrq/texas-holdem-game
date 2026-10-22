# Oracle 脚本（Node，只读参考原 JS 实现）

用途：用可注入的 seeded PRNG patch Math.random，导出固定种子的牌局/牌力/合法动作/权益/AI 决策到 `tests/golden/*.jsonl`，并作为 Rust 实现的对比基准。

原则见 `docs/RUST_REWRITE_HANDOFF.md` §5.3：
1. 先 patch `Math.random`（mulberry32），再 require 原模块。
2. 固定 seed / 座位数 / 筹码 / 盲注 / 人格 / 行动脚本。
3. 输出 JSONL，一行一个决策或牌局。

待实现（Phase 0）：
- [ ] `golden-eval.js` 牌力评估（标准 + 短牌）
- [ ] `golden-rules.js` 合法动作 / 加注网格 / 边池 / 退款
- [x] `golden-equity.js` 精确权益 + 固定 seed 蒙特卡洛（输出 `tests/golden/prob/`，见 `docs/specs/prob.md`）
- [ ] `golden-ai.js` 固定 seed AI 决策轨迹
- [ ] `golden-game.js` 整手牌局脚本

性能基准（Phase 3）：
- [x] `bench-js.js` 原 JS 引擎同机基准（`evaluate` / `calculate`，JSON 输出，供 `docs/benchmarks.md` 与 Rust criterion 对比）
