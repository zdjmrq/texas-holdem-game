# 德州扑克 Rust 全量重写 — 交接文档

> 执行者：dsh
> 编写日期：2026-10-23
> 原项目：<https://github.com/zdjmrq/texas-holdem-game>
> 原版本：v1.5.8
> 原仓库 HEAD：`e0c6c6f88d3e64324e1d83bfc700dd3a77f71b90`（2026-10-08）
> 工作区：`/home/dsh/projects`
> 状态：决策已拍板；Rust 工具链已部分就绪；等待执行

---

## 0. 给 dsh 的摘要（先读这段）

目标不是"把 Electron 换成 Tauri"，而是**用 Rust 从零重写整个项目**，包括牌局规则、AI、概率/CFR、联机服务、桌面 UI 和打包。原项目留在 Git 历史里做参考和黄金语料来源，不修改原仓库。

已拍板的四件事：

| 决策点 | 结论 |
| --- | --- |
| UI 方案 | **egui 原生 UI**，不保留 WebView/HTML/CSS |
| 目标平台 | **Windows 优先**；WSL 只用于核心库开发与测试；Android 暂不在范围 |
| AI 验收 | **统计等价**，不追求逐手决策一致 |
| ultracode | 重写期间**关闭**；骨架和黄金语料完成后，再用于并行移植、独立审查、测试排查 |

执行原则：

1. **先正确，后性能。** 每个 crate 先通过规则/黄金测试，再用 `criterion` 做基准优化。
2. **不要在没有黄金语料的情况下重写 AI。** 原项目的 AI 有大量 benchmark 和 152 项回归，是这次迁移最重要的验收资产。
3. **遇到新的架构/产品决策先问用户，不要自行拍板。** 本文档末尾列了已知的待拍板项。
4. **不改原仓库。** 新项目独立目录，原仓库只读参考。
5. **性能测试在 Windows 上做。** WSLg 下的帧率和 GPU 数据没有代表性。

---

## 1. 背景与目标

### 1.1 用户的目标

- 原项目是 Electron + 原生 JS 的德州扑克游戏，功能完整，运行在 Windows 上，用户电脑可以轻松运行。
- 用户希望**彻底用 Rust 重写**，追求极致代码性能，同时把这次重写当作熟悉 agent 协作的长期任务。
- 不是因为当前机器跑不动；内存/性能是重写要顺带改善的指标，不是唯一动机。
- 用户要求：有判断先沟通，重大决策等用户拍板；ultracode 在重写阶段先不开。

### 1.2 这次重写的性能重点

真正值得投入性能优化的地方：

1. **手牌评估**（standard 52 张 / short deck 36 张）。
2. **权益/胜率蒙特卡洛**（多对手、范围加权、牌张移除）。
3. **River CFR 求解**与 AI 决策树。
4. **AI 决策延迟**、**多核并行**（`rayon`）和内存占用。
5. **联机服务**的并发与稳定性（房间、限流、断线重连）。

UI 改成 egui 的主要收益是去掉 Chromium/WebView 依赖、降低内存和启动时间；它不会让"牌局计算"更快。性能预算应优先给 `poker-core` / `poker-prob` / `poker-ai`。

### 1.3 非目标

- 不追求 AI 逐手决策与 JS 版本 bit-for-bit 一致（见 §6.2）。
- 不在第一版做 Android。
- 不保留 Electron 壳或 Web UI。
- 不在第一阶段做像素级视觉还原；目标是高保真、功能完整、交互一致。
- 不做神经网络；沿用原项目的程序算法/CFR 思路。

---

## 2. 原项目事实清单

### 2.1 规模

| 指标 | 数值 |
| --- | --- |
| Git 文件数 | 251 |
| 源码（JS/HTML/CSS） | **16,571 行** |
| 测试 + 工具 | **3,863 行** |
| npm 运行时依赖 | 仅 `ws@8.21.3` |
| devDependency | `electron@^31` |
| 音乐资源 | 4 个 mp3，约 20 MB |
| benchmark 数据 | 159 个文件（`benchmarks/`） |
| 其他平台 | `android/` WebView 壳（本次不做） |

源码分布：

| 部分 | 行数 |
| --- | ---: |
| UI（`js/ui.js` 3,341 + `index.html` 490 + `css/style.css` 1,970） | 5,801 |
| AI（`ai-core` 1,749、`ai-shortdeck-stable` 1,339、`ai.js` 1,056、概率/CFR/Worker 等 718） | 4,862 |
| 牌局引擎/规则（`engine` 880、`game` 776、`shortdeck` 999、`cards` 312、`game-rules-core` 87） | 3,054 |
| 联机服务（`local-server` 738、`server-game` 675、`poker-rules` 128） | 1,541 |
| 壳/音频/网络/调试（`main` 152、`preload` 13、`desktop-attention` 93、`audio` 298、`network` 377、`controls` 134、`debug` 246） | 1,313 |

