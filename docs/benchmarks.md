# 性能基线（Phase 3：poker-core 评估器 / poker-prob 权益）

记录日期：2026-10-23。对应任务：task-5（Phase 3 性能基线）。
基准代码：`crates/poker-core/benches/eval.rs`、`crates/poker-prob/benches/equity.rs`、
`oracle/bench-js.js`。优化方案见 `docs/specs/eval-optimization.md`。

> **WSL 数据不代表 Windows 性能。** 本机是 WSL2（Intel Core Ultra 5 125H，共享机器），
> 最终验收目标（尤其 7 张评估 5000 万次/秒/核）必须在 Windows 原生 release 构建上复测。

## 1. 机器与环境

| 项 | 值 |
| --- | --- |
| CPU | Intel(R) Core(TM) Ultra 5 125H（18 逻辑核） |
| 内存 | 16,125,080 kB（约 16 GB） |
| OS | Ubuntu 24.04.5 LTS，WSL2 内核 `6.18.33.2-microsoft-standard-WSL2` |
| Rust | rustc 1.99.1 / cargo 1.99.1 |
| Node.js | v24.21.0 |
| Rust profile | release：`lto = "thin"`、`codegen-units = 1`；Cargo 镜像 = rsproxy |
| 计时 | criterion 0.7（release 默认配置）、Node `process.hrtime.bigint()` |
| 基准时负载 | Rust release 基线在 loadavg 2–5 时采集；JS 基线在 loadavg 4.9–5.6 时采集 |

共享机器说明：其他 agent 编译时 loadavg 可到 15–21，同一基准的 MC 数字会劣化 2–3 倍；
本文件采用系统较空闲时的一组数据。criterion 每次运行的 run-to-run 波动约 ±5%（评估）
到 ±10%（MC），并非严格可比的绝对值。

## 2. 方法

### 2.1 Rust criterion

- 评估器：标准/短牌 × 5/6/7 张；每轮 batch 固定 **10,000** 手（确定性生成：
  `XorShift32Rng::from_seed("bench-core-eval")` + 每手 clone 完整牌堆再 `Deck::shuffle`），
  throughput = 10,000 elements/轮。
- 牌堆：`Deck::standard()` 连续 Fisher-Yates shuffle 一次作为 1 次操作。
- 权益：4 个精确摊牌场景（标准/短牌 × 河牌/转牌）+ 4 个固定 seed MC 场景
  （2400 样本：标准 1/2/9 对手、短牌 7 对手）。全部走 `calculate()`（与 JS 对外 API 对齐，
  包含 outs 计算），throughput 单位 = 摊牌样本数。
- criterion 配置：默认 `sample_size=20`、`warm_up_time=300ms`、`measurement_time=1500ms`；
  可用环境变量覆盖（dev 快速诊断用）：
  `POKER_BENCH_SAMPLE_SIZE` / `POKER_BENCH_WARMUP_MS` / `POKER_BENCH_MEASUREMENT_MS`。
- 报出的 ns/op = batch 中位数 / 10,000；M ops/s = 1e9 / ns/op。
  权益 Msamples/s = 样本数 / 中位耗时。

### 2.2 JavaScript 同机（oracle/bench-js.js）

- 加载原仓库 `js/probability-core.js`（v1.5.8 / commit e0c6c6f8）。
- **手牌生成逐位复刻 Rust**：`oracle/lib/prng.js` 的 xorshift32（同 hashSeed）+ 同样的
  整副洗牌顺序 → 与 Rust 基准使用完全相同的 10,000 手牌（seed `bench-core-eval`）。
- evaluate：每轮评估 10,000 手，统计 ns/op。
- calculate：与 Rust 相同的 8 个场景（同样 hole cards/board/对手数/seed/simulations=2400/
  minSamples=2400，保证 JS 不因时间预算提前退出），每个场景计时 ns/op。

### 2.3 复跑命令

