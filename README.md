# 德州扑克 Rust 重写版

用 Rust 从零重写的德州扑克桌面游戏：标准德州（52 张）+ 短牌（6+，36 张），本地 AI 对局与 WebSocket 联机房间。
原项目为 JS/Electron v1.5.8；Rust 版从原实现导出黄金语料做逐项行为验收（规则/牌力/底池/协议精确一致，概率与 AI 统计等价）。

## 分支说明

| 分支 / 标签 | 内容 |
| --- | --- |
| `main` | **原 JS/Electron 版本 v1.5.8**（保持不动，作为迁移对照与黄金语料 oracle） |
| `rust-rewrite` | **Rust 全量重写版（当前分支）**：9 个 crate + egui 桌面应用 + 联机服务端/客户端 |
| 标签 `v2.0.0-rust-preview.1` | Windows 测试基线：附带在 Linux 上交叉构建的 x86_64 便携包（冒烟用）；正式构建用 Windows 原生 MSVC |

## 本轮原生界面与核心收尾

本轮工作分支为 `codex/rust-ui-completion`，基于 `rust-rewrite`，继续使用 Rust + egui。原版源码仅作只读参考，不作为运行时依赖。当前进展及 Windows 原生验证见 [docs/RESTORATION_REPORT.md](docs/RESTORATION_REPORT.md)，以下早期交付描述以新报告为准。

## 当前状态

- **本地对局可玩**：标准/短牌，真实 v1.5.8 移植 AI（人格/Range/River CFR/短牌策略），分析面板、加注面板、快捷键、音频、设置、双主题。
- **联机**：tokio+axum 服务端 + 可重连客户端；已通过与原 JS 客户端的双向互通测试。联机 UI 逻辑层完成，屏幕层（阶段 2b）待续。
- **质量**：约 41,800 行 Rust、194 个测试；`cargo fmt/clippy(-D warnings)/test --workspace` 全绿；5 套黄金语料可复现。
- **性能**：7 张牌评估 42.1 M/s（标准）/ 49.8 M/s（短牌），约为原 JS 的 6.4/6.7 倍。
- 完整交付说明与已知差距见 [docs/FINAL_REPORT.md](docs/FINAL_REPORT.md)。

## 结构

```
poker-core     牌/牌堆/可注入 RNG/查表牌力评估
poker-engine   下注状态机、底池/边池、退款、摊牌结算
poker-prob     精确权益、蒙特卡洛（与 JS 逐样本一致）、outs
poker-ai       人格/范围/决策流水线、River CFR、短牌策略
poker-protocol 协议 v2 DTO/校验/错误码/配置
poker-server   tokio+axum WebSocket 房间服务
poker-client   WebSocket 客户端（重连/resume）
poker-app      egui 桌面应用（本地对局 + 内嵌服务 + 联机逻辑层）
poker-cli      自对局/基准/黄金语料工具
```

依赖方向严格单向：`poker-core <- poker-prob / poker-engine <- poker-ai <- poker-protocol <- poker-server / poker-app <- poker-cli`。

## 开发与测试

```bash
source ~/.cargo/env
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
scripts/verify-all.sh          # 完整验收（需要 ../reference/texas-holdem-game 参考仓库）
```

## 运行

```bash
cargo run -p poker-app                        # 启动桌面游戏
cargo run -p poker-app -- --scenario settlement --screenshot /tmp/a.png   # 无头场景+截图
cargo run -p poker-server --example serve     # 单独启动联机服务（打印 ws:// 地址）
cargo run -p poker-cli -- sim --players 6 --hands 500 --policy aggressive --refill
```

## Windows 测试

1. **快速冒烟**：从本仓库 Release 标签 `v2.0.0-rust-preview.1` 下载便携包，或在 Actions 的 `windows-build` 工作流里下载产物；解压后双击 `texas-holdem-rs.exe`（assets 必须同目录）。
2. **原生构建**：Windows 安装 VS Build Tools（C++ 工作负载）+ rustup（msvc host），运行 `scripts\build-windows.cmd`，产出 `dist/texas-holdem-rs-windows-x64.zip`。
3. **验收清单**：按 [docs/windows-smoke.md](docs/windows-smoke.md) 15 节逐项实测（中文 IME、托盘闪红、无边框缩放、DX12 帧率、DPI、音频、启动/内存），结果填入模板。
   WSLg 的渲染/IME/托盘数据不作为验收依据；IME 若不达标请暂停 UI 铺开并反馈。

## 文档

- 交付报告：[docs/FINAL_REPORT.md](docs/FINAL_REPORT.md)
- 交接总纲 / 决策 / 状态：[docs/RUST_REWRITE_HANDOFF.md](docs/RUST_REWRITE_HANDOFF.md) / [docs/DECISIONS.md](docs/DECISIONS.md) / [docs/STATUS.md](docs/STATUS.md)
- 黄金语料：[docs/golden-corpus.md](docs/golden-corpus.md)
- 规格：协议 [docs/specs/protocol-v2.md](docs/specs/protocol-v2.md)、服务端 [docs/specs/server-plan.md](docs/specs/server-plan.md)、AI [docs/specs/ai-strategy.md](docs/specs/ai-strategy.md)、UI [docs/specs/ui.md](docs/specs/ui.md)
- 性能：[docs/benchmarks.md](docs/benchmarks.md)；egui spike：[docs/spikes/egui-spike.md](docs/spikes/egui-spike.md)
