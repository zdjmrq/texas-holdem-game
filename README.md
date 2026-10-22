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

工作已推送到 `rust-rewrite`，继续使用 Rust + egui。原版源码仅作只读参考，不作为运行时依赖。当前进展及 Windows 原生验证见 [docs/RESTORATION_REPORT.md](docs/RESTORATION_REPORT.md)。正常开局使用系统随机数；`--seed` 和 `--scenario` 用于可复现测试。

## 当前状态

- **本地对局可玩**：标准/短牌，真实 v1.5.8 移植 AI（人格/Range/River CFR/短牌策略），分析面板、加注面板、快捷键、音频、设置、双主题。
- **联机**：tokio+axum 服务端、可重连客户端及原生连接/大厅/牌桌/结算屏幕；服务端接入真实 Rust AI。
- **质量**：黄金语料、基础对局随机行动测试、Windows 本机发牌和结算回归；验证明细见当前报告。
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
poker-app      egui 桌面应用（本地对局 + 内嵌服务 + 原生联机界面）
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
2. **原生构建**：Windows 安装 VS Build Tools（C++ 工作负载）+ rustup（msvc host），运行 `scripts\build-windows.cmd`，产出 `dist/texas-holdem-rs-current/texas-holdem-rs.exe` 及同目录资源。默认不生成 zip。
3. **验收清单**：按 [docs/windows-smoke.md](docs/windows-smoke.md) 15 节逐项实测（中文 IME、托盘闪红、无边框缩放、DX12 帧率、DPI、音频、启动/内存），结果填入模板。
   WSLg 的渲染/IME/托盘数据不作为验收依据；IME 若不达标请暂停 UI 铺开并反馈。

## 克隆体积与本地产物

当前分支已跟踪文件约 40 MiB，主要是字体、音乐和黄金测试数据。`target/` 是可重建的 Rust 编译缓存，完整桌面依赖的调试构建会占用数 GB；`dist/` 是本地可运行程序。这两个目录均被 Git 忽略，不会因克隆而下载。正常开发构建还会另外下载 Rust 依赖并重新生成缓存。

只取 Rust 分支当前版本：

```bash
git clone --single-branch --branch rust-rewrite --depth 1 https://github.com/zdjmrq/texas-holdem-game.git
```

工作结束时保留一份当前可运行目录，将确认无用的旧产物移入 Windows 回收站。不要把缓存、便携包或 zip 提交到 Git。

## 文档

- 交付报告：[docs/FINAL_REPORT.md](docs/FINAL_REPORT.md)
- 交接总纲 / 决策 / 状态：[docs/RUST_REWRITE_HANDOFF.md](docs/RUST_REWRITE_HANDOFF.md) / [docs/DECISIONS.md](docs/DECISIONS.md) / [docs/STATUS.md](docs/STATUS.md)
- 黄金语料：[docs/golden-corpus.md](docs/golden-corpus.md)
- 规格：协议 [docs/specs/protocol-v2.md](docs/specs/protocol-v2.md)、服务端 [docs/specs/server-plan.md](docs/specs/server-plan.md)、AI [docs/specs/ai-strategy.md](docs/specs/ai-strategy.md)、UI [docs/specs/ui.md](docs/specs/ui.md)
- 性能：[docs/benchmarks.md](docs/benchmarks.md)；egui spike：[docs/spikes/egui-spike.md](docs/spikes/egui-spike.md)