### 2.2 关键源码地图

| 原文件 | 作用 | Rust 归属 |
| --- | --- | --- |
| `js/cards.js` | 52 张牌、牌堆、洗牌 | `poker-core` |
| `js/game-rules-core.js` | 加注网格、边池、未跟注退款、action meta、decision identity | `poker-core` / `poker-engine` |
| `js/probability-core.js` | 牌力评估（标准/短牌）、权益 | `poker-core` / `poker-prob` |
| `js/probability.js` + `probability-worker.js` | 主线程 Worker 封装 | `poker-prob`（Rust 内部线程） |
| `js/engine.js` | 变体无关的下注状态机（29 个方法） | `poker-engine` |
| `js/game.js` | 标准德州 `PokerGame` | `poker-engine` |
| `js/shortdeck.js` | 短牌 `ShortDeckGame` + 短牌 AI 适配 | `poker-engine` / `poker-ai` |
| `js/ai-core.js` | 统一 AI 核心、范围/权益/决策 | `poker-ai` |
| `js/ai-shortdeck-stable.js` | v1.5.6 已验证短牌策略 | `poker-ai` |
| `js/ai.js` | `AIPlayer`、人格/风格、`PlayerModel`、`OpponentRange` | `poker-ai` |
| `js/river-solver.js` + `compiled-cfr.js` | 河牌范围博弈、CFR | `poker-ai` |
| `js/ai-worker.js` + `ai-worker-client.js` | AI Worker | Rust 线程/任务 |
| `server/local-server.js` | WebSocket 房间服务、协议 v2 | `poker-server` |
| `server/server-game.js` | 服务端牌局 + 服务端 AI 适配 | `poker-engine` / `poker-server` |
| `server/poker-rules.js` | 服务端规则/洗牌 | `poker-core` |
| `js/ui.js` + `index.html` + `css/style.css` | 全部界面 | `poker-app`（egui） |
| `js/audio.js` | 音乐/音效 | `poker-app`（rodio） |
| `js/network.js` | 联机客户端、重连、sessionStorage | `poker-server` client / `poker-app` |
| `js/controls.js` | 键盘快捷键 | `poker-app` |
| `js/debug.js` | 调试面板 | `poker-app`（可选） |
| `main.js` / `preload.js` / `desktop-attention.js` | Electron 壳、托盘、任务栏闪烁 | `poker-app`（winit/tray-icon/windows） |
| `tests/*.test.js` | 152 项回归 | 各 crate 的 `tests/` + 黄金语料 |
| `tools/ai-*.js` | AI 实验/锦标赛/验收 | `poker-cli` / `benches/` |

### 2.3 原项目运行方式

```bash
npm ci            # 会装 Electron 31，体积大
npm start         # 启动 Electron
npm run server    # 只启动 WS 服务
npm test          # node --test tests/*.test.js
npm run test:all  # 单测 + AI smoke
```

本机 WSL 已有 Node `v24.21.0` / npm `11.19.0`。只跑核心测试和黄金语料时，用 `npm ci --omit=dev` 即可（只装 `ws`），不要为了跑测试下载 Electron。

### 2.4 原项目的性能/内存基线（来自 v1.5.8 报告）

- 默认 GPU 模式静态界面：工作集合计约 **977.71 MiB**，Private Bytes **697.49 MiB**。
- `--poker-low-memory`：约 **614.58 / 234.41 MiB**；滚动 P95 从 8.40ms 增至 16.60ms。
- AI：16,384 手确认赛局，相对 v1.5.7 **+1.79 BB/100**（95% 区间 +1.13～+2.46）。
- 河牌 Worker 18 个针对性场景：平均 **53.91ms**，最大 **99.10ms**；连续筹码全场平均 8.47ms，P95 17.76ms。
- Windows/Linux 完整回归各 **152 项通过**。

注意：这些是原项目的读数，不能直接当作 Rust 版的验收数字；它们用于判断"新实现是否明显退步"和"性能提升是否真实"。

---

## 3. 目标架构

### 3.1 Cargo workspace

