# egui 0.37 桌面 UI 风险 Spike 报告（Phase 0 / task-3）

> 负责人：app-spike（独立工作流）
> 日期：2026-10-23
> 目标平台：Windows x64（最终验收在用户的 Windows 机器）
> 开发/自动化环境：Ubuntu 24.04 WSL2 + WSLg
> 关联任务：team task-3；交接文档 RUST_REWRITE_HANDOFF.md §6.3 / §3.3
> 代码：crates/poker-app（Phase 0 spike window）与 docs/spikes/ 下的截图

---

## 0. 结论：GO（有条件）

egui/eframe 0.37 这条技术路线在 Linux/WSLg 上已跑通全部可验证项；IME 与 Windows 壳功能
（托盘、任务栏闪烁、无边框手感、DPI、GPU/帧率）必须在 Windows 上做最后确认，但代码路径
已按支持的 API 写好并隔离在 cfg(windows) 中，没有发现需要立即改 UI 方案的阻塞点。

| 风险项 | 结论 | 已有证据 | 剩余条件 |
| --- | --- | --- | --- |
| 中文渲染 | GO | system-fallback 与 explicit 两种模式截图均无豆腐块；罕见字（龘/龖/犇）正常 | 打包 3.1MB GB2312 子集保证跨机器一致 |
| 中文 IME | 架构 GO / 需 Windows 实测 | egui 0.37 IME 管线完整（TextEdit -> IMEOutput -> egui-winit -> winit WM_IME_*），应用层零代码 | 微软拼音实测（§5.2），UI 开发前完成 |
| 无边框窗口 | GO（骨架） | 自定义标题栏 + StartDrag + 双击最大化 + 8 方向 BeginResize 已实现并截图 | Windows 手感/Snap/缩放实测（§5.3） |
| 托盘/任务栏闪烁/单实例 | 方案 GO / 需 Windows 实测 | tray-icon 0.26 + single-instance 0.3 的 cfg(windows) 实现；闪烁用 egui 安全命令，winit 内部即 FlashWindowEx | Windows 构建 + 实测（§5.4） |
| mp3 音频 | GO | 解码 44100Hz/2ch/124.15s；WSLg PulseAudio 实播 3s 位置走到 2.920s | Windows WASAPI 实测（§5.5） |
| 性能 / GPU | 待 Windows | WSLg 只有 llvmpipe 软件渲染（10-26fps），无参考价值 | Windows DX12 实机 60fps（§5.6）；glow 回退预案 |
| MSRV | 阻塞项（流程） | eframe 0.37 要求 rustc >= 1.95，rodio 0.23 要求 >= 1.87；workspace rust-version 目前 1.85 | Lead 决定 bump（我未改 workspace/Cargo.toml） |

如果在 Windows 上 IME 实测不达标：按交接文档 §6.3 停止后续 Phase 6 UI 开发，回退 Tauri+Web UI
方案需用户重新拍板；不要在 IME 未确认前把牌桌 UI 全面铺开。

---

## 1. 环境与版本

| 项 | 值 |
| --- | --- |
| OS / 内核 | Ubuntu 24.04.5 LTS (WSL2)，Linux 6.18.33.2-microsoft-standard-WSL2 |
| CPU / 内存 | Intel Core Ultra 5 125H（18 线程可见）/ 15 GiB |
| 显示 | WSLg 1.0.73.2；DISPLAY=:0 + WAYLAND_DISPLAY=wayland-0（XWayland） |
| GPU | 无 /dev/dri，只有 Mesa llvmpipe 软件渲染（Vulkan 1.3 + GL 4.5） |
| Rust | rustc 1.99.1 / cargo 1.99.1（stable） |
| eframe / egui | 0.37.0 / 0.37.0（egui-winit 0.37.0、egui-wgpu 0.37.0、epaint 0.37.0） |
| winit / wgpu | 0.30.13 / wgpu 30.0.1（Vulkan + GLES 后端；Windows 上为 DX12） |
| 字体栈 | fontique 0.11.1（eframe system_fonts 特性，Windows 走 DirectWrite） |
| rodio / symphonia | 0.23.0 / 0.5.5 |
| 图片 / 日志 | image 0.25.10、env_logger 0.11.11、log 0.4.34 |
| Windows 专用（仅 Windows target 编译） | tray-icon 0.26.1、single-instance 0.3.3 |

系统装过的依赖（可复现）：

    sudo apt install -y libxkbcommon-dev libgl1-mesa-dev libwayland-dev libx11-dev \
        libxcursor-dev libxi-dev libxrandr-dev libasound2-dev libfontconfig1-dev \
        fonts-noto-cjk libxkbcommon-x11-0 libasound2-plugins
    # python3-fonttools 仅用于字体子集体积测量

说明：期间 static.crates.io 不可达导致 cargo 下载卡住，Lead 已把共享 cargo 源切到
rsproxy.cn。本报告的所有编译/测试最终都是离线或 rsproxy 下完成的，与 spike 结论无关。

---

## 2. 交付物

### 2.1 代码（crates/poker-app）

| 文件 | 作用 |
| --- | --- |
| src/main.rs | 二进制入口；Windows release 下 windows_subsystem = "windows"（无控制台窗口） |
| src/lib.rs | 入口 run()、窗口图标、eframe NativeOptions、headless 模式分发 |
| src/cli.rs | --screenshot / --font-mode / --frameless / --audio-test / --audio-play 参数 |
| src/app.rs | spike 窗口 UI：中文、IME 监视、动画、音频、窗口控制、平台壳状态 |
| src/fonts.rs | 三种中文字体策略：system-fallback / explicit-system(TTC face index) / bundled |
| src/audio.rs | rodio 0.23 + symphonia mp3：解码探针、AudioEngine、headless 播放验证 |
| src/screenshot.rs | egui request_screenshot 回调 -> PNG 落盘 -> 自动退出 |
| src/shell.rs | 无边框标题栏、拖动、双击最大化、8 方向 BeginResize、任务栏注意力 |
| src/platform/mod.rs | 平台壳分发（cfg 选择） |
| src/platform/windows.rs | Windows 托盘图标/菜单、单实例锁、托盘图标闪红 |
| src/platform/stub.rs | 非 Windows 空实现（Linux 不编译 tray-icon/single-instance） |
| tests/audio_decode.rs | 无音频设备下的 mp3 解码回归（2 项） |
| tests/cli_options.rs | CLI 解析回归（4 项） |
| tests/fonts.rs | 字体发现/路径回归（3 项） |