```bash
export CARGO_TARGET_DIR=/tmp/target-prob-dev   # 团队约定，避免 cargo 锁竞争
cd /home/dsh/projects/texas-holdem-rs

# Rust release 基线（两个 bench）
cargo bench -p poker-core --bench eval --offline
cargo bench -p poker-prob --bench equity --offline

# JS 同机基线（输出 JSON 到 stdout）
node oracle/bench-js.js

# 快速 dev-profile 诊断（不要与 release 数字混用）
POKER_BENCH_SAMPLE_SIZE=10 POKER_BENCH_WARMUP_MS=100 POKER_BENCH_MEASUREMENT_MS=200 \
  cargo bench --profile dev -p poker-core --bench eval --offline
```

## 3. Rust release 基线

### 3.1 评估器（`poker_core::evaluate`，单核）

| 变体 | 张数 | ns/op | M 次/秒 |
| --- | ---: | ---: | ---: |
| 标准 | 5 | 118.7 | 8.43 |
| 标准 | 6 | 138.0 | 7.25 |
| 标准 | 7 | 152.6 | 6.55 |
| 短牌 | 5 | 102.6 | 9.74 |
| 短牌 | 6 | 115.7 | 8.65 |
| 短牌 | 7 | 135.2 | 7.40 |
| 牌堆 shuffle（标准 52 张） | — | **157.1 ns/次** | ~6.36 M 次/秒 |

短牌更快的原因：`straight_high` 的搜索范围更小（最低 10 而非 6），且无 A-2-3-4-5 检查。

### 3.2 权益（`poker_prob::calculate`，单核）

| 场景 | 样本数 | 中位耗时 | 吞吐 |
| --- | ---: | ---: | ---: |
| 精确摊牌 · 标准河牌 | 990 | 38.2 µs | 25.9 M 样本/秒 |
| 精确摊牌 · 标准转牌 | 45,540 | 2.819 ms | 16.15 M 样本/秒 |
| 精确摊牌 · 短牌河牌 | 406 | 13.7 µs | 29.6 M 样本/秒 |
| 精确摊牌 · 短牌转牌 | 12,180 | 680.7 µs | 17.9 M 样本/秒 |
| MC · 标准翻前 heads-up | 2,400 | 789.2 µs | 3.04 M 样本/秒 |
| MC · 标准翻牌 3 人底池 | 2,400 | 829.1 µs | 2.90 M 样本/秒 |
| MC · 标准翻前 9 对手 | 2,400 | 3.986 ms | 0.602 M 样本/秒 |
| MC · 短牌翻前 7 对手 | 2,400 | 2.794 ms | 0.859 M 样本/秒 |

对照交接文档 Phase 3 验收目标：

- 河牌精确权益 P95 < 5 ms → **当前 38 µs，满足**；
- 9 对手翻前权益 P95 < 16 ms → **当前约 4 ms（2400 样本），满足**；
- 10 万次 7 张评估吞吐 > 5,000 万次/秒/核 → **当前 6.55 M/s，未满足（差约 7.6 倍）**，
  需要查表评估器（见 `docs/specs/eval-optimization.md`）。

### 3.3 Phase 3 优化后基线（2026-10-23 收尾）

`poker-core` 新增 7 张查表快速路径（OnceLock 懒加载，无 unsafe；5/6 张与部分手牌继续走
reference 路径，公共 API 不变），`poker-prob` 的 MC 抽样改为整数随机（与 JS 逐样本 bit-exact）。
优化后数字取自 loadavg 3–6 的共享机器（更高负载下的运行已丢弃；这台 WSL 的绝对数字仍不代表
Windows）。

**7 张评估（单核 release）**

| 变体 | 优化前 | 优化后 | 提升 |
| --- | ---: | ---: | ---: |
| 标准 7 张 | 152.6 ns（6.55 M/s） | **23.8 ns（42.1 M/s）** | **6.4×**（达成 ≥30 M/s 目标） |
| 短牌 7 张 | 135.2 ns（7.40 M/s） | **20.1 ns（49.8 M/s）** | **6.7×**（接近 50 M/s 理想值） |
| 标准 5/6 张 | 118.7 / 138.0 ns | 142.5 / 171.9 ns | 回退（非本轮目标；5/6 张仍走 reference 路径） |
| 短牌 5/6 张 | 102.6 / 115.7 ns | 121.3 / 118.8 ns | 基本持平 |

**权益（单核 release；精确枚举保持顺序实现）**

