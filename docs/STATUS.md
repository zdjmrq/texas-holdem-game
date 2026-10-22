# 项目状态（2026-10-23 夜间自动推进）

> 用户已确认：所有待拍板项按推荐默认值执行；Windows 实机验收由用户之后完成；不做 Android / 32 位 Windows。
> 本文件由 Lead 维护，记录各 crate 状态与可复现验证命令。

## 里程碑

| 阶段 | 状态 | 关键证据 |
| --- | --- | --- |
| Phase 0 环境/骨架/黄金语料 | ✅ | workspace、oracle 工具链、参考仓库 e0c6c6f、JS 基线 152/152 |
| Phase 1 poker-core | ✅ | 15,052 用例 + 全部 5 张组合穷举哈希与 JS 一致 |
| Phase 2 poker-engine | ✅ | 240 手 / 1,707 步黄金回放逐状态一致；poker-cli 自对局 |
| Phase 3 poker-prob | ✅ | 固定 seed MC 与 JS 逐样本一致；精确 105,646 样本对齐；性能目标大部分达成 |
| Phase 5 poker-protocol | ✅ | 20 种消息、215 条黄金消息、18 条解析错误矩阵 |
| Phase 5 poker-server | ✅ 阶段 A / 🚧 B | 完整 WS 服务端 + JS↔Rust 双向互通 11 项 check；集成 11 项测试 |
| Phase 4 poker-ai | 🚧 | 规格 1714 行 + 262 条决策语料；A 核心 / B river-CFR / C 短牌 并行实现中 |
| Phase 6 poker-app | 🚧 本地可玩 | 38 项测试、7 张截图、标准+短牌完整对局、分析面板/键盘/音频/设置 |
| Phase 6 poker-client | ✅ | 33 项测试，重连/resume/关闭码/退避与 JS 黄金一致 |
| Phase 7 打包/性能 | ⏳ | Windows 构建与冒烟脚本已交付；待真实 AI 接入后收尾 |

## crate 状态

| crate | 状态 | 测试 |
| --- | --- | --- |
| poker-core | ✅ | `cargo test -p poker-core` |
| poker-engine | ✅ | `cargo test -p poker-engine`（含 240 手黄金回放） |
| poker-prob | ✅ | `cargo test -p poker-prob` |
| poker-protocol | ✅ | `cargo test -p poker-protocol` |
| poker-server | ✅ 阶段 A | `cargo test -p poker-server -- --test-threads=1` |
| poker-client | ✅ | `cargo test -p poker-client` |
| poker-ai | 🚧 | 实现中（golden baseline 待出） |
| poker-app | 🚧 本地可玩 | `cargo test -p poker-app`；`cargo run -p poker-app` |
| poker-cli | ✅ | `cargo test -p poker-cli` |

## 统一验证

```bash
scripts/verify-all.sh          # fmt + clippy + test + 黄金语料复现性检查
scripts/verify-all.sh --skip-oracle   # 快速检查
```

## 关键文档

- 交接总纲：docs/RUST_REWRITE_HANDOFF.md
- 决策记录：docs/DECISIONS.md
- 黄金语料：docs/golden-corpus.md
- 协议：docs/specs/protocol-v2.md；服务端：docs/specs/server-plan.md
- AI：docs/specs/ai-strategy.md
- 概率：docs/specs/prob.md
- 客户端：docs/specs/client.md
- UI 规格：docs/specs/ui.md
- egui spike：docs/spikes/egui-spike.md
- 应用状态/TODO：docs/specs/app-status.md
- 性能：docs/benchmarks.md
- Windows 冒烟：docs/windows-smoke.md；构建：scripts/build-windows.cmd

## 环境注意

- `static.crates.io` 不可达：~/.cargo/config.toml 已切 rsproxy。
- 多 agent 并行时各用独立 CARGO_TARGET_DIR。
- WSLg 只能软件渲染：性能/IME/托盘数据以 Windows 实机为准。