```text
texas-holdem-rs/
├── Cargo.toml                # workspace, resolver = "3", edition = "2024"
├── rust-toolchain.toml       # stable + clippy + rustfmt
├── .cargo/config.toml        # crates.io 镜像
├── crates/
│   ├── poker-core/           # 牌、牌堆、RNG、标准/短牌牌力评估
│   ├── poker-engine/         # 下注状态机、底池/边池、退款、摊牌
│   ├── poker-prob/           # 权益、outs、蒙特卡洛、范围加权
│   ├── poker-ai/             # 人格、PlayerModel、OpponentRange、CFR
│   ├── poker-protocol/       # serde DTO、v2 JSON 协议、错误码
│   ├── poker-server/         # tokio + axum WebSocket、房间、限流
│   ├── poker-app/            # egui/eframe 桌面应用
│   └── poker-cli/            # 模拟、基准、黄金语料导出/对比工具
├── assets/                   # 牌面、图标、音乐（复用原项目）
├── tests/golden/             # 从 JS oracle 导出的黄金语料（JSONL）
├── benches/                  # criterion
├── oracle/                   # Node 脚本：导出/对比原 JS 行为（只读参考）
└── docs/                     # 本文件、性能记录、协议说明
```

建议的依赖方向（严格单向）：

```text
poker-core
   ↑        ↑
poker-prob  poker-engine
   ↑        ↑
   └─ poker-ai ─┘
        ↑
poker-protocol
        ↑
poker-server / poker-app
        ↑
poker-cli
```

`poker-core`、`poker-engine`、`poker-prob`、`poker-ai` 必须是**纯计算库**：无 async、无文件/网络 I/O、无全局状态。所有随机性通过注入的 `RandomSource` 获得，所有配置显式传入。这样既能做确定性黄金测试，也能并行化。

### 3.2 关键设计约束

1. **RNG 可注入**：所有洗牌、AI 混合、蒙特卡洛都走同一个 `RandomSource` trait。默认用 `rand_chacha::ChaCha8Rng`，测试用固定种子或脚本序列。
2. **协议保持兼容**：`poker-protocol` 的 JSON 字段名、消息类型、`protocolVersion = 2` 与原项目一致；Rust 服务端应能与原 JS 客户端互通，便于分阶段替换。
3. **浮点用 `f64`**：与原 JS 的 IEEE-754 双精度对齐。允许实现顺序不同，但不要在核心规则里引入不必要的精度差异。
4. **不用 `unsafe` 起步**：先 `#![forbid(unsafe_code)]`；只有在 benchmark 证明收益且隔离良好时才局部放开（SIMD、查表）。
5. **查表评估器**：7 张牌评估用手牌编码 + 查找表（Cactus Kev / Two Plus Two 思路）。短牌单独一张表，注意短牌牌型强弱与 A 的低位顺子。
6. **不引入重型游戏引擎**：牌桌 UI 用 egui 自绘即可；不要上 Bevy。
7. **资源复用**：`assets/` 直接复用原项目的牌面/图标/音乐；用 `rust-embed` 或 `include_bytes!` 打包，保证便携版单文件可运行。

### 3.3 UI 技术选型

- `eframe` + `egui`，`wgpu` 或 `glow` 后端。先用 `eframe` 默认后端；若 Windows 驱动兼容性有问题，退化到 `glow`。最终以基准和稳定性为准。
- 中文字体：内置 **Noto Sans SC**（OFL）或读取系统字体；必须验证中文输入法（IME）在 `TextEdit` 中可用。
- 音频：`rodio` + `symphonia`（mp3 解码）；音量、播放模式、切歌逻辑照原 `audio.js` 重写。
- 托盘/任务栏闪烁：`tray-icon` + `windows` crate（`FlashWindowEx`）；单实例锁用命名 mutex 或 `single-instance` crate。
- 窗口：无边框 + 自定义标题栏，保留拖动、双击最大化、边缘缩放、最小尺寸（原项目 900×600，默认 1280×860）。先做原生边框版本跑通，再补无边框细节。
- 动画：egui 的 `ctx.animate_value_with_time` / `request_repaint`；牌桌布局用统一坐标函数，避免座位跳动（原 v1.5.8 专门修过这个问题）。

### 3.4 服务端

- `tokio` + `axum`（`axum::extract::ws`）或 `tokio-tungstenite`；JSON 用 `serde_json`。
- 房间、房主迁移、准备状态、断线重连、resume token、空房 TTL（15s）、最大房间/连接数、按 IP 限流、Origin 白名单、代理信任开关，全部照 `server/local-server.js` 移植。
- 端口默认 `3000`；桌面应用启动时自动起本地服务并探测 `hello`，保留 EADDRINUSE 时换随机端口的逻辑。
- 姓名规范化：NFKC、去控制字符、最多 8 个字符（`unicode-normalization` crate）。
- 随机 token/uuid：`rand` + `base64` URL-safe / `uuid`。

---

## 4. 迁移顺序与阶段交付

每个阶段都必须有：**可运行的交付物 + 自动化验收 + 与 JS 的对比记录**。不要等全部写完再测。

### Phase 0 — 环境、骨架、黄金语料（1–2 天）

交付物：

