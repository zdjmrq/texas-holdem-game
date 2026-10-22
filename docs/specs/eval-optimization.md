# 评估器与权益计算优化方案（Phase 3 后续）

状态：方案文档（2026-10-23）；当前实现不改动，只给方案与验证标准。
适用范围：`poker-core` 的 `evaluate` 与 `poker-prob` 的精确枚举 / 蒙特卡洛 / outs。
硬约束：**公共 API 不变**；现有黄金语料（`tests/golden/eval/`、`tests/golden/prob/`）必须继续逐位通过。

## 0. 结论摘要

1. 性能热点只有一个：**7 张牌评估器**。精确枚举的 90.6%、9 对手蒙特卡洛的 71.9% 指令数在
   `poker_core::eval::evaluate` 里（§2）。
2. 当前 release 单核吞吐：7 张评估约 **6.5 M 次/秒**（标准）、8.1 M 次/秒（短牌），
   距交接文档的验收目标 **>5000 万次/秒/核** 仍差约 8 倍。
3. 推荐路线：**分层查表评估器**（rank 直方图 + 花色掩码 → 完美哈希表），目标 30–60 M 次/秒/核；
   预计精确转牌从 2.8 ms 降到 0.4–0.8 ms，9 对手翻前 MC 从 4.0 ms 降到约 0.8–1.2 ms。
4. 权益侧的第二优先级：RNG 抽样改整数运算（**可保持与 JS 逐样本一致**）、tally 用数组代替
   BTreeMap、新增并行 API（并行 MC 不承诺与 JS 逐位一致，并行精确摊牌可保持逐位一致）。
5. 所有优化都必须通过"黄金语料 bit-exact 回归"才允许合入（§5）。

## 1. 当前基线（同机 release，WSL2 共享机器）

机器：Intel Core Ultra 5 125H（18 逻辑核）、16 GB、Ubuntu 24.04.5 on WSL2
（内核 6.18.33.2-microsoft-standard-WSL2）、rustc 1.99.1，release = `lto = "thin"`、`codegen-units = 1`。
详细数字与复跑命令见 `docs/benchmarks.md`。摘要：

| 基准 | 当前结果 |
| --- | --- |
| 7 张评估（标准） | 152.6 ns/次 ≈ 6.55 M 次/秒 |
| 7 张评估（短牌） | 135.2 ns/次 ≈ 7.40 M 次/秒 |
| 精确摊牌（标准，转牌 45,540 样本） | 2.82 ms |
| 精确摊牌（标准，河牌 990 样本） | 38.2 µs |
| MC 2400 样本，9 对手翻前（标准） | 3.99 ms ≈ 0.60 M 样本/秒 |
| MC 2400 样本，7 对手翻前（短牌） | 2.79 ms ≈ 0.86 M 样本/秒 |

交接文档 Phase 3 验收目标：河牌精确 P95 < 5 ms（已满足，当前 38 µs）、9 对手翻前权益
P95 < 16 ms（已满足，当前约 4 ms）、**7 张评估 > 5000 万次/秒/核（未满足，差约 8 倍）**。

## 2. Profile 方法与数据

WSL2 不暴露硬件 PMU（`perf stat -e cycles/instructions` 返回 not supported），`perf record -e cpu-clock`
采样过稀（2 s 仅约 29 样本），因此使用 **callgrind 指令级 profile + criterion 墙钟基准**：

- callgrind（valgrind 3.22）统计 Ir（执行的指令数），与机器负载无关、可复现；
- profile 对象：用 `RUSTFLAGS="-C debuginfo=1 -C lto=off"` 单独构建 `--profile bench` 的
  `poker-core`/`poker-prob` rlib（独立 `CARGO_TARGET_DIR=/tmp/target-prob-prof`，不进仓库），
  再用 `rustc -O` 链接一个固定工作量的小程序（200k 次 7 张评估 / 5 次标准转牌精确枚举 /
  20 次 9 对手 2400 样本 MC），用 `callgrind_annotate --auto=yes` 得到行级成本；
- 源码路径：`crates/poker-core/src/eval.rs`、`card.rs`、`poker-prob/src/equity.rs`；
- 数字是**指令数占比**，不是缓存/分支预测的完整画像；发布性能以 criterion 墙钟为准。

### 2.1 7 张评估（标准，200,000 次调用）

总指令 118,276,510 Ir → **591 Ir/次**；其中：

| 归属 | Ir/次 | 占比 |
| --- | ---: | ---: |
| `eval::evaluate`（不含 finish） | 551 | 93.2% |
| `eval::finish` | 16 | 2.7% |
| 调用方循环（bench main） | 22 | 3.8% |