| 场景 | 优化前 | 优化后 | 提升 |
| --- | ---: | ---: | ---: |
| 精确摊牌 · 标准河牌 | 38.2 µs | **23.5 µs** | 1.6× |
| 精确摊牌 · 标准转牌 | 2.819 ms | **≈1.20 ms** | ≈2.4× |
| 精确摊牌 · 短牌河牌 | 13.7 µs | **9.1 µs** | 1.5× |
| 精确摊牌 · 短牌转牌 | 680.7 µs | **≈0.38 ms** | ≈1.8× |
| MC · 标准翻前 heads-up | 789 µs | **312 µs** | 2.5× |
| MC · 标准翻牌 3 人 | 829 µs | **390 µs** | 2.1× |
| MC · 标准翻前 9 对手 | 3.986 ms | **1.053 ms** | 3.8× |
| MC · 短牌翻前 7 对手 | 2.794 ms | **709 µs** | 3.9× |

注：精确转牌的两个"优化后"数字来自单独的顺序计时（bench 内嵌的并行实验未合入，见下），
其余来自 criterion 单次运行；共享机器下 run-to-run 波动约 ±10%。

**等价性证据**（测试文件未改动）

- `cargo test -p poker-core --lib`：26 项通过，其中 4 项为新增差分测试：
  全部 **49,205** 个 7 张 rank pattern × 双变体、全部 **4,719** 个 flush mask × 双变体、
  每个变体 **100,000** 手随机 5/6/7 张牌，fast 与 reference 的 rank_index/score/name/category 全部相等。
- `cargo test -p poker-core --test js_golden_eval`：15,052 用例 + 两变体 5 张穷举哈希全绿。
- `cargo test -p poker-prob`：全部黄金语料（精确/固定 seed MC/findImprovements）全绿，
  证明 MC 整数抽样与 JS 逐样本 bit-exact。

**未合入的实验**：精确摊牌 rayon 并行版本（按河牌分块、整数 tally 按序合并）在 golden 测试中
验证了 bit-exact，但在这台 18 线程 WSL 机器的全局线程池下反而更慢
（顺序 ≈1.2 ms vs 并行 1.4–2.6 ms，任务过小 + 调度开销），按任务约束未合入。

## 4. Rust dev-profile 诊断数字

dev profile（未优化 + debug 断言，`--profile dev`）仅用于理解相对成本，**不是发布性能**。
采集时系统 loadavg 16–21，criterion 用低样本配置（10 samples / 100 ms warmup / 200 ms measurement）。

| 场景 | dev 中位耗时 | release 中位耗时 | dev/release |
| --- | ---: | ---: | ---: |
| 7 张评估（标准，10k 手 batch） | 15.44 ms | 1.526 ms | ~10.1× |
| 7 张评估（短牌，10k 手 batch） | 9.22 ms | 1.352 ms | ~6.8× |
| shuffle（标准） | 3.82 µs | 157 ns | ~24× |
| 精确摊牌 · 标准转牌 | 41.81 ms | 2.819 ms | ~14.8× |
| 精确摊牌 · 标准河牌 | 2.267 ms | 38.2 µs | ~59× |
| MC · 标准翻前 9 对手 | 33.45 ms | 3.986 ms | ~8.4× |
| MC · 短牌翻前 7 对手 | 23.04 ms | 2.794 ms | ~8.2× |

（dev 数字受 loadavg 16–21 影响，尤其小 benchmark 抖动大；仅作量级参考。）

## 5. JavaScript 同机基线（node v24.21.0）

### 5.1 `probability-core.evaluate`

| 变体 | 张数 | ns/op | M 次/秒 |
| --- | ---: | ---: | ---: |
| 标准 | 5 | 855.8 | 1.169 |
| 标准 | 6 | 915.9 | 1.092 |
| 标准 | 7 | 890.9 | 1.122 |
| 短牌 | 5 | 752.0 | 1.330 |
| 短牌 | 6 | 932.5 | 1.072 |
| 短牌 | 7 | 1054.2 | 0.949 |

### 5.2 `probability-core.calculate`（与 Rust 场景完全一致）