1. 安装系统依赖（见 §7.1）。
2. `cargo new --lib` 各 crate，建立 workspace、CI、`rust-toolchain.toml`、`.cargo/config.toml`。
3. 克隆原仓库到只读参考目录，跑一遍 `npm ci --omit=dev && npm test` 记录基线。
4. 写 `oracle/` Node 脚本：
   - 用可注入的 seeded PRNG（例如 mulberry32）替换 `Math.random`，在加载模块前 patch。
   - 导出固定种子的牌局、牌力、合法动作、权益、AI 决策到 `tests/golden/*.jsonl`。
   - 服务端测试用 stub 替换 `crypto.randomUUID` / `randomBytes`。
5. **egui 风险 spike（2–3 天）**：一个最小窗口，验证中文显示、中文 IME、无边框拖动/缩放、托盘图标、任务栏闪烁、mp3 播放、60fps 动画。若 egui 在 IME/托盘/无边框上不达标，**停下来找用户重新拍板 UI 方案**，不要硬扛。

验收：`cargo test` 空跑通过；黄金语料能稳定复现（同 seed 两次输出一致）；egui spike 在 Windows 上可运行。

### Phase 1 — `poker-core`（1–2 周）

范围：

- 牌编码（0..51）、牌堆、洗牌（可注入 RNG）。
- 标准 52 张牌力评估；短牌 36 张牌力评估。
- 短牌特殊规则：A 可作低牌、顺子 A-6-7-8-9、同花大于葫芦、无 2–5。
- 牌型比较、踢脚、平局。

验收：

- 从 `tests/rules.test.js`、`tests/shortdeck-engine.test.js`、`tests/probability-core.test.js` 提取的黄金用例全部通过。
- 与 JS `probability-core.evaluate` 对随机 10 万手牌逐一对比（标准 + 短牌），结果完全一致。
- `proptest`：评估结果满足反对称/传递性；同花与顺子的边界不重叠。
- `criterion`：单次 7 张评估的 ns/op 记录在 `docs/benchmarks.md`。

### Phase 2 — `poker-engine`（1–2 周）

范围：

- 共享下注状态机（对应 `js/engine.js` 的 29 个方法）。
- 标准德州（盲注）与短牌（前注）两种强制下注。
- 合法动作、最小加注、加注网格（小盲/前注整数倍）、全下、边池、未跟注退款、零头分配、摊牌、单次结算。
- 服务端牌局 `ServerPokerGame` 的等价实现。

验收：

- 从 `tests/rules.test.js`、`tests/regression.test.js`、`tests/shortdeck-engine.test.js`、`tests/server.test.js` 提取的牌局脚本逐动作对比：合法动作集合、筹码变化、底池、边池、退款、结算完全一致。
- 属性测试：筹码守恒、底池总和 = 各边池之和、无负筹码、弃牌玩家不能赢、全下边池资格正确。
- 黄金语料中的 2048 手配置兼容 + 1536 手连续筹码场景零错误。

### Phase 3 — `poker-prob`（1–2 周）

范围：

- 精确枚举（翻牌/转牌/河牌权益、平局分摊）。
- 蒙特卡洛（多对手、范围加权、牌张移除、条件摊牌）。
- outs 计算、听牌分类、胜率分析面板所需数据。

验收：

- 精确枚举与 JS 结果在 `1e-12` 内一致；蒙特卡洛在固定种子下逐样本一致，或统计误差在预设置信区间内。
- 性能目标（建议，最终与用户确认）：
  - 9 对手翻前权益 P95 < 16ms（单核，或 4 核并行）；
  - 河牌精确权益 P95 < 5ms；
  - 10 万次 7 张评估吞吐 > 5,000 万次/秒/核（Release，查表实现）。
- `criterion` 基线写入 `docs/benchmarks.md`，并记录与 Node 版同机对比。

### Phase 4 — `poker-ai`（2–4 周）

范围：

- `AIPlayer`、人格/风格表、`PlayerModel`、`OpponentRange`、位置/牌面纹理分析。
- 范围权益、合法下注收益、风险/对手回应决策。
- River CFR：公开范围、合法组合排除撞牌、完整回应树、800 次迭代、混合策略。
- 短牌保留 v1.5.6 已验证策略。

验收：

- 用 `tools/ai-*.js` 的等价 Rust harness 跑相同配置：
  - 与 JS v1.5.8 在相同 seed/座位交换下对比 BB/100；统计等价，不要求逐手一致。
  - 非法操作、筹码错误、采样超时为 0。
  - 决策延迟：非 CFR P95 < 16ms；CFR P95 < 50ms、max < 100ms（建议目标，按 Windows 实测调整）。