### 2.2 资源（assets/）

| 文件 | 体积 | 说明 |
| --- | --- | --- |
| assets/music/01.mp3 | 3,975,095 B | 从原仓库 assets/music 原样复制，include_bytes! 内嵌 |
| assets/icons/attention-icon.png | 1,174 B | 32x32 RGBA；窗口图标 + 托盘图标共用 |

后续正式版需要复制全部 4 首音乐（约 20MB）并补多尺寸 .ico，见 §4 待拍板项。

### 2.3 截图（docs/spikes/）

| 文件 | 分辨率 | 说明 |
| --- | --- | --- |
| egui-spike-linux-system-font.png | 2560x1720 | Wayland + ppp=2.0 + system-fallback 字体，中文/牌面符号正常 |
| egui-spike-linux-explicit-font.png | 1280x860 | X11 + ppp=1.0 + 显式加载 Noto Sans CJK SC（TTC face #2），中文正常 |
| egui-spike-linux-frameless.png | 1280x860 | X11 + 无边框窗口 + 自定义标题栏（拖动/最小化/最大化/关闭按钮） |

截图由程序自己完成：egui Context::request_screenshot -> wgpu 读回像素 -> image 编码 PNG ->
自动 Close，不依赖外部截图工具，Windows 上也可用同一条命令取证。

---

## 3. 实测记录

### 3.1 编译与静态检查

命令（CARGO_TARGET_DIR 独立、CARGO_HOME 隔离以避免和其他 agent 抢包缓存锁）：

    export CARGO_TARGET_DIR=/tmp/target-app-spike
    cargo build -p poker-app
    cargo fmt -p poker-app -- --check
    cargo clippy -p poker-app --all-targets -- -D warnings
    cargo test -p poker-app

结果：

| 检查 | 结果 |
| --- | --- |
| cargo build | 通过（eframe/wgpu 依赖树全量编译；debug target 目录约 4.5GB，含 check/clippy 缓存） |
| cargo check（依赖 metadata 全量） | 1m09s（18 线程，依赖已下载，非冷启动网络时间） |
| 增量编译 | 2.7-4.8s |
| cargo fmt --check | 通过 |
| cargo clippy -D warnings | 通过，0 warning（修过 Id::new -> scope_id().with() 的 deprecated 警告） |
| cargo test | 9 项全绿：audio_decode 2、cli_options 4、fonts 3 |
| debug 二进制 | 455,711,032 B（含调试符号，不代表 release 体积） |

首轮失败过的 3 个点（已修复，供 Phase 6 参考）：

1. rodio 0.23 的 Source trait 方法（repeat_infinite 等）需要显式 use rodio::Source as _;
2. egui 0.37 的 Id::new 已 deprecated，交互 ID 用 ui.scope_id().with(...) 或 Id::unique(...)；
3. 无边框边缘交互不要用 Id::new 生成常量 ID（现在用父 Ui 的 scope_id 派生）。

### 3.2 Linux/WSLg 启动与渲染

命令：

    export DISPLAY=:0 WAYLAND_DISPLAY=wayland-0
    cargo run -p poker-app -- --screenshot docs/spikes/egui-spike-linux-system-font.png \
        --font-mode system-fallback --screenshot-frame 30

关键日志：

    egui_wgpu: There are 2 available wgpu adapters:
      {backend: Vulkan, device_type: Cpu, name: "llvmpipe (LLVM 20.1.2, 256 bits)", ...}
      {backend: Gl, device_type: Cpu, name: "llvmpipe ... 4.5 (Core Profile)", ...}
    egui_wgpu: Software rasterizer detected - loss of performance expected. backend: Vulkan
    [poker-app] first frame: pixels_per_point=2.00, content_rect=[[0.0 0.0] - [1280.0 860.0]]
    SCREENSHOT_OK docs/spikes/egui-spike-linux-system-font.png (2560x1720)

结论：

- eframe 0.37 默认 wgpu 后端在 WSLg 上能初始化（选到 Vulkan llvmpipe，无 GPU）；窗口创建、
  中文渲染、egui 上传纹理、读回截图全部成功，说明渲染链路完整。
- ppp 在 Wayland 下为 2.0（WSLg 合成器缩放），X11 下为 1.0；两种都渲染清晰，说明 egui 的
  DPI 处理可用（Windows 上 125%/150% 仍需实测）。
- 帧率：Wayland 软件渲染 31 帧/3.0s（约 10fps，2560x1720），X11 31 帧/1.2s（约 26fps，
  1280x860）。这是 llvmpipe 的 CPU 光栅化数字，不代表 Windows + DX12 的真实性能，不能作为
  性能验收依据。

WSLg 环境坑（仅开发环境，与 Windows 无关）：

- 连续启动 Wayland 窗口时遇到过一次 winit EventLoopError（Exit Failure: 1）与 arboard 剪贴板
  连接错误；改用 X11 后端（unset WAYLAND_DISPLAY + DISPLAY=:0）后三次截图全部稳定。
  自动化取证脚本建议固定用 X11 后端。
- X11 后端需要 libxkbcommon-x11-0（WSL 默认没装，否则 xkbcommon-dl panic）。
- Wayland 下无边框窗口曾以 640x480 出现（min_inner_size 未生效），X11 下正常 1280x860；
  属于 WSLg/合成器差异，Windows 需按 §5.3 复核。

### 3.3 中文字体

eframe 0.37 的一个重要变化：默认特性多了 system_fonts（fontique 后端），
NativeOptions.system_font_fallback 默认 true。系统字体按需加载：
Windows 走 DirectWrite（微软雅黑等），Linux 走 fontconfig（需要系统装了 CJK 字体）。
也就是说不在代码里写任何字体加载逻辑，中文也能显示（WSL 实测截图证明）。

