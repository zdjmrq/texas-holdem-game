# poker-app Phase 6 垂直切片状态

> 任务：task-8（Phase 6: poker-app 垂直切片）
> 更新：2026-10-23
> 结论：本地标准德州 + 短牌 6+ 对局已可完整游玩，Windows 构建脚本与冒烟清单已交付；
> 联机 UI 为占位页；AI 为可替换 trait + 确定性临时策略桩，等待 poker-ai adapter 接入。

## 1. 已实现功能

### 1.1 应用架构

| 模块 | 作用 |
| --- | --- |
| src/app.rs | 根 App：Screens（HOME/TABLE/SETTINGS/ONLINE）、键盘分发、settings 持久化、提示音/闪烁时机 |
| src/session.rs | TableSession：poker-engine 驱动、座位、AI 调度、结算/下一手/庄家轮转、筹码守恒 |
| src/ai.rs | AiPolicy trait + DeterministicAi 临时策略（poker-ai 就绪后替换，不复制其逻辑） |
| src/analysis.rs | 后台分析线程（poker-prob calculate）+ 缓存（手牌+公共牌+对手数+variant 的 key） |
| src/audio.rs | 音乐播放列表/切歌/音量/顺序·随机·单曲、提示音合成、headless 探针 |
| src/settings.rs | 设置 JSON 持久化 + 容错/夹取（配置 dir：Windows %APPDATA%、Linux ~/.config） |
| src/table_ctrl.rs | 纯逻辑：快捷键映射、主操作、加注面板网格/步进/默认值、分析遮蔽、BB 格式化 |
| src/ui/table.rs + ui/components.rs | 牌桌/座位/手牌/公共牌/底池/动作条/加注面板/结算弹窗/分析面板绘制 |
| src/fonts.rs | 内置 Noto Sans SC GB2312 子集（3,183,992 字节）+ 系统字体回退 |
| src/assets.rs | 资源定位（exe 同目录 -> CWD -> 仓库）、音乐扫描、config 目录 |
| src/platform/* | 保持 Phase 0 的 cfg(windows) 托盘/单实例；Linux stub |
| src/shell.rs | 自定义标题栏/拖动/双击最大化/8 方向 BeginResize/任务栏注意力 |
| src/screenshot.rs | 程序内截图（request_screenshot -> PNG -> 自动退出），用于证据与冒烟 |
| src/cli.rs | --screenshot/--scenario/--seed/--hide-cards/--open-raise/--frameless/--audio-* 等 |

数据流：UI 每帧从 TableSession 构造只读快照 -> ui/table 返回动作枚举 -> app 应用到 session；
分析请求发送到后台线程，UI 只 poll 缓存；音频设备惰性打开（音乐关闭时完全不初始化）。

### 1.2 对局

- 标准德州（52 张，盲注）与短牌 6+（36 张，前注 + 庄家双倍前注，同花>葫芦）均可开局。
- 本地 1 人类 + 2..=9 AI（短牌最多 8 人），设置可选人数/筹码/盲注/AI 速度/名字。
- 每手：庄家轮转、盲注/前注、翻牌/转牌/河牌、全下自动发完公共牌、边池、未跟注退款、
  摊牌、弃牌胜、结算弹窗、下一手；筹码守恒（单元测试验证）。
- 人类破产/赢下牌桌时显示对局结束并可回主页。
- 分析面板：胜率/平局/负率/期望份额、当前牌型、可击败牌型（10 种彩色 chip）、
  outs 总数/强/薄 + 前 5 个牌型分组；后台计算，不阻塞 UI；手牌隐藏时整体遮蔽。
- 手牌隐藏：点击自己的手牌或顶栏按钮切换；隐藏时分析面板与分析相关状态条同步遮蔽。
- 加注面板：合法上下限（引擎 grid）、步进=小盲/前注；滑块 + 1/←/↓ 减一个单位、
  2/→/↑ 加一个单位、Esc 取消、Space/Enter 确认；默认值=翻前 3BB、其他 minRaise+2BB（ui.md §3.8）。
- 键盘：F/C/R/A、Space/Enter 主操作、N 下一手、Esc 关闭；输入框聚焦时不触发；
  快捷键只在轮到人类时生效（ui.md §4.2）。
- 结算弹窗：赢家/金额/人类盈亏/公共牌/摊牌牌面与牌型（赢家 🏆）、下一手(N)/回主页。

### 1.3 设置 / 音频 / 窗口 / 平台

- 设置项：玩法、人数、筹码、盲注/前注、名字、AI 速度、音乐开关/音量/播放模式/提示音、
  全屏、自定义标题栏（无边框）、显示分析、默认隐藏手牌。
- 持久化 JSON：容错（缺失->默认；损坏->默认+警告）、字段夹取（人数/筹码/音量/名字长度）。
- 音乐：assets/music 下 4 首 mp3 运行时加载（不内嵌，便携 zip 一起带）；切歌/播放模式/音量；
  音乐关闭时不打开音频设备（不预加载）。提示音：轮到你单响、结算两响（合成正弦波，无额外资源）。
- 窗口：默认系统边框；设置可切自定义标题栏（运行中切换 ViewportCommand::Decorations）；
  最小 900x600；全屏切换；任务栏闪烁（RequestUserAttention，4 秒自动 Reset + 托盘图标闪红）。
- 字体：内置 GB2312 子集（assets/fonts/NotoSansSC-Regular.otf，include_bytes 进二进制）。

### 1.4 Windows 交付（第一步）

- scripts/build-windows.ps1 + build-windows.cmd：Windows 原生 release 构建 + 便携 zip + 日志。
- scripts/README.md：VS Build Tools / rustup msvc / rsproxy / 常见问题。
- docs/windows-smoke.md：Windows 冒烟清单 + 结果回收模板。
- Linux 上已验证 cargo check --target x86_64-pc-windows-msvc 通过（Phase 0 记录，Phase 6 代码保持）。

## 2. 与 docs/specs/ui.md 的对齐情况

已对齐（本切片范围）：

- 快捷键全表与优先级（加注面板 > 结算 > 牌桌动作；输入框例外；仅轮到我时生效）。
- 加注面板：网格/上下限/步进/默认值、1/2/方向键/Esc/Space 行为。
- 手牌隐藏 + 分析面板遮蔽（等效实现：内容替换为「手牌已隐藏，分析已遮蔽」）。
- 分析面板内容：胜率条、当前牌型、可击败牌型、outs 分组、计算中状态。
- 结算弹窗：赢家/金额/公共牌/摊牌牌型/下一手(N)/game over 提示。
- 窗口：1280x860 / 最小 900x600 / 系统边框默认 / 自定义标题栏 / 全屏 / DPI 由 egui 处理。

已知偏差 / 后续 TODO（不阻塞可玩性）：

1. 顶部工具栏 85% 悬停展开/滚出逻辑未实现（当前固定顶栏）；小窗口三档布局未做。
2. Tab/Shift+Tab 焦点顺序沿用 egui 默认，未实现「结算弹窗内 Tab 强制聚焦下一手按钮」。
3. 结算弹窗不支持拖动（egui Modal 固定居中）；赢家行样式简化。
4. 牌桌座位坐标按椭圆布局自绘，未逐像素对齐原 CSS；高保真、非像素级（符合 DECISIONS #4）。
5. 分析面板 beats 用彩色文字 chip（无 hover 说明）；outs 仅显示前 5 组。
6. 深色主题；原项目为浅色牌桌 + CSS 主题变量，视觉还原在 Phase 6 后续 polish。
7. 联机 UI 仅占位；poker-client（task-12）就绪后接入。
8. 音效仅轮到你/结算两种提示音；原项目的其他音效与静音细则后续补。
9. AI 为临时确定性策略桩，决策质量不作验收；poker-ai adapter 就绪后通过 AiPolicy 替换。
10. 内存/启动时间未做 Windows 实测；release 打包与性能验收在 Phase 7。
11. 加注面板金额默认值/快捷键已按 ui.md，但滑块 UI 细节（aria/焦点）未逐项对齐。

## 3. 验证与证据

### 3.1 命令

    export CARGO_TARGET_DIR=/tmp/target-app-spike
    cargo fmt -p poker-app -- --check
    cargo clippy -p poker-app --all-targets -- -D warnings
    cargo test -p poker-app
    cargo build -p poker-app

### 3.2 测试

- 单元测试 20 项：session 状态/筹码守恒/庄家轮转/短牌牌面、加注面板边界与默认值、
  快捷键映射、遮蔽逻辑、AI 桩确定性/合法性、分析 key/后台结果、音频播放列表、字体、BB 格式化。
- 集成测试 18 项：audio_decode 2、cli_options 6、fonts 4、settings_persist 6。
- 合计 38 项（详见 `cargo test -p poker-app` 输出）。

### 3.3 截图（Linux/WSLg，X11 后端，程序内截图）

| 文件 | 内容 |
| --- | --- |
| docs/spikes/phase6-home.png | 主页：玩法/人数/筹码/盲注/名字/AI 速度 + 开始按钮 |
| docs/spikes/phase6-preflop.png | 牌桌翻前 + 加注面板（--open-raise） |
| docs/spikes/phase6-flop.png | 牌桌翻牌：公共牌/底池/outs/分析面板 |
| docs/spikes/phase6-hidden.png | 手牌隐藏 + 分析遮蔽 |
| docs/spikes/phase6-settlement.png | 结算弹窗（赢家/公共牌/摊牌牌型） |
| docs/spikes/phase6-shortdeck.png | 短牌 6+ 牌桌（前注制） |
| docs/spikes/phase6-settings.png | 设置页 |

复现：

    unset WAYLAND_DISPLAY; export DISPLAY=:0
    cargo run -p poker-app -- --scenario table-flop --screenshot docs/spikes/phase6-flop.png --screenshot-frame 70

### 3.4 可玩性验证

- `--scenario settlement` 自动打完一手并展示结算（含摊牌牌型）。
- 单元测试 autoplay_ten_hands_never_panics 连续 10 手自动行动无 panic；
  full_hand_settles_and_chips_are_conserved 验证筹码守恒；next_hand_rotates_the_dealer 验证庄家轮转。
- 人工交互验证（Linux/WSLg）：开始本地对局 -> AI 行动 -> 人类动作（含加注面板）-> 下一手流程。

## 3.5 依赖说明

poker-app 当前只依赖 poker-core / poker-engine / poker-prob（外加 eframe/rodio 等第三方）。
poker-ai 与 poker-protocol **暂不作为依赖**：前者由 Phase 4 并行迁移，后者用于联机客户端；
等正式 AI adapter（AiPolicy 实现）和 poker-client 就绪后由 Lead 接线时再加回。这样 poker-app
的构建/测试/clippy 不会被并行迁移中的 crate 拖垮。

注意：工作区级 `cargo clippy --workspace --all-targets -- -D warnings` 目前会被其他 crate
（poker-engine rules.rs:121 collapsible_if、poker-protocol 同类问题、poker-ai 迁移中编译错误）
阻塞；poker-app 自身用 `cargo clippy -p poker-app --all-targets --no-deps -- -D warnings` 验证为 0 warning。

## 4. 临时 AI 桩（重要）

- 接口：crates/poker-app/src/ai.rs 的 AiPolicy trait（name + decide(&AiView) -> Action）；
  AiView 只暴露公开状态 + 自己的底牌（不泄露对手手牌）。
- 当前实现：DeterministicAi（确定性伪随机 roll + 牌力阈值；返回前做合法性兜底）。
- 替换路径：poker-ai 提供 adapter 后实现 AiPolicy（Lead 负责接线），poker-app 其余代码不动。
- poker-app 当前**不依赖 poker-ai crate**（避免与 Phase 4 并行迁移互相拖垮编译）；把 trait 对象
  换成正式 adapter 时再在 Cargo.toml 加回依赖。

## 5. 待 Lead 确认 / 下一步

1. **poker-ai 依赖 TODO（Lead 已确认）**：本轮为规避 Phase 4 并行迁移的编译问题，poker-app
   暂时移除了 poker-ai 依赖（只保留 AiPolicy trait + DeterministicAi 桩）。等 poker-ai 可编译且
   golden baseline 达标后，按 Lead 通知：`Cargo.toml` 加回 poker-ai 依赖 -> 新增
   `PokerAiAdapter`（impl AiPolicy，把 AiView 映射到 poker-ai 输入、把决策映射为 Action）-> 
   在 TableSession 创建处替换 Box::new(DeterministicAi)。接口形状请 ai 团队 review 一次。
   联机接入时同理加回 poker-protocol / poker-client 依赖。
2. UI polish 优先级：顶部工具栏悬停、浅色主题、牌桌视觉、结算弹窗拖动。
3. Windows 冒烟：把 dist/texas-holdem-rs-windows-x64.zip 交用户按 docs/windows-smoke.md 跑。
4. 是否把音乐资源（20MB）提交进 git：本轮按 assets/** 已复制 4 首；如不接受可改 LFS/外置下载。
5. poker-app 的 poker-protocol 依赖目前未使用，联机 UI 接入时启用。
## 6. task-15 收尾补充（Phase 6 polish + 联机/AI 接入点）

本轮（task-15）在垂直切片之上补齐 UI 完成度与接入准备，未扩大功能范围：

### 6.1 已完成

- **响应式布局骨架**：新增 src/layout.rs —— WindowTier::{Normal,Compact,Tiny}（<=760 / <=650 断点，ui.md §2.7）、
  85% 悬停高度计算、工具栏显隐判定、牌桌最小高度与滚动判定、座位/牌面缩放系数；table 绘制按 tier 缩放，
  小窗自动进入 ScrollArea 并保持最小牌桌高度。
- **Tab/焦点链**：新增 src/focus.rs —— 主页 order（「开始游戏」第一）、牌桌 order（顶栏开关 + 合法动作）、
  结算弹窗 focus trap order、step/wrap 纯逻辑；egui 原生 Tab 按创建顺序移动（Creation order 与 focus order
  一致），结算打开时请求初始焦点到「下一手」，加注面板打开时聚焦滑块；金色焦点环组件。
- **结算弹窗拖动 + 焦点陷阱**：Modal 支持拖动（偏移量、重开重置、边界 clamp），按钮带焦点 id；
  egui Modal 层天然把 Tab 限制在弹窗内（ui.md §3.9/§4）。
- **主题**：新增 src/theme.rs（ui.md §2.8 token）—— 深色（原版默认）/浅色两套 Palette + egui Visuals +
  字号表；设置页可切换主题；牌桌/座位/底池/文字颜色走 Palette；截图见 phase6-polish-light.png。
- **OnlineController 抽象**：src/online_controller.rs —— ConnectionState/RoomInfo/RoomConfig/OnlineRequest/
  OnlineEvent + OnlineController trait + NoopOnlineController（无联机客户端时显式失败），app 只依赖 trait；
  trait 预留 set_server_url/server_url/shutdown 供内嵌服务与 ui-analyst 的 OnlineClient 使用。
- **内嵌本地服务（Lead 决策 A）**：src/local_server.rs —— 后台线程 current_thread tokio runtime，
  spawn_server_with_fallback(POKER_PORT 未设置时用 0 临时端口)，URL 通过 channel 回传 UI，退出时
  ServerHandle::shutdown + join；联机占位页显示实际 ws:// 地址；单元测试验证 spawn/publish/shutdown。
- **AI 接入点**：src/ai_bridge.rs —— PokerAiBackend trait + BridgeInput（AiView 拥有化拷贝）+
  PokerAiBridge（后端决策非法时回退确定性桩）+ create_policy(seed, short_deck) 工厂；
  feature real-ai 默认关闭（当前 no-op 骨架，Phase 4 达标后按 docs/specs/ai-server-adapter.md 接线），
  默认构建/测试不受影响。TableSession 创建改走 create_policy。
- **依赖接通**：poker-app 加回 poker-protocol / poker-client / poker-server / tokio；dev-deps tokio(test-util)
  + tokio-tungstenite（ui-analyst mock 测试用）；workspace 可编译。
- **CLI**：新增 --window-size WxH、--theme dark|light、--toolbar-pinned、--no-server、--scenario online。

### 6.2 证据（2026-10-23）

    cargo fmt -p poker-app -- --check                  # FMT_OK
    cargo clippy -p poker-app --all-targets -- -D warnings   # 0 warning（含依赖）
    cargo test -p poker-app                            # 72 项全过
    cargo check --target x86_64-pc-windows-msvc        # OK

新增截图：docs/spikes/phase6-polish-tiny.png（900x600 小窗 + 内嵌 server 启停日志）、
phase6-polish-light.png（浅色主题牌桌）、phase6-online-placeholder.png（联机占位页 + 实际 ws:// 地址）。

### 6.3 遗留（项目收尾，记录待办）

- detached 工具栏（85% 悬停浮层）只完成判定/尺寸纯逻辑，UI 浮层尚未接（当前小窗固定顶栏 + 缩放/滚动）；
  Tab 焦点链已按 ui.md 顺序实现纯逻辑 + 初始焦点请求，但未逐控件做焦点环回归（金色环只在动作/弹窗按钮上）。
- 联机 UI 阶段 2b（OnlineClient/worker/ui/demo）由 ui-analyst 后续任务实现；本轮 app 挂 Noop，
  lib.rs 已保留 pub mod online; 与 src/online/** 纯逻辑层（12 项测试绿）。
- real-ai 仍是骨架；poker-ai golden baseline 达标后接 PokerAiBackend + optional poker-ai 依赖。
