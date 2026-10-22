# 项目状态（2026-10-23 交付）

> 用户已确认：所有待拍板项按推荐默认值执行；Windows 实机验收由用户完成；不做 Android / 32 位 Windows。
> 详细交付与验收证据见 **docs/FINAL_REPORT.md**。一键验证：`scripts/verify-all.sh«。

## 总览

- 9 个 crate 全部实现并有测试；`cargo fmt/clippy -D warnings/test --workspace« 全绿（最终验证已通过）。
- 黄金语料：评估/引擎/概率/AI/协议全部从原 JS v1.5.8 导出并可复现；规则、底池、协议、MC 逐样本对齐。
- 桌面应用本地标准/短牌对局可玩，已接入真实 v1.5.8 移植 AI；分析面板/键盘/音频/设置/双主题完成。
- 联机：Rust 服务端与旧 JS 客户端、Rust 客户端与旧 JS 服务端双向互通通过；联机 UI 逻辑层完成、屏幕层待续（阶段 2b）。
- Windows：交叉构建便携 zip 已产出（dist/）；原生 MSVC 构建脚本与 15 节冒烟清单已交付，实机验收待做。
- 性能：7 张评估 42.1 M/s（标准）/ 49.8 M/s（短牌），较原实现 6.55/7.40 提升 6.4/6.7 倍。

## 剩余工作（详见 FINAL_REPORT §5）

1. AI 标准模式决策对齐 88.9% → ≥95%（已定位两处根因）
2. 服务端接入真实 AI（工厂接口已就绪）
3. 联机 UI 阶段 2b（worker + 连接/大厅/房间/结算屏幕）
4. 5/6 张评估查表优化
5. Windows 实机冒烟（IME/托盘/DX12/DPI/性能）——需用户机器

## 快速命令

```bash
cargo run -p poker-app                       # 本地对局
cargo test --workspace                       # 全测试
scripts/verify-all.sh                        # 完整验收（含黄金语料复现）
cargo run -p poker-server --example serve    # 联机服务
```