`evaluate` 内部行级热点（占总指令比例）：

| 热点 | 占比 | 估算 |
| --- | ---: | --- |
| `card.suit()` 的整数除法 `self.0 / 13`（每张牌调用） | 12.4% | ~73 Ir/次 |
| `card.rs` 未归因行（主要是 `rank_value` 的 `% 13`） | 4.1% | ~24 Ir/次 |
| 迭代器机制（slice/iter macros、zip、range、non_null 等，`evaluate` 内联后归属） | ~20.9% | ~124 Ir/次 |
| rank 计数循环（`(2..=14).rev()`：counts 读取 + `count == 0` 分支） | 8.1% | ~48 Ir/次 |
| 花色直方图写入 `suit_values[suit][suit_lens[suit]] = value` + lens 自增 | 7.1% | ~42 Ir/次 |
| 杂项分支/数组初始化/四张、葫芦、三条、两对判断 | ~33% | ~200 Ir/次 |
| `finish` 的 score 组装 | 1.7% | ~10 Ir/次 |

关键判断：**评估器 93% 的成本在自己的直方图/分支逻辑与逐张牌 decode 上**，其中约 21% 是迭代器
抽象（`zip`、范围迭代、`(0..5).all`）带来的指令开销——查表方案把这些全部消除。

### 2.2 精确摊牌（标准转牌，5 次调用 = 227,930 次评估）

总指令 136,581,028 Ir（599 Ir/评估）：`evaluate` 90.6%、`exact_heads_up` 循环/计数 6.3%、
`finish` 2.8%、其余 ~0.3%。**精确枚举成本 ≈ 评估器成本**，优化评估器即可线性提升。

### 2.3 蒙特卡洛（9 对手翻前，20×2400 样本 = 480,000 次评估）

总指令 378,508,653 Ir（48,000 个样本 → 7,886 Ir/样本）：

| 归属 | 占比 | 说明 |
| --- | ---: | --- |
| `evaluate`（玩家 + 9 对手，480k 次）+ `finish` | 71.9% | 567 Ir/评估（evaluate 552 + finish 15，含 card.rs 未归因部分） |
| `calculate` 内联的 MC 循环 | 26.7% | 2,107 Ir/样本 |
| 其中 RNG 抽样 `index + (next_f64()*span) as usize` | 6.7% | 529 Ir/样本（23 次抽样） |
| 其中 swap 循环 + 逆序还原 | 1.2% | ~95 Ir/样本 |
| 其中 hand 缓冲区写入（player/board/opponent 两张牌） | 0.5% | ~40 Ir/样本 |
| 其中 BTreeMap tally `entry.or_insert` | 0.4% | ~31 Ir/样本 |
| 其中 opponent_scores 清理/压入/最大比较 | 0.7% | ~55 Ir/样本 |
| libc memcmp（BTreeMap key 比较） | 0.9% | 主要来自 tally 的字符串 key 比较 |

结论：评估器仍是第一优先；MC 循环的 RNG 抽样是第二热点，且可用**整数等价变换**无损优化（§4.2）。

## 3. 方案 A：分层查表评估器（推荐）

### 3.1 数据结构

1. **牌表示**：公开 `Card(u8)` 不变；内部为每张牌准备 `const` 查找表
   `CARD_SUIT[52]: u8`、`CARD_RANK[52]: u8`（或 bit trick，如 `floor(x/13) = (x*79)>>10`），
   消除 `suit()` 的除法与 `rank_value()` 的取模（profile 中合计 ~17%）。
2. **rank 直方图键**：13 个点数各 4 bit 计数 → `u64` 键（`key += 1u64 << (rank*4)`），
   7 张牌最多 4 个同点，4 bit 足够；构建成本 = 7 次查表 + 7 次移位加。
3. **花色掩码**：4 个 13-bit rank 掩码，`suit_mask[card.suit] |= 1 << rank`；7 张牌后
   `popcount >= 5` 的花色即同花；直接用掩码取 top-5 牌（或 8192 项同花查找表）。
4. **查表**：
   - 同花：花色 rank 掩码（13 bit，8192 项）→ 表项 `EvalEntry { category, kickers }`
     （覆盖同花顺/皇家同花顺/同花）。
   - 非同花：rank 直方图 → 完美哈希（例如 `hash(key) = (key * PRIME) >> (64 - BITS)`，
     碰撞用开放寻址或两级表；也可用经典 2+2 风格大表）。表项存
     `EvalEntry { category: HandCategory, kickers: [u8;5] }`。
   - 表在 `build.rs` 生成后 `include_bytes!` 打包（约 50k 项 × 4–8 B ≈ 0.2–0.4 MB，
     加同花表 <0.5 MB），或启动时用 `const fn`/一次性初始化生成（注意纯计算库不要隐藏全局可变状态，
     可用 `OnceLock` 或直接编译期常量）。