- 黄金语料中的固定 seed 决策：允许边界差异，但差异率需记录并解释（例如浮点/混合策略阈值）。
- 独立 CFR 对照（Kuhn 均衡、全树 oracle）通过。

### Phase 5 — `poker-server` + `poker-protocol`（1–2 周）

范围：

- 协议 v2 全部消息（见 §8.1）。
- 房间生命周期、房主迁移、准备/开局、AI 延迟、断线重连、resume token、空房 TTL。
- 限流、消息大小上限、Origin 白名单、环境变量。
- 与原 JS 客户端的兼容测试。

验收：

- 原 `tests/server.test.js` 场景在 Rust 侧等价通过。
- 用原 JS 客户端连接 Rust 服务端，创建/加入/开局/行动/结算/重连全流程通过。
- 用 Rust 客户端连接原 JS 服务端，同样通过。
- 并发/限流/恶意消息测试：不崩溃、不泄漏、错误码一致。

### Phase 6 — `poker-app`（egui UI，3–6 周）

先做垂直切片，再补全：

1. 主页/模式选择/本地设置/联机面板。
2. 牌桌：座位、手牌、公共牌、底池、下注、动作按钮、动画、结算弹窗。
3. 分析面板：胜率、outs、牌型、手牌历史、隐藏手牌/分析遮蔽。
4. 设置：人数、筹码、盲注、AI 速度、音乐、音量、播放模式、全屏、提示音。
5. 键盘快捷键：Tab/Shift+Tab、Enter/Space、F/C/R/A、1/2、方向键、Esc、N。
6. 壳功能：无边框、拖动、缩放、置顶工具栏悬停区、托盘、任务栏/托盘闪烁、单实例、`--poker-low-memory` 等价开关。
7. 联机 UI：创建/加入房间、重连状态、房主迁移提示。

验收：

- 功能清单逐项对照原 `README.md` / `index.html` / `ui.js`（见 §8.2）。
- 手动回归清单：本地标准/短牌、联机、键盘、音乐、提醒、小窗口滚动。
- Windows 上 60fps（牌桌动画/滚动），启动 < 500ms（建议目标）。
- 中文 IME、字体、DPI 125%/150% 缩放正常。
- 内存目标（建议，最终确认）：RSS < 100MB 常态；无音乐预加载时更低。

### Phase 7 — 打包与性能收尾（1–2 周）

- Windows Release 构建、便携 zip、可选安装包（Inno Setup / WiX）。
- 图标、版本号、崩溃日志、单实例、首次启动。
- 全量基准：手牌评估、权益、CFR、整局决策、启动时间、内存、帧时间。
- 与 JS 基线对比，写 `docs/performance-report.md`。
- 修复性能回归；必要时局部 SIMD/多线程优化。

总计：单人全职约 **3–6 个月**；并行 agent 可加速独立模块和审查，但架构、协议、黄金语料必须串行把关。

---

## 5. 性能与验收方法

### 5.1 基准工具

- `criterion`：微基准（评估、权益、CFR 迭代、决策）。
- `poker-cli`：端到端模拟（自对局、固定 seed、座位交换、BB/100、错误计数、超时统计）。
- Windows：Task Manager / `GetProcessMemoryInfo` 记录 RSS/Private Bytes；帧时间用 egui 内置帧时间或 ETW。
- Linux/WSL：`perf`、`heaptrack`/`dhat`（仅用于开发期定位，不作为最终性能结论）。

### 5.2 验收分类

| 类别 | 标准 |
| --- | --- |
| 规则/牌力/合法动作/底池 | **精确一致**（黄金语料逐项） |
| 精确权益 | 浮点容差 `1e-12` |
| 蒙特卡洛权益 | 固定 seed 逐样本一致，或统计误差在置信区间内 |
| AI 决策 | **统计等价**：BB/100、胜率、非法操作=0、延迟达标；允许边界差异 |
| UI | 功能清单完整；交互一致；手动回归通过；不是像素级 |
| 性能 | 不低于本文建议目标；与 JS 同机对比有明确提升 |

### 5.3 黄金语料生成原则

1. 在 Node 里**先 patch `Math.random`**（mulberry32/ChaCha 的 JS 实现），再 `require` 原模块；否则无法复现。
2. 固定：seed、座位数、起始筹码、盲注、人格分配、行动脚本。
3. 记录：牌堆顺序、每人手牌、公共牌、每步合法动作、筹码/底池、评估结果、权益、AI 决策、耗时（耗时仅参考）。
4. 输出 JSONL，一行一个决策/牌局；大文件放 `tests/golden/`，用 `.gitignore` 排除超大样本，保留生成脚本。
5. **不要**把 benchmark 原始数据直接塞进 git；保留生成/对比脚本和汇总报告。

---

## 6. 已知风险与坑

