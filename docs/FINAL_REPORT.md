# 德州扑克 Rust 重写 — 最终交付报告（2026-10-23）

> 历史交付记录：后续修复与当前验证请看 [RESTORATION_REPORT.md](RESTORATION_REPORT.md)。本文的“已知差距”保留作为接手时的基线。

> 目标：用 Rust 从零重写原 JS/Electron 德州扑克 v1.5.8 的主桌面版（Windows x64，含标准德州 + 短牌、本地 AI、WebSocket 联机、egui 原生 UI）。
> 范围：不做 Android、不做 32 位 Windows（用户已确认）。原项目只读参考，不修改。

## 1. 当前状态（最重要）

| 模块 | 状态 | 证据 |
| --- | --- | --- |
| 牌/牌堆/RNG/牌力评估（标准+短牌） | ✅ | 15,052 条 JS 黄金用例 + 双变体全部 5 张组合穷举哈希逐位一致 |
| 下注引擎（盲注/前注、边池、退款、摊牌、奇数筹码） | ✅ | 240 手 / 1,707 步 JS 黄金回放逐状态一致 |
| 概率/权益（精确枚举 + 蒙特卡洛 + outs） | ✅ | 固定 seed MC 与 JS **逐样本一致**；精确枚举 105,646 样本一致 |
| AI（人格/范围/决策/River CFR/短牌） | ✅ 已实现并接入 app | 262 条决策回放（短牌 82/82=100%、标准 88.9%、CFR 21/21 policy 一致）；app 本地对局已用真实 AI |
| 协议 v2 | ✅ | 20 种消息、215 条黄金消息、18 条解析错误矩阵一致 |
| 联机服务端 | ✅ | 25 项测试；**原 JS 客户端 ↔ Rust 服务端** 与 **Rust 客户端 ↔ 原 JS 服务端** 均通过 |
| 联机客户端（重连/resume） | ✅ | 33+ 项测试，退避序列与 JS 黄金一致 |
| 桌面 UI（egui） | ✅ 本地对局可玩 | 标准+短牌完整对局、分析面板、加注面板、键盘、音频、设置、双主题；72 项测试 |
| Windows 构建 | ✅ 交叉构建产物 + 原生脚本 | dist/texas-holdem-rs-windows-x64.zip（GNU 交叉构建冒烟用）；Windows 原生 MSVC 用 scripts/build-windows.cmd |
| 性能 | ✅ 7 张评估 42.1/49.8 M/s（原 6.55/7.40） | docs/benchmarks.md |

最终验证（本机 WSL）：`scripts/verify-all.sh --skip-oracle« → fmt + clippy(-D warnings) + workspace 全测试 + app 冒烟 **全部通过**。
带 oracle 的完整检查：`scripts/verify-all.sh«（需 reference 仓库，会重新生成全部黄金语料并检查零漂移）。

## 2. 代码结构（9 个 crate，单向依赖）

```
poker-core    牌/牌堆/可注入 RNG/查表牌力评估（标准 52 + 短牌 36）
poker-engine  下注状态机、底池/边池、退款、摊牌结算、action meta
poker-prob    精确权益、蒙特卡洛（与 JS 逐样本一致）、outs
poker-ai      人格/风格、PlayerModel/OpponentRange、preflop/postflop 决策、River CFR、短牌策略
poker-protocol 协议 v2 DTO/校验/错误码/配置白名单
poker-server  tokio+axum WebSocket 房间服务（心跳/限流/重连/房主迁移）
poker-client  WebSocket 客户端（终端码/退避/resume/消息流）
poker-app     egui 桌面应用（本地对局 + 内嵌服务 + 联机逻辑层）
poker-cli     自对局/基准/黄金语料工具
```

## 3. 如何运行

### Linux/WSL（开发环境）
```bash
source ~/.cargo/env
cargo run -p poker-app                                   # 启动游戏（本地对局）
cargo run -p poker-app -- --scenario settlement --screenshot /tmp/a.png   # 无头场景+截图
cargo run -p poker-cli -- sim --variant standard --players 6 --hands 500 --policy aggressive --refill
cargo run -p poker-server --example serve                # 单独启动联机服务（打印 ws:// 地址）
```