spike 同时验证了三种策略（--font-mode）：

| 模式 | 实现 | 实测结果 |
| --- | --- | --- |
| system-fallback（默认） | eframe/fontique 按需加载系统字体 | 通过；中文/牌面符号/罕见字正常；WSL 日志有一条 "Failed to parse locale C" 警告（locale 未设置导致，不影响字形回退；Windows 用系统 locale） |
| explicit-system | 自己找系统 CJK 文件，用 FontData::from_blob + TTC face index 注册为最高优先级字体 | 通过；加载 /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc face #2（SC），19,484,784 字节 |
| bundled | 从 assets/fonts/NotoSansSC-Regular.otf 读取并注册 | 代码路径已实现；spike 未提交字体文件，因此当前返回 "not found" 提示。发布版推荐用这个 |

字体体积实测（Noto Sans CJK，OFL 许可）：

| 对象 | 体积 / 信息 |
| --- | --- |
| 系统 NotoSansCJK-Regular.ttc（JP/KR/SC/TC/HK 共 5 个 face） | 19,484,784 B |
| SC face 字形数 | 65,535 glyphs |
| GB2312 常用集子集（7,735 字符：ASCII + 6763 汉字 + 全角标点 + 常用符号/花色） | 3,183,992 B（保留 layout features）/ 3,093,432 B（极简） |
| 完整 Noto Sans SC Regular OTF（参考值） | 约 10MB |

子集生成命令（发布版打包时执行一次，产物提交到 assets/fonts/）：

    python3 -m fontTools.subset /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc \
        --font-number=2 --text-file=/tmp/gb2312_chars.txt \
        --output-file=assets/fonts/NotoSansSC-Regular.otf \
        --layout-features="*" --no-hinting --desubroutinize

建议：

1. 发布版内置 3.1MB 的 GB2312 子集（含 ASCII/全角标点/♠♥♣♦ 等），保证任何 Windows 机器
   渲染一致、启动不受系统字体枚举影响；同时保留 system_font_fallback=true 作为罕见字兜底。
   若要完全确定性，打包后再把 NativeOptions.system_font_fallback 设为 false（首版建议 true）。
2. 不要内置 19.5MB 的 TTC；体积/解析时间都不划算。
3. 字体加载 API（0.37 写法）：

    let data = egui::FontData::from_blob(std::sync::Arc::new(font_bytes), face_index);
    ctx.add_font(egui::FontInsert::new("poker-cjk", data, vec![
        egui::InsertFontFamily { family: egui::FontFamily::Proportional,
                                  priority: egui::FontPriority::Highest },
        egui::InsertFontFamily { family: egui::FontFamily::Monospace,
                                  priority: egui::FontPriority::Lowest },
    ]));

   注意：FontData::from_owned 的 face index 固定为 0；TTC 必须用 from_blob(arc, index)。

DPI 建议：所有布局用逻辑点（egui 默认）；不要自己乘 DPI。窗口 1280x860 / 最小 900x600 已按
交接文档设置。Windows 125%/150% 下需要看字体粗细、标题栏按钮点击区域、托盘菜单；若用户觉得
字体太大/太小，Phase 6 可加一个 zoom factor 设置（ctx.set_zoom_factor），不要用像素硬编码。

启动成本：fontique 在后台线程枚举系统字体（几百 ms 量级），首次 CJK 字形查找会等它完成；
Windows 字体多时可能更久。内置字体可以消掉这个首帧卡顿，这也是推荐内置子集的理由之一。

### 3.4 中文 IME

源码级结论（按本机实际下载的 crate 版本核对，0.37.0 / winit 0.30.13）：

1. TextEdit 获得焦点且拥有 IME 事件时，会在 PlatformOutput.ime 里填 IMEOutput
   { purpose, rect, cursor_rect, should_interrupt_composition }：
   egui-0.37.0/src/widgets/text_edit/builder.rs:1049-1072，结构定义
   egui-0.37.0/src/data/output.rs:100-114。
2. egui-winit 每帧读取该输出：有 IME 时调用 window.set_ime_allowed(true)、
   set_ime_purpose()，并在光标矩形变化时调用 set_ime_cursor_area()（用于把候选窗定位到
   光标附近）；没有 IME 时关闭。位置：egui-winit-0.37.0/src/lib.rs:1258-1310。
3. winit 0.30.13 的 Windows 后端处理 WM_IME_STARTCOMPOSITION / WM_IME_COMPOSITION
   （GCS_COMPSTR 读组合串、GCS_RESULTSTR 读上屏结果、ISC_SHOWUICOMPOSITIONWINDOW），
   并把 set_ime_cursor_area 映射到 Win32 IME 光标区域：
   winit-0.30.13/src/platform_impl/windows/event_loop.rs:1513 起、
   platform_impl/windows/ime.rs:115 / :141。
4. winit 事件被 egui-winit 转成 Event::Ime(ImeEvent::Preedit / Commit / DeleteSurrounding)，
   TextEdit 负责组合串内联显示、光标/选区、上屏插入和 DeleteSurrounding 删除：
   egui-0.37.0/src/widgets/text_edit/builder.rs:1417-1490。
5. 因此应用层不需要为 IME 写任何启用/定位代码（不需要 IMEAllowed / IMERect 命令），
   只要用普通的 egui TextEdit 即可。egui 0.37 也提供了 ViewportCommand::IMEAllowed/IMERect/
   IMEPurpose 作为手动覆盖手段（spike 未使用）。

结论：架构上 IME 链路完整，比旧版 egui 强很多；但真实微软拼音的候选窗、组合串样式、
全角标点、快捷键冲突必须 Windows 实测（清单见 §5.2）。spike 窗口内置了 IME 事件监视器
（实时显示 Event::Ime 的 Debug 输出），用户截图/录屏即可作为验收证据。

已实现的相关 UI：中文 TextEdit（预填 "德州扑克全下 All-in：..."）、最近 8 条 IME 事件显示、
清空按钮、窗口焦点状态显示。

### 3.5 动画 / 帧率