### 6.1 规则正确性

- **短牌牌型强弱**：同花 > 葫芦；A 可作低牌；顺子 A-6-7-8-9；没有 2–5。必须用原 `probability-core.evaluate` 的黄金用例逐项核对。
- **边池/退款**：`calculateSidePots`、`findUncalledRefund` 的逻辑很细；弃牌玩家的筹码仍进池，但无资格；零头分配有特定规则。
- **加注网格**：普通加注按小盲（短牌按前注）整数倍；跟注/全下用实际整数筹码；平分底池允许零头。
- **行动元数据**：`buildActionMeta`、`decisionIdentity` 影响 AI 行为，必须原样移植。
- **摊牌顺序/平局分池**：多人平局、边池平局、奇数筹码分配。

### 6.2 AI 等价性

- JS 里 `Math.random` 既用于洗牌也用于 AI 混合；不 patch 就无法复现。
- 即使 patch，Rust 与 JS 的浮点运算顺序、`Math.round` vs Rust `round`、数组排序稳定性都可能造成边界决策差异。
- 因此验收用统计指标；固定 seed 的逐手对比只作为诊断，不作为硬性门槛。
- CFR 的迭代顺序、随机数消耗顺序会显著影响结果；移植时保持"每个决策重新求解、800 次迭代、完整回应树"的结构，不要顺手改成近似算法。

### 6.3 egui / Windows 壳

- **中文 IME**：`egui` 的文本输入依赖 winit IME；必须在 Phase 0 spike 里实测中文输入。若不达标，回退方案是保留 Tauri + Web UI（需用户重新拍板）。
- **中文字体**：egui 默认字体不含 CJK；内置 Noto Sans SC（OFL）或加载系统字体，注意字体体积和启动时间。
- **无边框窗口**：拖动、双击最大化、边缘缩放、DPI 缩放、最小尺寸都要自己处理；先用原生边框跑通，再补。
- **托盘/任务栏闪烁**：`tray-icon` + `windows` crate；WSL 下无法验证，必须在 Windows 测。
- **音频**：rodio 的 mp3 解码、音量、播放模式、切歌、静音、结算提示音；与 Web Audio 的行为不完全一样，需要手动回归。
- **UI 动画/布局**：egui 是立即模式，布局每帧重算；牌桌坐标用统一函数，避免座位跳动（原 v1.5.8 的教训）。
- **无障碍**：egui 的 AccessKit 支持有限；原项目有键盘操作，需保证键盘可达性。若无障碍是硬需求，提前告知用户。

### 6.4 联机协议

- 协议版本 `2`；`hello` 包含 `serverId: "texas-holdem-game"`、`protocolVersion: 2`、`appVersion`。
- 原桌面客户端探测服务端时要求 **appVersion 完全匹配**；Rust 服务端要么报告相同版本，要么同步改客户端探测逻辑。分阶段迁移时要先决定。
- 房间配置白名单：筹码 `[5000,10000,20000,50000,100000]`；盲注 `[[10,20],[20,40],[40,80],[50,100],[100,200],[200,400]]`；AI 延迟 `[800,1800,3200]`；空房 TTL 15s；名字最多 8 字符。
- 环境变量：`POKER_HOST`、`POKER_PORT`、`POKER_MAX_ROOMS`、`POKER_MAX_CONNECTIONS`、`POKER_MAX_CONNECTIONS_PER_IP`、`POKER_ALLOWED_ORIGINS`、`POKER_TRUST_PROXY`。
- 安全：消息大小上限、按 IP 限流、Origin 白名单、拒绝非法 JSON、断线清理；不要因为是本地游戏就省掉。

### 6.5 构建/平台

- WSL 交叉编译 Windows GUI 很痛；**Windows 上原生构建**。
- MSVC toolchain 需要 Visual Studio Build Tools（C++ 工作负载）。
- egui 在 Windows 用 wgpu/DX12 或 glow/OpenGL；老显卡/远程桌面可能有兼容问题，保留 glow 回退。
- 便携版需要嵌入资源或随包附带 `assets/`；两者都做，便于调试。

---

## 7. 开发环境

### 7.1 WSL 当前状态（2026-10-23）

已安装：

- Ubuntu 24.04.5 LTS
- `rustc 1.99.1`、`cargo 1.99.1`、`rustup 1.29.2`
- `clippy 0.1.99`、`rustfmt 1.10.0-stable`
- Node `v24.21.0`、npm `11.19.0`

已确认的问题：

- `static.rust-lang.org` 在本机**连接超时**；本次通过清华镜像安装。
- **缺少 C 链接器 `cc`**：`cargo build` 会在链接阶段失败。需要用户执行（需要 sudo 密码，agent 无免密 sudo）：