5. **评分复用**：表项只给语义类别 + 踢脚，最终 score 仍走现有
   `finish(variant, category, kickers)` 公式（`rank * 10^10 + Σ v_i * 10^(8-2i)`），
   因此 `rank_index`/`score`/`name` 与 JS 完全一致；短牌的 rank 交换继续由
   `hand_rank_index(variant, category)` 完成。
6. **<5 张**：保持现有 `evaluate_partial`（黄金语料里 2/3/4 张部分手牌语义有专门用例）。

### 3.2 表生成与验证

- 生成器：`poker-core/build.rs` 或 `tools/gen-eval-tables`（二者都需在实现阶段决定；建议 build.rs，
  避免提交二进制）。
- 表项枚举：对全部 2,598,960（标准）与 376,992（短牌）5 张组合各评估一次（用当前实现作为参考），
  归并出等价类；再把花色/直方图状态映射到表项。
- **bit-exact 验证（必须）**：
  1. `crates/poker-core/tests/js_golden_eval.rs` 的 15,052 条 JSONL 逐字段（rank/score/name）通过；
  2. 全部 5 张组合的 FNV 顺序哈希 + 牌型直方图与 `tests/golden/eval/*-five-card-hash.json` 一致；
  3. 与旧实现做差分测试：随机 6/7 张（每变体 ≥200 万手）+ 全部 5 张组合，`rank_index`、`score`、
     `name` 必须完全相等（旧实现可作为 `#[cfg(test)]` 参考路径保留一个阶段）；
  4. 全部 7,526×2 条评估语料 + prop 测试（反对称/传递性）通过；
  5. `poker-prob` 黄金语料 `tests/golden/prob/` 全部字段仍逐位相等（评估器 bit-exact 的直接推论，
     但必须实跑）。
- 表项自检：生成时断言表内每个状态都能还原出与参考评估器相同的 `(category, kickers)`。

### 3.3 预期收益（按 profile 估算）

| 优化阶段 | 预计 Ir/次 | 预计吞吐（单核 release） |
| --- | ---: | ---: |
| 现状 | ~591 | 6.5 M/s |
| A1：card decode LUT + 去掉迭代器/zip + 显式分支（不动算法） | ~380–450 | ~9–12 M/s |
| A2：5 张查表 + 21 组合取最大 | ~180–250 | ~20–30 M/s |
| A3：直方图 + 花色掩码 + 完美哈希直接 7 张查表 | ~50–90 | **30–60 M/s（目标）** |

对应权益：精确转牌 2.8 ms → 0.4–0.8 ms；9 对手 MC 4.0 ms → 0.8–1.4 ms（单核）。

## 4. 方案 B：权益侧优化

### 4.1 精确枚举并行（可保持 bit-exact）

标准转牌 45,540 个摊牌彼此独立。按河牌索引分块给 rayon：每块统计
`(wins, ties, losses, best_hand_counts)` 整数计数；合并后：

```text
heads-up 每样本 equity ∈ {0, 0.5, 1}
=> equity_total = wins + 0.5 * ties      （整数/半整数，f64 精确）
win_prob = wins / samples, tie_prob = ties / samples, lose_prob = losses / samples
```

因此**并行精确摊牌可以做到与 JS 顺序累加完全相同的 f64 结果**（半整数求和精确可表示，与顺序无关），
CPU 4 线程预计 2.8 ms → 0.8 ms。保留现有 `exact_heads_up` 顺序实现；新增
`exact_heads_up_parallel(&Prepared, threads)`（命名待定，Phase 4 定 API），用黄金语料断言结果相等。

### 4.2 蒙特卡洛抽样整数化（可保持 bit-exact，推荐先做）

JS：`swap = index + Math.floor(rng() * (pool.length - index))`，其中 `rng() = u32 / 2^32`。
对 `span = pool.length - index ≤ 52`，`floor(u32 * span / 2^32)` 与
`(u32 as u64 * span as u64) >> 32` 完全等价（乘积 < 2^38，无精度损失；u32/2^32 精确，
乘积舍入误差 < 1 ulp ≪ 1/2^32，不会跨越整数边界）。因此可改成纯整数抽样：

```rust
let draw = (u64::from(rng.next_u32()) * span as u64) >> 32;
let swap = index + draw as usize;
```