实现：ProgressBar + Spinner + 帧计数器，每帧 ctx.request_repaint_after(16ms)，保证持续重绘。
WSLg 软件渲染下动画正常推进（31 帧分别用了 3.0s/1.2s，见 §3.2），没有掉帧/卡死/
时钟回退问题。Windows 验收：牌桌动画、窗口拖动/缩放时观察是否平滑（§5.3/5.6）。

### 3.6 音频（rodio 0.23 + symphonia mp3）

依赖配置（poker-app/Cargo.toml）：

    rodio = { version = "0.23", default-features = false, features = ["playback", "mp3"] }

rodio 0.23 与原项目/旧教程代码差异很大（迁移时必须按新 API 写）：

| 用途 | rodio 0.23 写法 |
| --- | --- |
| 打开默认输出 | rodio::DeviceSinkBuilder::open_default_sink() -> MixerDeviceSink |
| 设备信息 | sink.config()（sample_rate/channel_count/sample_format/buffer_size） |
| 播放器 | rodio::Player::connect_new(sink.mixer())；append/play/pause/stop/clear/get_pos/is_paused/empty |
| 解码文件/内存 | rodio::Decoder::try_from(File) 或 Decoder::try_from(Cursor<Vec<u8>>) |
| 循环 | source.repeat_infinite()（需要 use rodio::Source） |
| 简写播放 | rodio::play(&mixer, reader) -> Player |

实测 1（无音频设备也能跑，做了自动化测试）：内置 assets/music/01.mp3 解码结果

    01.mp3: 44100 Hz / 2 ch / total 124.15s / decoded 264600 samples / peak 0.512

实测 2（WSLg 真实播放）：WSL 默认 ALSA 没有声卡（/dev/snd 不存在），直接跑会失败。用 WSLg
自带的 PulseAudio 桥接后实播成功：

    sudo apt install -y libasound2-plugins
    mkdir -p /tmp/alsa-home
    printf 'pcm.!default { type pulse }\nctl.!default { type pulse }\n' > /tmp/alsa-home/.asoundrc
    HOME=/tmp/alsa-home PULSE_SERVER=unix:/mnt/wslg/PulseServer \
        /tmp/target-app-spike/debug/poker-app --audio-play 3

输出：

    decode: OK 01.mp3: 44100 Hz / 2 ch / total 124.15s / decoded 176400 samples / peak 0.512
    device: OK 48000 Hz / 2 ch / F32 / buffer Fixed(2048)
    playback: started=true
    playback position after 3.0s wall time: 2.920s (advancing=true)

说明 cpal + rodio + PulseAudio 全链路真实播放（位置在推进）。Windows 上 cpal 会自动用 WASAPI，
这条链路本身就是跨平台验证过的；mp3 解码是平台无关的 symphonia。仍需在 Windows 上听声音确认
（§5.5），并回归 4 首音乐、切歌、音量、静音、提示音时机。

注意：spike 用 include_bytes! 把 mp3 编进二进制（4MB），发布版 4 首约 20MB，exe 体积与
打包形态（单文件 vs 外置 assets）需要 Lead/用户决定（§4）。

### 3.7 无边框窗口（frameless）

方案（全部用 egui 0.37 的 viewport API，安全代码）：

- 创建：ViewportBuilder::default().with_decorations(false).with_resizable(true)
  .with_inner_size([1280.0, 860.0]).with_min_inner_size([900.0, 600.0])。
- 自定义标题栏：egui 自绘一行，包含标题 + 拖动区 + 最小化/最大化/关闭按钮。
- 拖动：拖动区 Response::drag_started() -> ViewportCommand::StartDrag。
- 双击最大化：Response::double_clicked() -> ViewportCommand::Maximized(!maximized)，
  最大化状态从 ctx.input(|i| i.viewport().maximized) 读。
- 边缘/角落缩放：鼠标指针进入 5px 边缘条 -> 设置 CursorIcon -> 交互 drag_started 时发
  ViewportCommand::BeginResize(ResizeDirection)。8 个方向都实现了，矩形区域用
  ctx.content_rect() 计算。
- 最小化/关闭：ViewportCommand::Minimized(true) / Close。
- 运行中切换：ViewportCommand::Decorations(bool) 可以运行时切换系统边框（spike 窗口里
  有 "切换系统边框" 按钮，实测可用）。
- 任务栏注意力：ViewportCommand::RequestUserAttention(UserAttentionType::Critical /
  Informational / Reset)，详见 §3.8。

实测：X11 后端下 --frameless 截图 1280x860，自定义标题栏、按钮、中文均正常（见截图
egui-spike-linux-frameless.png）。Linux 上 BeginResize 在 Wayland 不受支持（winit 限制），
X11 支持；Windows 支持（winit 用 WM_NCLBUTTONDOWN + HT* 命中测试实现）。

Windows 需要重点实测/决策的点：

1. 手感：拖动是否跟手、双击最大化/还原、8 方向缩放、边缘 5px 判定是否合适（可能调到 6-8px）。
2. Win11 Snap Layouts：自定义最大化按钮 hover 不会出现系统 Snap 菜单（这是自绘标题栏的
   已知代价）。要完整支持需 Hook WM_NCHITTEST + 自己处理，属于 unsafe Win32，建议单独 crate
   （§3.8）。若用户不需要 Snap 菜单，可接受。
3. 最小尺寸 900x600、最大化时覆盖任务栏、多显示器/跨屏拖动、任务栏自动隐藏。
4. DPI 125%/150%/200% 下标题栏高度与按钮命中区域（按逻辑点写，正常应该自动正确）。
5. 关闭按钮/Alt+F4/托盘退出 三种退出路径都要清理：停音频、移除托盘图标、释放单实例锁。

建议：第一版默认带系统边框，设置里提供 "无边框窗口" 开关（spike 已验证运行时切换），
在 Windows 上对比后再决定是否默认无边框。

### 3.8 托盘 / 任务栏闪烁 / 单实例

依赖隔离（poker-app/Cargo.toml）：

    [target.'cfg(windows)'.dependencies]
    tray-icon = "0.26"
    single-instance = "0.3"