```bash
sudo apt update
sudo apt install -y build-essential pkg-config
```

- 后续 `rustup` 操作需要镜像环境变量（否则会去连 `static.rust-lang.org`）：

```bash
export RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup
export RUSTUP_UPDATE_ROOT=https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup
```

建议写进 `~/.bashrc` 或 `~/.profile`（执行前问用户确认）。

- egui/音频/窗口开发还需要（Ubuntu 24.04 包名）：

```bash
sudo apt install -y libxkbcommon-dev libgl1-mesa-dev libwayland-dev libx11-dev \
  libxcursor-dev libxi-dev libxrandr-dev libasound2-dev libfontconfig1-dev
```

- WSLg 可以显示 GUI，但**性能数据不可信**；最终验收在 Windows。

### 7.2 Cargo 镜像（建议）

`~/.cargo/config.toml`：

```toml
[source.crates-io]
replace-with = "tuna"

[source.tuna]
registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/"

[net]
git-fetch-with-cli = true
```

如果清华 crates 镜像不稳定，备选 `sparse+https://rsproxy.cn/index/`。

### 7.3 Windows 构建环境

1. 安装 Visual Studio 2022 Build Tools，勾选「使用 C++ 的桌面开发」。
2. 安装 rustup（Windows 侧），默认 host `x86_64-pc-windows-msvc`。
3. 若 rustup 下载慢，设置同样的 `RUSTUP_DIST_SERVER` / `RUSTUP_UPDATE_ROOT` 环境变量。
4. 验证：

```powershell
rustc --version
cargo --version
cargo build --release
```

5. 运行/性能测试都在 Windows 原生环境，不在 WSLg。

### 7.4 版本与工具链固定

`rust-toolchain.toml`：

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

建议在 CI 里固定一个具体版本（例如 `1.99.1`），避免 stable 漂移导致基准波动。

---

## 8. 附录

### 8.1 协议 v2 消息类型

客户端 → 服务端：

- `hello`（服务端发）
- `create_room`
- `join_room`
- `leave_room`
- `start_game`
- `player_action`
- `ready_for_next`
- `resume_session`

服务端 → 客户端：

- `hello`
- `room_created` / `room_joined`
- `game_starting`
- `game_state`
- `action_ack`
- `ready_status`
- `host_changed`
- `ack`
- `error`

`game_state` 是权威状态快照；客户端不要自行推断隐藏信息。服务端 AI 与本地 AI 共用同一核心。

### 8.2 UI 功能清单（验收对照）

主页/设置：

- 标准德州 / 短牌选择
- 本地对局 / 联机
- 人数、起始筹码、盲注、AI 速度
- 玩家名字
- 音乐开关、切歌、音量、播放模式、提示音开关
- 全屏

牌桌：

- 座位/头像/名字/筹码/状态（行动中、弃牌、全下、庄家位）
- 自己的两张手牌（可点击隐藏/显示）
- 公共牌、底池、当前下注、盲注/前注显示
- 弃牌/过牌/跟注/加注/全下；加注面板（滑块、1/2 快捷键、合法上下限、小盲步长）
- 行动解释、牌型名称、手牌计数、BB 显示
- 结算弹窗、下一手
- 牌桌历史/分析：胜率、outs、牌型、手牌强度
- 分析遮蔽：隐藏手牌时保护分析信息
- 顶部工具栏悬停展开/收起（小窗口/顶部滚出时）
- 窗口拖动、最小化、最大化/还原、关闭

联机：

- 创建/加入房间、房间码、玩家列表、准备、房主开始、房主迁移
- 断线重连、会话恢复、连接状态/错误提示
- 本地服务地址显示

提醒：

- 轮到你/结算时任务栏闪烁、托盘图标闪红
- 回到前台/获得焦点后停止；同一提示不重复提醒
- 结算提示音两声（受静音规则控制）

键盘：

- Tab/Shift+Tab、Enter/Space、F/C/R/A、数字 1/2、方向键、Esc、N

### 8.3 关键配置/常量

| 项 | 值 |
| --- | --- |
| 协议版本 | `2` |
| 服务端标识 | `serverId = "texas-holdem-game"` |
| 默认端口 | `3000` |
| 空房 TTL | `15000ms` |
| 名字最大长度 | `8` 字符（NFKC 后） |
| 筹码档位 | `5000, 10000, 20000, 50000, 100000` |
| 盲注档位 | `(10,20), (20,40), (40,80), (50,100), (100,200), (200,400)` |
| AI 延迟档位 | `800, 1800, 3200ms` |
| 默认窗口 | `1280×860`，最小 `900×600` |
| 默认配置 | 标准十人桌、起始 5000、盲注 100/200、25 BB（AI 重点配置） |
| 短牌牌数 | 36 张（6–A），前注制 |