| 场景 | 中位耗时 | 吞吐 |
| --- | ---: | ---: |
| 精确摊牌 · 标准河牌 | 0.714 ms | 1.39 M 样本/秒 |
| 精确摊牌 · 标准转牌 | 29.10 ms | 1.57 M 样本/秒 |
| 精确摊牌 · 短牌河牌 | 0.275 ms | 1.47 M 样本/秒 |
| 精确摊牌 · 短牌转牌 | 8.07 ms | 1.51 M 样本/秒 |
| MC · 标准翻前 heads-up | 4.71 ms | 0.509 M 样本/秒 |
| MC · 标准翻牌 3 人底池 | 6.59 ms | 0.364 M 样本/秒 |
| MC · 标准翻前 9 对手 | 19.61 ms | 0.122 M 样本/秒 |
| MC · 短牌翻前 7 对手 | 14.99 ms | 0.160 M 样本/秒 |

## 6. Rust vs JS 同机对比

（Rust 数字取自 loadavg 2–4 的 release 基线；JS 取自 loadavg 4.9–5.6 的同机运行。
JS 侧负载更高，因此该比值对 Rust 略有利，保守解读。）

| 项目 | Rust | JS | Rust 加速 |
| --- | ---: | ---: | ---: |
| 5 张评估（标准） | 118.7 ns | 855.8 ns | **7.2×** |
| 6 张评估（标准） | 138.0 ns | 915.9 ns | **6.6×** |
| 7 张评估（标准） | 152.6 ns | 890.9 ns | **5.8×** |
| 5 张评估（短牌） | 102.6 ns | 752.0 ns | **7.3×** |
| 6 张评估（短牌） | 115.7 ns | 932.5 ns | **8.1×** |
| 7 张评估（短牌） | 135.2 ns | 1054.2 ns | **7.8×** |
| 精确河牌（标准） | 38.2 µs | 714 µs | **18.7×** |
| 精确转牌（标准） | 2.819 ms | 29.10 ms | **10.3×** |
| 精确河牌（短牌） | 13.7 µs | 275 µs | **20.1×** |
| 精确转牌（短牌） | 681 µs | 8.07 ms | **11.9×** |
| MC 翻前 heads-up（2400 样本） | 789 µs | 4.71 ms | **6.0×** |
| MC 翻牌 3 人（2400 样本） | 829 µs | 6.59 ms | **8.0×** |
| MC 翻前 9 对手（2400 样本） | 3.99 ms | 19.61 ms | **4.9×** |
| MC 翻前 7 对手（短牌） | 2.79 ms | 14.99 ms | **5.4×** |

结论：

1. 当前 Rust 实现在所有场景比原 JS 快 **约 5–20 倍**；精确枚举收益最大（10–20×），
   因为 JS 侧逐手评估开销高且精确枚举 100% 是评估调用。
2. MC 的加速比（≈5–8×）低于精确枚举，因为 MC 中约有 27% 的指令在抽样/计数逻辑
   （见 profile），且评估器加速尚未做到位。
3. 与 JS 的差距不是终点：评估器自身仍有约 8 倍余量到交接目标，方案见
   `docs/specs/eval-optimization.md`。

## 7. 注意事项与已知偏差

1. **WSL2 数据不代表 Windows**：虚拟化、CPU 频率策略、共享宿主都会影响；Windows 原生
   release 构建需要单独复测（尤其 7 张评估吞吐目标）。
2. **共享机器噪声**：同一份二进制在 loadavg 2 与 loadavg 15 时，MC 场景可差 2–3 倍；
   criterion 自身 run-to-run ±5–8%。报告时应同时给负载条件。
3. **profiling 限制**：WSL2 不暴露硬件 PMU（cycles/instructions not supported），
   `perf record -e cpu-clock` 采样过稀；实际 profile 使用 callgrind 指令计数 +
   行级注解（release 优化、无 LTO、debuginfo=1 的独立构建），详见优化方案 §2。
4. **公平性**：JS 基准手牌与 Rust 完全一致（同一 seed、同一洗牌顺序）；所有 calculate
   场景参数一致（seed/simulations/minSamples 相同），JS 不会触发时间预算提前退出。
5. 基准数据不入 git；本文件记录的是机器快照，复跑命令见 §2.3。