Linux 构建完全不编译这些代码（platform/stub.rs 顶替），也不会引入 GTK/libappindicator 等
Linux 系统依赖；Cargo.lock 里能看到它们，但 Linux target 不构建。

托盘（src/platform/windows.rs）：

- TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("德州扑克 Rust 重制版")
  .with_icon(Icon::from_rgba(32x32 PNG 解码的 RGBA))。菜单项：显示主窗口 / 退出。
- 每帧 poll：MenuEvent::receiver().try_recv()，把菜单事件转成 ShellEvent::{ShowWindow,Quit}，
  App 里 Focus/Visible/Close。
- 托盘图标闪红：PlatformShell::set_alert_icon(bool) 把图标 RGBA 红色提亮后 set_icon，
  提醒结束时换回；Phase 6 可按 500ms 交替实现闪烁。
- 注意：tray-icon 0.26 文档要求 Windows 上创建 TrayIcon 时事件循环已经在运行；eframe 的
  App::new 在 winit 事件循环启动后调用，满足该要求。Explorer 重启、DPI 变化、任务栏位置等
  需要 Windows 实测。
- 图标资源：当前只有 32x32 PNG；正式版应提供 16/24/32/48/256 多尺寸 .ico（Windows 任务栏/
  高 DPI 下清晰度）。窗口图标同理。

任务栏闪烁（不需要 windows crate）：egui 0.37 的 UserAttentionType 有 Critical/Informational/
Reset；egui-winit 把它转给 winit::Window::request_user_attention，Windows 实现为
FlashWindowEx：

    Critical       -> FLASHW_ALL | FLASHW_TIMERNOFG, uCount = u32::MAX
    Informational  -> FLASHW_TRAY | FLASHW_TIMERNOFG, uCount = 0
    Reset          -> FLASHW_STOP

（winit-0.30.13/src/platform_impl/windows/window.rs:935-957）

FLASHW_TIMERNOFG 的行为正是 "一直闪直到窗口到前台"，Reset 可以提前停止，和原
desktop-attention.js 的需求一致。spike 窗口有 "任务栏闪烁 / 请求注意" 和 "停止闪烁" 两个
按钮，Windows 上点一下即可验收。托盘闪红 + 任务栏闪烁的自动触发条件（轮到你/结算）在
Phase 6 接牌局状态后实现；同一提示不重复提醒、获得焦点后停止 由 App 状态机控制。

为什么没有直接用 windows crate 调 FlashWindowEx：workspace lints 和 crate 级
forbid(unsafe_code) 禁止 unsafe，而 windows-rs 的 FFI 全部是 unsafe。当前安全封装
（RequestUserAttention）已经覆盖任务栏闪烁需求，所以不应为了它破坏 unsafe 策略。
确实需要 raw Win32（SetOverlayIcon、ITaskbarList3 进度条、WM_NCHITTEST/Snap、JumpList）时，
建议新增独立 crate poker-win32，局部把 unsafe 策略改为 allow/warn 并集中审查（需 Lead 拍板）：

    // poker-win32/src/lib.rs（示意，不在本 crate 编译）
    #![allow(unsafe_code)]
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        FLASHWINFO, FLASHW_ALL, FLASHW_TIMERNOFG, FlashWindowEx,
    };
    pub fn flash(hwnd: HWND, count: u32) {
        let info = FLASHWINFO {
            cbSize: core::mem::size_of::<FLASHWINFO>() as u32,
            hwnd,
            dwFlags: FLASHW_ALL | FLASHW_TIMERNOFG,
            uCount: count,
            dwTimeout: 0,
        };
        unsafe { FlashWindowEx(&info); }
    }

HWND 可以通过 eframe::Frame 的 HasWindowHandle 拿到（RawWindowHandle::Win32.hwnd），不需要
额外全局变量。

单实例（src/platform/windows.rs）：

- single_instance::SingleInstance::new("texas-holdem-rs-spike")，is_single() 为 false 时
  当前实现直接退出并打印错误。
- 局限：第二个实例退出时无法把已有窗口拉到前台。正式版应加 IPC（命名管道或
  WM_COPYDATA/RegisterWindowMessage）让第二个实例通知第一个实例 ShowWindow/SetForegroundWindow；
  或者用 single-instance 的 crate 生态方案。Phase 6 补。
- 测试：双击 exe 两次、不同用户会话、托盘退出后重新启动。

### 3.9 Windows 交叉编译检查（重要）

本机是 Linux，但用 rustup 的 msvc target 做了纯类型检查（cargo check 不链接，不需要 MSVC 链接器）：

    rustup target add x86_64-pc-windows-msvc
    export CARGO_TARGET_DIR=/tmp/target-app-spike-win
    cargo check -p poker-app --target x86_64-pc-windows-msvc

结果：Finished dev profile in 1m25s，0 error 0 warning。该检查实际编译了：
wgpu DX12 后端、windows 0.62.2 / windows-sys、accesskit_windows 0.35.0、fontique 的 DirectWrite
后端、cpal WASAPI、tray-icon 0.26.1、muda 0.21.2、single-instance 0.3.3，以及我们的
src/platform/windows.rs。也就是说 Windows 专用代码（托盘、单实例、cfg 分支）API 层面已验证，
剩下的是真实 Windows 上的构建/链接与运行行为。

限制：这只是类型检查，不覆盖链接、资源、manifest（图标/版本信息）、真实 Win32 行为与性能。

---

## 4. 需要 Lead / 用户拍板的事项（附建议默认）