### 8.4 原项目文档索引

- `README.md` — 功能、操作、联机、测试命令
- `AI_STRATEGY_DESIGN.md` — AI 设计
- `AI_STRATEGY_SPEC_v1.5.8.md` — 当前策略规格
- `AI_BENCHMARK_REPORT_v1.5.8.md` — 验收数据与局限
- `CHANGELOG_v1.5.8.md` — 最新改动、内存代价
- `RELEASE_ANDROID.md` — Android（本次不做，仅参考）
- `tests/*.test.js` — 152 项回归
- `tools/ai-*.js` — AI 基准/锦标赛/验收工具

---

## 9. 交接第一步（dsh 直接照做）

1. 让用户执行：

```bash
sudo apt update && sudo apt install -y build-essential pkg-config
```

2. 验证工具链：

```bash
source "$HOME/.cargo/env"
rustc --version && cargo --version
cargo new /tmp/hello-rust && cd /tmp/hello-rust && cargo run
```

3. 克隆原仓库到只读参考目录（不要改）：

```bash
mkdir -p /home/dsh/projects/reference
git clone https://github.com/zdjmrq/texas-holdem-game.git /home/dsh/projects/reference/texas-holdem-game
cd /home/dsh/projects/reference/texas-holdem-game
git checkout e0c6c6f88d3e64324e1d83bfc700dd3a77f71b90
npm ci --omit=dev
npm test
```

4. 创建新 workspace：

```bash
cd /home/dsh/projects
cargo new --lib texas-holdem-rs/crates/poker-core
# 继续创建其余 crate，或直接手写 workspace Cargo.toml
```

5. 先写 `oracle/` 导出脚本，生成第一批黄金语料；在 `poker-core` 里用这些语料建立第一条测试。

6. 做完 egui spike，把中文 IME/托盘/无边框/音频的实测结果反馈给用户；不达标就停下来重新拍板。

---

## 10. ultracode 使用计划（重写后/后期）

用户已明确：重写阶段先不开；搞定后可能用 ultracode 做**测试和排查**。

适合开 ultracode 的时机：

1. **骨架 + 黄金语料 + 协议固定之后**，并行移植独立模块（core/engine/prob/ai/server），每个模块一个 agent，输入是接口规格 + 黄金语料，输出是代码 + 测试。
2. **独立审查**：每个 crate 用 2–3 个不同视角的 reviewer（正确性、规则一致性、性能、协议兼容），findings 做对抗性验证。
3. **性能排查**：并行 profile 不同热点（评估、权益、CFR、UI 帧），独立优化方案 + judge panel。
4. **测试/故障排查**：失败用例 fan-out，每个 agent 复现 + 定位 + 给修复建议，最后统一验证。

不建议用 ultracode 做的事：

- 架构决策、协议设计、接口定义（需要单上下文一致性）。
- 多个 agent 同时改同一批文件（除非用 worktree 隔离）。
- 没有明确验收标准的"帮我重写一下"。

使用时保持：<10 个 agent/工作流、明确输入路径、结构化输出、findings 必须对抗性验证、不要静默截断覆盖范围。

---

## 11. 需要用户拍板的新问题

以下问题在重写过程中会遇到，dsh 不要自行决定：

1. **联机协议是否必须兼容旧 JS 客户端？** 建议：分阶段期间兼容（协议 v2 + 相同字段），Rust 客户端稳定后再考虑升级。
2. **Windows 分发形式**：便携 zip 优先，还是必须 installer（Inno Setup/WiX）？
3. **Android 是否最终要恢复？** 当前决策是不做；若以后要做，egui 的 Android 支持较弱，可能需要重新评估 UI 方案。
4. **视觉还原程度**：egui 高保真即可，还是必须像素级接近原 Web UI？后者成本显著上升。
5. **无障碍/键盘可达性**是否有硬性要求？
6. **是否保留原项目的调试面板/benchmark 工具**，还是只保留面向玩家的功能？

---

## 12. 完成定义（Definition of Done）

- `cargo test` 全绿；`cargo clippy -- -D warnings` 全绿；`cargo fmt --check` 全绿。
- 黄金语料：规则/牌力/合法动作/底池精确一致；权益在容差内；AI 统计指标不劣于 JS v1.5.8。
- Windows Release 可运行：本地标准/短牌、联机、键盘、音乐、提醒、设置、结算全部通过手动回归。
- 性能报告：评估/权益/CFR/决策/启动/内存/帧时间有数据，与 JS 同机对比。
- 无 Chromium/WebView/Electron 依赖；便携包可在干净 Windows 机器运行。
- 文档：协议、架构、基准、已知差异、回退方案齐全。