### Windows x64
- **快速冒烟（无需 Rust 环境）**：`dist/texas-holdem-rs-windows-x64.zip«，解压后双击 `texas-holdem-rs.exe«（assets 必须同目录）。这是 Linux 上 x86_64-pc-windows-gnu 交叉构建，仅供冒烟；正式版建议 Windows 原生构建。
- **原生 MSVC 构建**：Windows 上安装 VS Build Tools（C++ 工作负载）+ rustup msvc，然后双击 `scripts\build-windows.cmd«（或运行 `scripts\build-windows.ps1«），产出 `dist/texas-holdem-rs-windows-x64.zip«。
- **验收**：按 `docs/windows-smoke.md« 15 节清单实测（中文/IME/无边框/托盘闪红/音频/DPI/60fps/启动时间/内存），结果填回收模板。
  ⚠️ 中文输入法（微软拼音）、托盘/任务栏闪烁、DX12 帧率、DPI 只能在 Windows 实机确认，WSLg 数据无参考价值。

## 4. 关键文档

| 文档 | 内容 |
| --- | --- |
| docs/STATUS.md | 各 crate 状态与统一验证命令 |
| docs/DECISIONS.md | 已拍板决策（用户确认的默认值） |
| docs/golden-corpus.md | 黄金语料规范/权威 oracle/复现方法 |
| docs/specs/protocol-v2.md / server-plan.md / interop.md | 协议、服务端、双向互通 |
| docs/specs/ai-strategy.md (1714 行) / ai-server-adapter.md | AI 移植规格与接入契约 |
| docs/specs/prob.md / client.md / ui.md (1555 行) / online-ui-plan.md | 概率/客户端/UI 规格 |
| docs/spikes/egui-spike.md | egui 风险 spike（GO，含 Windows 待测清单） |
| docs/benchmarks.md | 性能基线与 JS 同机对比 |
| docs/windows-smoke.md | Windows 验收清单 + 结果模板 |
| docs/specs/app-status.md | app 已完成/桩/TODO |

## 5. 已知差距与后续建议（按优先级）

1. **AI 标准模式决策对齐度 88.9%**（目标 ≥95%）：剩余差异已定位到 selectCandidate 的 RNG 消耗次序（solver 前 eligible 集合）与 checked-River 的 belief 权重接线；短牌 82/82=100%。验收口径是统计等价（BB/100），不是逐手一致，但建议继续收窄后打开 GOLDEN_AI_STRICT gate。方法见 docs/specs/ai-strategy.md §11。
2. **服务端 AI 仍是确定性桩**：`ServerAi«/`AiFactory« 接口已就绪，按 docs/specs/ai-server-adapter.md 接入 poker-ai 即可（约 1 个适配器 + 工厂）；当前联机 AI 强度不代表最终效果。
3. **联机 UI 阶段 2b 未完成**：协议客户端、服务端互通、联机逻辑层（OnlineView/OnlineController/内嵌服务）都已就绪，缺 worker 线程 + 连接/大厅/房间/结算屏幕的 egui 实现（方案与 7 处挂载点见 docs/specs/online-ui-plan.md）。
4. **5/6 张评估仍走 reference 路径**：7 张查表路径已完成（42-50 M/s）；5/6 张可复用同表后续优化。
5. **Windows 实机验收未做**：需用户按冒烟清单执行；IME 不达标时的回退方案见交接文档 §6.3。
6. 精确权益 rayon 并行已实现但本机无收益，未合入（bit-exact 保留在文档）。

## 6. 与原 JS 行为的主动差异（均已文档化）

- Rust 解析比 JS 严格（异常客户端由 INVALID_REQUEST 拒绝；旧客户端不受影响）。
- 1009 reason 文本非空串；Origin 拒绝在 HTTP 401 层（客户端只看 code）。
- MC 时间预算改为显式样本数（确定性模式），便于黄金逐样本对拍。
- hello.appVersion 兼容期报 1.5.8（POKER_APP_VERSION 可覆盖）。

## 7. 复现全部验收

```bash
scripts/verify-all.sh            # 需要 ../reference/texas-holdem-game (commit e0c6c6f)
```
黄金语料生成脚本：oracle/golden-{eval,engine,equity,ai,protocol}.js（确定性、可重复、manifest 含 sha256）。