| # | 事项 | 建议默认 | 影响 |
| --- | --- | --- | --- |
| 1 | workspace rust-version 1.85 -> 至少 1.95 | 由 Lead 改 workspace/Cargo.toml 的 rust-version（eframe 0.37 要求 1.95，rodio 0.23 要求 1.87） | 不 bump 的话 rust-version 声明与实际依赖矛盾，CI/发布可能报错 |
| 2 | 字体打包 | 内置 3.1MB GB2312 子集 + 保留系统字体兜底 | 渲染一致性、启动速度、exe 体积 |
| 3 | 无边框默认策略 | 第一版默认系统边框，设置项可切换无边框（运行时已可切） | Win11 Snap 菜单/手感的取舍 |
| 4 | 任务栏闪烁实现路径 | 用 egui RequestUserAttention 安全封装（够用，零 unsafe） | 若要 overlay icon/进度条/JumpList 则需独立 poker-win32 crate（需放宽 unsafe 策略） |
| 5 | 托盘/窗口图标 | 需要设计提供多尺寸 .ico（16/24/32/48/256）；当前 32x32 PNG 仅够 spike | 高 DPI 清晰度 |
| 6 | 音乐打包 | 4 首约 20MB：先内嵌（单文件便携），Phase 7 再决定是否压缩/外置 | exe 体积、首次启动 |
| 7 | GPU 回退 | 第一版只验 wgpu/DX12；若用户机器/远程桌面有兼容问题，再加 eframe glow 特性 + --renderer glow | 老显卡/远程桌面兼容性 |

关于 glow 回退的具体接入方式（Phase 6/7 再做）：

    # Cargo.toml
    [features]
    glow-renderer = ["eframe/glow"]
    # 代码
    #[cfg(feature = "glow-renderer")]
    if std::env::var("POKER_RENDERER").as_deref() == Ok("glow") {
        native_options.renderer = eframe::Renderer::Glow;
    }

（eframe 0.37 默认特性是 wgpu，glow 不在默认特性里，需要显式打开。）

---

## 5. Windows 实测清单（交付用户在 Windows 机器执行）

把 crates/poker-app 单独拷到 Windows 或整仓库构建均可。建议先 debug 构建（有 console 日志），
再 release 冒烟。

### 5.1 构建

前置：Visual Studio 2022 Build Tools（使用 C++ 的桌面开发）、rustup（host
x86_64-pc-windows-msvc）、Node 不需要。

    cd texas-holdem-rs
    cargo build -p poker-app
    cargo run -p poker-app -- --help
    cargo run -p poker-app

通过标准：编译通过；窗口出现 1280x860；标题 "德州扑克 Rust 重制版 — egui Phase 0 Spike"；
窗口图标是扑克提示图标（不是 egui 默认白 e）；中文无豆腐块。

记录：Windows 版本、GPU 型号/驱动、是否 DX12、窗口截图。

### 5.2 中文 IME（最高风险，优先做）

1. 启动应用，点击 "中文输入法（IME）" 输入框。
2. 切换到微软拼音（Win+Space），输入 "nihao"，观察候选窗与组合串。
3. 观察输入框内组合串是否内联显示、是否有下划线/选区样式。
4. 选择候选词上屏，确认文字进入输入框。
5. 测试 Esc 取消组合、Backspace 删组合串、输入中途点击其他输入框、输入全角标点（，。！？）、
   中英混输、粘贴中文（Ctrl+V）。
6. 观察 spike 的 "IME 事件监视器"：应出现 Preedit/Commit 事件（egui Event::Ime 的 Debug 文本）。
7. 测试长按候选、翻页、候选窗是否被窗口边缘裁切、窗口移动时候选窗位置是否跟随。

通过标准：候选窗出现在光标附近；组合串可见且不吞键；上屏正确；无 panic/卡死；事件监视器
能看到 Preedit/Commit。若候选窗位置明显错误或无法输入中文 -> 不通过，停下来找 Lead/用户。

记录：录屏 + 事件监视器截图。

### 5.3 无边框窗口

    cargo run -p poker-app -- --frameless

1. 拖动自定义标题栏（空白区域）移动窗口，是否跟手、是否会选中文字。
2. 双击标题栏最大化/还原；点 "▢" 按钮同样行为。
3. 分别从 8 个方向（4 边 + 4 角）拖动边缘缩放，光标形状是否正确，缩放手感是否正常。
4. 缩到最小 900x600 是否被限制；最大化后是否覆盖任务栏；还原是否回到原尺寸。
5. 点 "切换系统边框" 按钮在无边框/系统边框间切换，确认不闪烁、不崩溃。
6. Win11 下尝试拖动窗口到屏幕边缘贴边分屏（Aero Snap），以及最大化按钮 hover 是否出现
   Snap Layouts（预期：不出现，需记录为用户可接受/不可接受）。
7. 125%/150% DPI 下重复 1-6。

通过标准：拖动/缩放/最大化/还原/最小尺寸全部可用；无窗口跳动；DPI 正常。

### 5.4 托盘 / 任务栏闪烁 / 单实例

1. 启动后看任务栏通知区域是否有扑克图标；鼠标悬停 tooltip 是否为 "德州扑克 Rust 重制版"。
2. 右键托盘图标：菜单 "显示主窗口"/"退出" 是否正常，中文是否乱码。
3. 点应用内 "任务栏闪烁 / 请求注意"：任务栏按钮是否闪烁；点击窗口使其到前台后是否停止；
   再点一次后点 "停止闪烁" 是否立即停止。
4. 点 "任务栏闪烁" 时观察托盘图标是否变红（托盘闪红的简化验证）。
5. 最小化窗口后再触发闪烁，确认任务栏按钮闪烁。
6. 启动第二个实例：应只允许一个（第二个直接退出）；记录是否需要 "第二次启动唤起已有窗口"
   （当前不支持，属已知限制）。
7. 用任务管理器重启 explorer.exe，观察托盘图标是否自动恢复（tray-icon 行为需实测）。
8. 退出应用：托盘图标是否消失；进程是否完全退出（任务管理器确认无残留）。

通过标准：托盘菜单可用、闪烁符合预期且能停止、单实例生效、退出无残留。

### 5.5 音频

1. 启动后点 "播放背景音乐 01.mp3"，确认能听到音乐（音量 35%）。
2. 点 "暂停"，再点播放，确认暂停/继续正常。
3. 等待 2 分钟以上确认循环播放（曲长 124s）。
4. 点 "停止" 确认停止。
5. Windows 声音设置里切换默认输出设备（如扬声器<->耳机），观察是否崩溃/是否继续播放。
6. 播放中最小化/最大化/切换 DPI，确认不卡顿。
7. 记录当前设备信息（面板 "音频设备" 一行）。

