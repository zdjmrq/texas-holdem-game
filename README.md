# 德州扑克 Rust 重写版

用 Rust 从零重写的德州扑克桌面游戏：标准德州（52 张）+ 短牌（6+，36 张），本地 AI 对局与 WebSocket 联机房间。

原项目（JS/Electron, v1.5.8）保存在 `../reference/texas-holdem-game` 作为只读参考与黄金语料 oracle。

## 结构

见 `docs/RUST_REWRITE_HANDOFF.md` §3.1。crate 依赖方向严格单向：

```text
poker-core <- poker-prob / poker-engine <- poker-ai <- poker-protocol <- poker-server / poker-app <- poker-cli
```

## 开发

```bash
source ~/.cargo/env
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

## 当前阶段

Phase 0：环境、workspace 骨架、oracle 导出与黄金语料。