RNG 状态序列不变（仍每抽样一次 xorshift32），所以**固定 seed 的样本序列与 JS 完全一致**；
profile 显示该行占 MC 总指令 6.7%，整数化后预计省 ~2.5–3%（23 次抽样/样本）。
注意：`Deck::shuffle` 等其他调用点不动，避免影响既有黄金牌局语料。

### 4.3 MC 循环微优化（不影响 bit-exact）

- tally 的 `BTreeMap<&'static str, u64>` 改为按 `rank_index` 索引的 `[u64; 10]`，
  结束时再映射为 BTreeMap（profile：31 Ir/样本，且消除字符串 memcmp 0.9%）。
- `opponent_scores: Vec<u64>` 改为固定 `[u64; 9]` + len（当前有 clear/push 和分配器元数据）。
- 玩家/公共牌 hand 缓冲区每次样本前只重写变化的槽位（当前手写 2+missing+2*opponents 个槽位已很少，收益小）。
- 预取/循环展开对手评估（9 次连续 evaluate 可展开，减少循环开销）。
- 预计合计再省 3–6% MC 指令。

### 4.4 MC 并行（不承诺 JS 逐位一致）

- 新增并行 API：`monte_carlo_parallel(&Prepared, &MonteCarloOptions, threads)`（Phase 4 定稿）。
- 每个线程用 `hash_seed(&format!("{seed}:{chunk_index}"))` 派生独立 XorShift32 流；
  样本数按块均分。结果统计等价，但样本序列与 JS 不同。
- 若要并行且 bit-exact：xorshift32 是线性变换，可预计算 2^k 步的状态转移矩阵，把每个 chunk 的
  起始状态直接跳到 `k * draw_count * chunk_index` 步；实现成本较高，留给后续（需要时再做）。
- 生产 UI/AI 用并行；golden/AI 回放用确定性顺序 API。
- 4 线程预计 9 对手 2400 样本 4.0 ms → ~1.1–1.5 ms（叠加评估器优化后 <0.5 ms）。

### 4.5 批量评估 / SIMD

- 把多个手牌组成 SoA（rank 掩码、花色掩码数组），批量查表；对 MC 的 10 手（玩家+9 对手）一次
  构建直方图/掩码，减少重复读取公共牌。
- 7 张表评估本身很便宜（目标 50–90 Ir），批量收益有限；优先评估器与并行。
- 不引入 `unsafe`/核心 SIMD intrinsic，先看查表后是否还值得（`poker-core` 目前 `forbid(unsafe_code)`）。

## 5. 分阶段计划与验收

| 阶段 | 内容 | 验收 |
| --- | --- | --- |
| P0（已完成） | criterion 基线 + JS 同机基准 + callgrind profile | `docs/benchmarks.md` 数据可复跑 |
| P1（0.5–1 天） | card decode LUT、去迭代器、显式分支（不改算法） | 黄金语料全绿；7 张评估 ≥9 M/s |
| P2（2–3 天） | 分层查表评估器（§3），先 5 张表/21 组合，再直方图直查 | 黄金语料全绿（含穷举哈希）；≥30 M/s；精确转牌 <1 ms |
| P3（1–2 天） | RNG 整数抽样 + tally 数组化 + 精确枚举并行 | prob 黄金语料逐位相等；9 对手 MC <2 ms（单核）、精确转牌 <1 ms（4 线程） |
| P4（按需） | MC 并行 API（统计等价）、SoA 批量、Windows 性能复测 | 并行 API 文档标注不等价；AI/UI 延迟达标 |

**合入硬门槛**：`cargo test -p poker-core -p poker-prob` 全绿 + `tests/golden/eval` 与
`tests/golden/prob` 逐字段/逐位一致 + criterion 回归不低于 P0 基线（防止回退）。

## 6. 风险

1. **表构建复杂度**：7 张完美哈希需要正确枚举等价类；用"生成时逐一校验 + 差分测试"兜底，
   失败可退回 A2（5 张表 + 21 组合），仍可得 2–4 倍提升。
2. **内存/缓存**：大表（数十 MB）会拖慢随机访问；优先压缩表（≤2 MB）并实测 WSL/Windows 差异。
3. **bit-exact 陷阱**：任何 score 计算路径改动都可能破坏 `u64` 精确性；坚持"`finish()` 是唯一
   score 组装点"可规避。短牌 rank 交换只经 `hand_rank_index`。
4. **并行浮点**：MC 并行不承诺逐位一致，必须在 API 文档与 golden 测试里区分；精确摊牌用整数/半整数
   计数合并可保持逐位一致。
5. **WSL 数据不代表 Windows**：最终性能验收须在 Windows 原生构建复测（见 `docs/benchmarks.md`）。