通过标准：有声音、暂停/继续/停止/循环正常、切换设备不崩溃。若无声，记录面板上的错误文本。

### 5.6 DPI / 多显示器 / GPU / 性能

1. 系统缩放 100% -> 125% -> 150% -> 200% 分别启动，检查中文清晰度、标题栏高度、按钮命中。
2. 多显示器（不同 DPI）之间拖动窗口，检查是否重新缩放、文字是否重新栅格化清晰。
3. 观察动画面板帧数/流畅度；Windows 上应是 60fps 左右（vsync），拖动窗口时不明显掉帧。
4. 记录启动时间（双击到窗口出现）和任务管理器内存（RSS/Private Bytes）。
5. 若有远程桌面/老显卡环境，测试是否能创建 wgpu DX12 上下文；失败则记录错误并准备 glow 回退。

通过标准：DPI 无模糊/错位；动画流畅；启动与内存合理；DX12 可用。

### 5.7 其他

- 关闭窗口（X 按钮/Alt+F4/托盘退出）都干净退出。
- 连续开关 10 次不崩溃、不残留进程/托盘图标。
- 日志：debug 构建时设置 RUST_LOG=info 可以看到启动日志；release 是 GUI 子系统无控制台，
  可用 cmd 重定向 "poker-app.exe > out.txt 2>&1" 收集（Windows 下有效）。
- 把结果（截图/录屏/错误文本）发回 Lead；任一 IME/无边框/托盘项不通过 -> 先不要开始 Phase 6
  的牌桌 UI 工作，等方案确认。

---

## 6. 风险清单

| # | 风险 | 等级 | 证据/说明 | 缓解 | 回退 |
| --- | --- | --- | --- | --- | --- |
| 1 | Windows IME 实际表现不达标（候选窗错位/组合串异常） | 中高 | 代码链路完整但未在 Windows 实测 | 按 §5.2 优先实测；内置 IME 事件监视器取证 | Tauri + WebView（需用户重新拍板） |
| 2 | 自绘标题栏的手感/Snap Layouts 不被接受 | 中 | Win11 Snap 菜单不会出现在自绘按钮上 | 默认系统边框 + 设置项；先给用户体验对比 | 保留系统边框，不做无边框 |
| 3 | wgpu DX12 在用户机器/远程桌面不兼容 | 中 | WSLg 只有软件 Vulkan/GL，无法证明 DX12 | Windows 实测；保留 glow 特性接入点 | eframe glow/OpenGL 渲染 |
| 4 | 系统字体 fallback 导致不同机器字体不一致 | 中 | fontique 依赖系统字体 | 内置 3.1MB GB2312 子集 | 完全关闭系统 fallback |
| 5 | egui 0.37 API 较新，网上教程/旧代码不兼容 | 中 | App::ui/panel/show/Id/screenshot API 与 0.2x-0.3x 教程不同 | 本报告 §9 记录差异；以 crate 源码为准 | 固定 0.37，不随意升级 |
| 6 | eframe 0.37 要求 rustc >= 1.95，与 workspace rust-version 1.85 冲突 | 中（流程） | cargo metadata 未报错但声明不一致 | Lead bump workspace rust-version >= 1.95 | 固定工具链 |
| 7 | 软件渲染下帧率低（仅 WSL） | 低 | llvmpipe 10-26fps | Windows 实机验收 | 无（开发环境现象） |
| 8 | 托盘图标在 Explorer 重启/DPI 变化后状态异常 | 低 | tray-icon 平台行为 | §5.4 实测；必要时监听 TaskbarCreated 重建 | 去掉托盘（不推荐） |
| 9 | 单实例只能"退出第二个"，不能唤起第一个 | 低 | 当前实现 | Phase 6 加命名管道 IPC | 接受现状 |
| 10 | unsafe_code forbid 限制 raw Win32 能力（overlay icon/Snap 等） | 低 | workspace lints | 新增独立 poker-win32 crate，局部策略 | 不做这些增强 |
| 11 | 音乐内嵌导致 exe 变大（20MB 级） | 低 | 4 首 mp3 约 20MB | Phase 7 压缩/外置/按需加载 | 外置 assets 目录 |
| 12 | WSLg 偶发 Wayland 崩溃/frameless 尺寸异常 | 低（仅开发） | 见 §3.2 | 自动化固定 X11 后端 | 无（不影响 Windows） |

---

## 7. 复现命令（Linux/WSLg，全部已验证）

    # 环境
    source ~/.cargo/env
    export CARGO_TARGET_DIR=/tmp/target-app-spike
    sudo apt install -y fonts-noto-cjk libxkbcommon-x11-0
    # 说明：cargo 源按 Lead 统一配置为 rsproxy；若离线可用加 --offline

    cd texas-holdem-rs
    # 1) 静态检查 + 测试
    cargo fmt -p poker-app -- --check
    cargo clippy -p poker-app --all-targets -- -D warnings
    cargo test -p poker-app

    # 2) Linux 截图（固定 X11 后端最稳）
    unset WAYLAND_DISPLAY; export DISPLAY=:0
    cargo run -p poker-app -- --screenshot docs/spikes/egui-spike-linux-system-font.png \
        --font-mode system-fallback --screenshot-frame 30
    cargo run -p poker-app -- --screenshot docs/spikes/egui-spike-linux-explicit-font.png \
        --font-mode explicit --screenshot-frame 30
    cargo run -p poker-app -- --screenshot docs/spikes/egui-spike-linux-frameless.png \
        --frameless --screenshot-frame 30

    # 3) 音频
    cargo run -p poker-app -- --audio-test          # 解码 + 设备信息
    # WSLg 真实播放（WSL 无 /dev/snd，用 Pulse 桥接）
    HOME=/tmp/alsa-home PULSE_SERVER=unix:/mnt/wslg/PulseServer \
        cargo run -p poker-app -- --audio-play 3

    # 4) Windows 交叉类型检查（Linux 上，不需要链接器）
    rustup target add x86_64-pc-windows-msvc
    CARGO_TARGET_DIR=/tmp/target-app-spike-win \
        cargo check -p poker-app --target x86_64-pc-windows-msvc

    # 5) 交互运行
    cargo run -p poker-app -- --frameless --auto-play

