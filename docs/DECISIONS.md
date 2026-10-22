# 决策记录

状态：Phase 0–3 已完成，Phase 4/5/6 并行推进中（2026-10-23）。

## 已确认（交接文档 + 用户 2026-10-23 指示）

1. 用 Rust 从零重写，不保留 Electron/WebView；UI 用 egui 原生绘制，音频 rodio，联机 tokio/axum。
2. 只重写主版本（Windows x64 桌面版，含标准德州 + 短牌两种模式、本地 AI、联机）；**不做 Android，不做 32 位 Windows**。
3. 原仓库只读，不修改；黄金语料从原 JS 导出，权威 oracle 为原 JS v1.5.8（commit e0c6c6f）。
4. 先正确后性能；AI 验收采用统计等价；规则/牌力/合法动作/底池必须精确一致。
5. **用户 2026-10-23 明确：以下 7 项待拍板问题全部按推荐默认值执行**，无需再逐项确认。
6. Windows 实测由用户在自己的 Windows x64 机器上完成（用户先去休息，先由 agent 尽可能推进 Linux 侧可验证工作）。WSL 内的性能/IME/托盘数据不作为最终验收依据。

## 已拍板结论（原待确认项，现全部按默认值生效）

| # | 问题 | 结论 |
| --- | --- | --- |
| 1 | Windows 最终验收方式 | agent 提供 Windows 构建脚本 + 冒烟清单/exe；用户在 Windows 实机跑并反馈；Linux/WSL 内做全部自动化测试 |
| 2 | 联机协议是否兼容旧 JS 客户端 | **是**：协议 v2 字段/消息/错误码保持一致，Phase 5 做 Rust↔JS 双向互通测试 |
| 3 | Windows 分发形式 | **便携 zip 优先**，安装包（可选）后补 |
| 4 | 视觉还原程度 | **高保真**（布局/信息/交互一致），不追求像素级 |
| 5 | 无障碍 | 键盘可达性必须；屏幕阅读器尽力而为 |
| 6 | 调试面板 / benchmark 工具 | 保留 poker-cli 与精简调试面板 |
| 7 | 音乐/图标资源 | 先原样复用原 assets（约 20MB），打包阶段再评估压缩 |

## 已完成里程碑

- Phase 0：Rust workspace 骨架、Cargo 镜像（rsproxy）、参考仓库固化、JS 152 项测试基线、oracle 工具链。
- Phase 1：poker-core（牌/牌堆/可注入 RNG/标准+短牌评估器），15,052 条黄金用例 + 全部 5 张组合穷举哈希与 JS 一致。
- Phase 2：poker-engine（严格移植 ServerPokerGame），240 手 / 1,707 步黄金回放逐状态一致；poker-cli 自对局 harness。
- Phase 3：poker-prob（prepare/exact/MC/calculate/findImprovements），固定 seed MC 与 JS 逐样本一致；性能基线进行中。

## 环境注意事项（2026-10-23 更新）

- `static.crates.io` 不可达；`~/.cargo/config.toml` 已切换到 rsproxy（index + 下载均可达），并设置 http timeout=30 / net retry=2 防止锁死。
- 多 agent 并发 cargo 时使用各自的 `CARGO_TARGET_DIR`；依赖已缓存时优先 `--offline`。
