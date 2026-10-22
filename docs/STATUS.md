# 项目状态（2026-10-23 自动推进中）

> 用户已确认：所有待拍板项按推荐默认值执行；Windows 实机验收由用户之后完成；不做 Android / 32 位 Windows。
> 本文件由 Lead 维护，记录各 crate 状态与可复现验证命令。

## 里程碑

| 阶段 | 状态 | 关键证据 |
| --- | --- | --- |
| Phase 0 环境/骨架/黄金语料 | ✅ | workspace 8 crate、oracle 工具链、参考仓库固定 e0c6c6f、JS 基线 152/152 |
| Phase 1 poker-core | ✅ | 15,052 条黄金用例 + 全部 5 张组合穷举哈希与 JS 一致 |
| Phase 2 poker-engine | ✅ | 240 手 / 1,707 步 ServerPokerGame 黄金回放逐状态一致；poker-cli 自对局 |
| Phase 3 poker-prob | ✅ 正确性 / 性能基线进行中 | 固定 seed MC 与 JS 逐样本一致；exact 105,646 样本对齐 |
| Phase 5 poker-protocol | ✅ | 20 种消息 DTO、215 条黄金消息、18 条解析错误、配置/名字规范化对齐 |
| Phase 5 poker-server | 🚧 实现中 | 规格 docs/specs/server-plan.md；task-7 |
| Phase 4 poker-ai | 🚧 规格+语料完成，实现待启 | docs/specs/ai-strategy.md；262 条决策黄金语料（含 21 river CFR） |
| Phase 6 poker-app | 🚧 垂直切片中 | egui spike GO（截图+Windows cross-check）；task-8 |
| Phase 7 打包/性能 | ⏳ | 依赖 Phase 6；Windows 构建脚本在 task-8 第一步 |

## crate 状态

| crate | 状态 | 测试 |
| --- | --- | --- |
| poker-core | ✅ 牌/牌堆/RNG/评估器 | `cargo test -p poker-core` |
| poker-engine | ✅ 下注状态机/底池/结算 | `cargo test -p poker-engine`（含黄金回放） |
| poker-prob | ✅ 权益/outs、性能基线进行中 | `cargo test -p poker-prob` |
| poker-protocol | ✅ 协议 v2 DTO/校验 | `cargo test -p poker-protocol` |
| poker-ai | 🚧 待实现（有规格+语料） | — |
| poker-server | 🚧 | — |
| poker-app | 🚧 egui spike 已跑通 | `cargo test -p poker-app` |
| poker-cli | ✅ 自对局 harness | `cargo test -p poker-cli` |

## 统一验证命令（Linux/WSL）

```bash
source ~/.cargo/env
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
node oracle/golden-eval.js        # 重新生成评估器黄金语料（需 reference 仓库）
node oracle/golden-engine.js      # 引擎黄金语料
node oracle/golden-equity.js      # 权益黄金语料
node oracle/golden-ai.js          # AI 决策黄金语料
node oracle/golden-protocol.js    # 协议黄金语料
```

## 环境注意

- `static.crates.io` 在本机不可达：`~/.cargo/config.toml` 已切 rsproxy；若下载再卡住改用 `--offline`。
- 多 agent 并行时各用独立 `CARGO_TARGET_DIR`。
- WSLg 只有软件渲染，性能/IME/托盘数据不作为 Windows 验收依据。

## 文档索引

- 交接总纲：`docs/RUST_REWRITE_HANDOFF.md`
- 决策记录：`docs/DECISIONS.md`
- 黄金语料规范：`docs/golden-corpus.md`
- 协议规格：`docs/specs/protocol-v2.md` / 服务端方案：`docs/specs/server-plan.md`
- AI 规格：`docs/specs/ai-strategy.md`
- 概率 API：`docs/specs/prob.md`
- egui spike：`docs/spikes/egui-spike.md`
- UI 规格：`docs/specs/ui.md`（生成中）
- 性能基线：`docs/benchmarks.md`（生成中）