---

## 8. 代码骨架（已落地，供 Phase 6 复用）

### 8.1 eframe 启动 / 窗口

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("德州扑克 Rust 重制版")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([900.0, 600.0])
            .with_decorations(!frameless)
            .with_resizable(true)
            .with_icon(icon),
        centered: true,
        system_font_fallback: true,
        ..Default::default()
    };
    eframe::run_native("poker-app", native_options, Box::new(move |cc| {
        Ok(Box::new(app::SpikeApp::new(cc, cli, icon)))
    }))

### 8.2 App trait（egui 0.37 写法）

    impl eframe::App for SpikeApp {
        fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
            let ctx = ui.ctx().clone();
            egui::CentralPanel::default().show(ui, |ui| {
                if self.frameless { shell::custom_title_bar(ui, "标题"); }
                egui::ScrollArea::vertical().show(ui, |ui| { self.content(ui, &ctx, frame); });
            });
            if self.frameless { shell::handle_resize_edges(ui); }
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

注意 0.37 的变化：App trait 的方法是 ui(&mut self, ui, frame)（不是 update(ctx, frame)）；
CentralPanel/SidePanel 都是 show(ui, ...) 接收 &mut Ui；没有全局 screen_rect，改用
ctx.content_rect()。

### 8.3 拖动 / 最大化 / 边缘缩放

    // 标题栏拖动
    if drag_response.drag_started() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    // 双击最大化/还原
    if drag_response.double_clicked() {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    // 边缘缩放（8 方向）
    ui.scope_id().with(("poker-app-resize-edge", dir_index)); // 稳定 Id
    if response.drag_started() {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
    }

### 8.4 字体（TTC face）

    let data = egui::FontData::from_blob(std::sync::Arc::new(bytes), 2 /* SC face */);
    ctx.add_font(egui::FontInsert::new("poker-cjk", data, vec![
        egui::InsertFontFamily { family: egui::FontFamily::Proportional,
                                  priority: egui::FontPriority::Highest },
    ]));

发布版改为 include_bytes!("assets/fonts/NotoSansSC-Regular.otf") 或 rust-embed。

### 8.5 音频

    let sink = rodio::DeviceSinkBuilder::open_default_sink()
        .map_err(|e| format!("open default audio sink failed: {e}"))?;
    let player = rodio::Player::connect_new(sink.mixer());
    player.set_volume(0.35);
    let decoder = rodio::Decoder::try_from(std::io::Cursor::new(mp3_bytes.to_vec()))?;
    player.append(decoder.repeat_infinite());   // 需要 use rodio::Source

### 8.6 任务栏注意力

    // 请求（Windows: FlashWindowEx FLASHW_ALL|FLASHW_TIMERNOFG）
    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
        egui::UserAttentionType::Critical));
    // 停止（FLASHW_STOP）；窗口到前台也会自动停止
    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
        egui::UserAttentionType::Reset));

### 8.7 Windows 托盘（cfg(windows)，已通过 target check）

    let menu = Menu::new();
    let show_item = MenuItem::new("显示主窗口", true, None);
    let quit_item = MenuItem::new("退出", true, None);
    menu.append_items(&[&show_item, &quit_item])?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("德州扑克 Rust 重制版")
        .with_icon(Icon::from_rgba(rgba, 32, 32)?)
        .build()?;
    // 每帧
    while let Ok(event) = MenuEvent::receiver().try_recv() { /* 比较 event.id() */ }

---

## 9. egui/eframe 0.37 API 差异备忘（团队其他 Phase 参考）

0.37 是很新的版本，和网上大多数 0.2x-0.3x 教程不同，以下差异已在 spike 中踩过：

1. eframe::App: fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) 取代 update；
   另有 fn logic(&mut self, ctx, frame)（窗口隐藏时仍可跑逻辑）。
2. 面板：CentralPanel::default().show(ui, |ui| ...)；SidePanel/TopBottomPanel 统一成
   egui::Panel::left/right/top/bottom(id).show(ui, ...)；接收的是 &mut Ui。
3. ctx.screen_rect() 改为 ctx.content_rect()；窗口信息用 ctx.input(|i| i.viewport())。
4. Id::new 已 deprecated：全局唯一用 Id::unique，局部用 parent_id.with(salt)。
5. 截图：ViewportCommand::Screenshot(ScreenshotCallback) / ctx.request_screenshot(|img| ...)
   （回调式，不再是 Event::Screenshot）。
6. eframe 默认特性含 wgpu（不是 glow）+ system_fonts（fontique 系统字体回退）+ wayland/x11；
   glow 需显式开特性。
7. FontData::from_owned 只有 face 0；TTC 用 FontData::from_blob(bytes, face_index)。
8. TextEdit 的 IME 已完整接入（IMEOutput -> egui-winit -> winit），一般不需要手动 IMEAllowed。
9. UserAttentionType 新增 Reset，可主动停止闪烁。
10. ViewportBuilder::with_decorations(false) + BeginResize 是无边框窗口的标准做法；
    Wayland 不支持 BeginResize（Windows/X11 支持）。

---

## 10. 下一步（Phase 6 之前）

1. Lead 把 §5 Windows 冒烟 exe/构建脚本交给用户，在 Windows 上按 §5 清单实测（优先 IME）。
2. Lead 决定 §4 的 7 个事项（尤其 rust-version、字体打包、无边框默认策略）。
3. 根据 Windows 结果：
   - 全绿 -> egui 方案正式定案，Phase 6 按 docs/specs/ui.md 开发牌桌 UI；
   - IME/无边框不达标 -> 回来重新评估 UI 方案（Tauri/WebView 或其它），不要硬扛。
4. Phase 6 实现时直接复用本 spike 的 src/{app,fonts,audio,shell,platform} 结构与
   cfg(windows) 隔离方式；把 spike 的调试面板代码删掉/收进 cfg(debug_assertions)。

（报告完）