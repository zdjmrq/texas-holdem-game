//! The Phase 0 spike window.
//!
//! This is deliberately a single, loud window: every risky egui capability the Windows port
//! depends on gets a visible section (fonts, IME events, animation, audio, window controls,
//! platform shell) so a Windows tester can confirm or reject the stack in under a minute.

use std::time::Duration;

use eframe::egui;

use crate::audio::{self, AudioEngine, DecodeProbe};
use crate::cli::CliOptions;
use crate::fonts::{self, FontInstallReport};
use crate::platform::{PlatformShell, ShellEvent};
use crate::screenshot::ScreenshotState;
use crate::shell;

/// Runtime state of the spike application.
pub struct SpikeApp {
    cli: CliOptions,
    icon: Option<egui::IconData>,
    font_report: FontInstallReport,
    audio_probe: Result<DecodeProbe, String>,
    audio: Option<AudioEngine>,
    audio_error: Option<String>,
    audio_playing: bool,
    input_text: String,
    ime_events: Vec<String>,
    platform: PlatformShell,
    screenshot: Option<ScreenshotState>,
    frame_index: u64,
    first_frame_time: Option<f64>,
    attention_until: Option<f64>,
    status: String,
}

impl SpikeApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        cli: CliOptions,
        icon: Option<egui::IconData>,
    ) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let font_report = fonts::install(&cc.egui_ctx, cli.font_mode);
        println!("[poker-app] {}", font_report.summary());
        println!(
            "[poker-app] desktop env: DISPLAY={:?} WAYLAND_DISPLAY={:?}",
            std::env::var("DISPLAY").ok(),
            std::env::var("WAYLAND_DISPLAY").ok()
        );

        let audio_probe = audio::decode_probe(3.0);
        match &audio_probe {
            Ok(probe) => println!("[poker-app] mp3 decode probe: OK {}", probe.summary()),
            Err(error) => eprintln!("[poker-app] mp3 decode probe: FAILED {error}"),
        }

        let (audio, audio_error) = match AudioEngine::open() {
            Ok(engine) => {
                println!("[poker-app] audio device: {}", engine.device_summary());
                (Some(engine), None)
            }
            Err(error) => {
                eprintln!("[poker-app] audio device unavailable: {error}");
                (None, Some(error))
            }
        };

        let platform = PlatformShell::init(icon.as_ref());
        println!("[poker-app] platform: {}", platform.description());

        let screenshot = cli
            .screenshot
            .clone()
            .map(|path| ScreenshotState::new(path, cli.screenshot_frame));

        let mut app = Self {
            cli,
            icon,
            font_report,
            audio_probe,
            audio,
            audio_error,
            audio_playing: false,
            input_text: "德州扑克全下 All-in：♠A ♥K ♣Q ♦J".to_owned(),
            ime_events: Vec::new(),
            platform,
            screenshot,
            frame_index: 0,
            first_frame_time: None,
            attention_until: None,
            status: "启动完成".to_owned(),
        };
        if app.cli.auto_play {
            app.start_music();
        }
        app
    }

    fn content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, frame: &mut eframe::Frame) {
        ui.add_space(4.0);
        ui.heading("德州扑克 Rust 重制版 — egui Phase 0 风险 Spike");
        ui.label("验证目标：中文字体渲染 · 中文 IME · 60fps 动画 · mp3 播放 · 无边框/托盘/任务栏闪烁方案");
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(
                    "牌面符号 ♠ ♥ ♣ ♦ ｜ 动作 弃牌 过牌 跟注 加注 全下 ｜ 模式 标准德州 / 短牌",
                )
                .size(15.0),
            );
        });
        ui.separator();

        self.runtime_section(ui, ctx);
        ui.separator();
        self.font_section(ui);
        self.ime_section(ui, ctx);
        ui.separator();
        self.animation_section(ui, ctx);
        ui.separator();
        self.audio_section(ui);
        ui.separator();
        self.window_section(ui, ctx, frame);
        ui.separator();
        self.platform_section(ui, ctx);
        ui.separator();

        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(format!("状态：{}", self.status)).weak());
            ui.label(
                egui::RichText::new("｜ 详细结论与 Windows 待测清单：docs/spikes/egui-spike.md")
                    .weak(),
            );
        });
        ui.add_space(6.0);
    }

    fn runtime_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("运行环境");
        let content = ctx.content_rect();
        let native_ppp = ctx.input(|input| input.viewport().native_pixels_per_point);
        let maximized = ctx.input(|input| input.viewport().maximized);
        let focused = ctx.input(|input| input.viewport().focused);
        let elapsed = self
            .first_frame_time
            .map(|start| ctx.input(|input| input.time) - start)
            .unwrap_or(0.0);
        egui::Grid::new("spike_runtime_grid")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                ui.label("窗口内容尺寸");
                ui.label(format!(
                    "{:.0} x {:.0} pt",
                    content.width(),
                    content.height()
                ));
                ui.end_row();
                ui.label("pixels_per_point");
                ui.label(format!("{:.2}", ctx.pixels_per_point()));
                ui.end_row();
                ui.label("native_pixels_per_point");
                ui.label(format!("{native_ppp:?}"));
                ui.end_row();
                ui.label("maximized / focused");
                ui.label(format!("{maximized:?} / {focused:?}"));
                ui.end_row();
                ui.label("帧计数 / 运行时间");
                ui.label(format!("{} / {elapsed:.1}s", self.frame_index));
                ui.end_row();
                ui.label("窗口边框");
                ui.label(if self.cli.frameless {
                    "无边框（自定义标题栏 + BeginResize）"
                } else {
                    "系统边框（可通过下方按钮切换）"
                });
                ui.end_row();
            });
    }

    fn font_section(&mut self, ui: &mut egui::Ui) {
        ui.heading("中文字体");
        ui.label(format!(
            "字体方案：{}（mode={}）",
            self.font_report.mode.label(),
            self.font_report.installed
        ));
        ui.label(&self.font_report.description);
        if let Some(path) = &self.font_report.path {
            ui.label(format!(
                "字体文件：{}（{} 字节）",
                path.display(),
                self.font_report.font_bytes
            ));
        }
        ui.label(
            "字形检查：德州扑克 · 皇家同花顺 · 底池 · 边池 · 全下 · 庄家 · 盲注 · 汉字 龘 龖 犇",
        );
        ui.label("若以上出现方块/豆腐块，说明所选字体策略在该机器上不可用。");
    }

    fn ime_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("中文输入法（IME）");
        ui.label(
            "在 Windows 上切换到微软拼音，点击输入框输入中文；下方会显示 egui 收到的 IME 事件。",
        );
        ui.add(
            egui::TextEdit::singleline(&mut self.input_text)
                .desired_width(f32::INFINITY)
                .hint_text("点击这里输入中文…"),
        );
        ui.horizontal(|ui| {
            let focused = ctx.input(|input| input.viewport().focused);
            ui.label(format!("窗口焦点：{focused:?}"));
            if ui.button("清空 IME 事件").clicked() {
                self.ime_events.clear();
            }
        });
        if self.ime_events.is_empty() {
            ui.label("（尚未捕获 IME 事件；Linux/WSLg 没有系统输入法，需要在 Windows 上实测）");
        } else {
            for event in self.ime_events.iter().rev().take(6) {
                ui.monospace(event);
            }
        }
    }

    fn animation_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("动画 / 帧率");
        let time = ctx.input(|input| input.time);
        let progress = (0.5 + 0.5 * (time * 1.2).sin()) as f32;
        ui.add(
            egui::ProgressBar::new(progress)
                .show_percentage()
                .animate(true),
        );
        ui.horizontal(|ui| {
            ui.spinner();
            let elapsed = self
                .first_frame_time
                .map(|start| time - start)
                .unwrap_or(0.0);
            ui.label(format!(
                "动画运行中：第 {} 帧，已运行 {:.1}s（每帧 request_repaint_after(16ms)）",
                self.frame_index, elapsed
            ));
        });
        ui.label(
            "Windows 验收时应观察：进度条是否平滑、窗口拖动/缩放时是否掉帧、启动后是否持续重绘。",
        );
    }

    fn audio_section(&mut self, ui: &mut egui::Ui) {
        ui.heading("音频（rodio 0.23 + symphonia mp3）");
        match &self.audio_probe {
            Ok(probe) => {
                ui.label(format!("内置 01.mp3 解码探针：{}", probe.summary()));
            }
            Err(error) => {
                ui.colored_label(egui::Color32::RED, format!("内置 mp3 解码失败：{error}"));
            }
        }
        match (&self.audio, &self.audio_error) {
            (Some(engine), _) => {
                ui.label(format!("音频设备：{}", engine.device_summary()));
            }
            (None, Some(error)) => {
                ui.colored_label(egui::Color32::RED, format!("音频设备不可用：{error}"));
            }
            (None, None) => {}
        }
        ui.horizontal(|ui| {
            let label = if self.audio_playing {
                "暂停背景音乐 01.mp3"
            } else {
                "播放背景音乐 01.mp3"
            };
            if ui.button(label).clicked() {
                self.toggle_music();
            }
            if ui.button("停止").clicked() {
                if let Some(engine) = self.audio.as_mut() {
                    engine.stop();
                }
                self.audio_playing = false;
                self.status = "背景音乐已停止".to_owned();
            }
            ui.label(if self.audio_playing {
                "状态：播放中（循环）"
            } else {
                "状态：未播放"
            });
        });
    }

    fn window_section(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        _frame: &mut eframe::Frame,
    ) {
        ui.heading("窗口 / 无边框 / 任务栏提醒");
        ui.horizontal_wrapped(|ui| {
            if ui.button("最小化").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            if ui.button("最大化 / 还原").clicked() {
                let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            if ui.button("切换系统边框").clicked() {
                self.cli.frameless = !self.cli.frameless;
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(!self.cli.frameless));
                self.status = if self.cli.frameless {
                    "已切换为无边框窗口（自定义标题栏生效）".to_owned()
                } else {
                    "已切换为系统边框窗口".to_owned()
                };
            }
            if ui.button("任务栏闪烁 / 请求注意").clicked() {
                self.trigger_attention(ctx);
            }
            if ui.button("停止闪烁").clicked() {
                shell::clear_attention(ctx);
                self.attention_until = None;
                self.platform.set_alert_icon(false);
                self.status = "已清除任务栏闪烁请求".to_owned();
            }
            if ui.button("关闭窗口").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.label(
            "Windows 实现：egui ViewportCommand::RequestUserAttention(Critical/Reset) -> winit ->              FlashWindowEx(FLASHW_ALL | FLASHW_TIMERNOFG)；回到前台自动停止，Reset 可提前停止。",
        );
        if let Some(until) = self.attention_until {
            let remaining = (until - ctx.input(|input| input.time)).max(0.0);
            ui.label(format!("闪烁请求剩余 {remaining:.1}s（到时自动 Reset）"));
        }
    }

    fn platform_section(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        ui.heading("平台壳 / 托盘");
        ui.label(self.platform.description());
        let icon_state = if self.icon.is_some() {
            "窗口图标：已从内嵌 32x32 PNG 加载"
        } else {
            "窗口图标：加载失败（回退 egui 默认图标）"
        };
        ui.label(icon_state);
        ui.label("托盘图标（tray-icon 0.26）与单实例锁（single-instance 0.3）只在 cfg(windows) 下编译；Linux 构建不包含这些代码。");
        if self.screenshot.is_some() {
            ui.label("截图模式：渲染稳定后将自动保存 PNG 并退出。");
        }
    }

    fn trigger_attention(&mut self, ctx: &egui::Context) {
        shell::request_attention(ctx, true);
        let now = ctx.input(|input| input.time);
        self.attention_until = Some(now + 4.0);
        self.platform.set_alert_icon(true);
        self.status =
            "已请求任务栏/窗口闪烁（4 秒后自动停止；Windows 上由 winit 调用 FlashWindowEx）"
                .to_owned();
    }

    fn start_music(&mut self) {
        let Some(engine) = self.audio.as_mut() else {
            self.status = "无法播放：音频设备不可用".to_owned();
            return;
        };
        match engine.toggle() {
            Ok(active) => {
                self.audio_playing = active;
                self.status = format!("背景音乐已启动：{active}");
            }
            Err(error) => {
                self.audio_playing = false;
                self.status = format!("背景音乐启动失败：{error}");
            }
        }
    }

    fn toggle_music(&mut self) {
        let Some(engine) = self.audio.as_mut() else {
            self.status = "无法播放：音频设备不可用".to_owned();
            return;
        };
        match engine.toggle() {
            Ok(active) => {
                self.audio_playing = active;
                self.status = format!("背景音乐播放状态：{active}");
            }
            Err(error) => {
                self.audio_playing = false;
                self.status = format!("背景音乐切换失败：{error}");
            }
        }
    }

    fn process_ime_events(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|input| input.events.clone());
        for event in events {
            if matches!(event, egui::Event::Ime(_)) {
                self.ime_events.push(format!("{event:?}"));
                while self.ime_events.len() > 8 {
                    self.ime_events.remove(0);
                }
            }
        }
    }

    fn handle_shell_events(&mut self, ctx: &egui::Context) {
        let events = self.platform.poll_events();
        for event in events {
            match event {
                ShellEvent::ShowWindow => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.status = "托盘：显示主窗口".to_owned();
                }
                ShellEvent::Quit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn handle_attention_timeout(&mut self, ctx: &egui::Context) {
        let Some(until) = self.attention_until else {
            return;
        };
        if ctx.input(|input| input.time) >= until {
            shell::clear_attention(ctx);
            self.attention_until = None;
            self.platform.set_alert_icon(false);
            self.status = "闪烁已自动停止（请求注意 Reset）".to_owned();
        }
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(screenshot) = self.screenshot.as_mut() else {
            return;
        };
        if screenshot.is_done() {
            match screenshot.take_result() {
                Some(Ok(())) => {
                    self.status = "截图已保存，正在退出".to_owned();
                }
                Some(Err(error)) => {
                    eprintln!("[poker-app] screenshot failed: {error}");
                    self.status = format!("截图失败：{error}");
                }
                None => {}
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            screenshot.request_if_due(ctx, self.frame_index);
            ctx.request_repaint();
        }
    }
}

impl eframe::App for SpikeApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame_index = self.frame_index.saturating_add(1);
        let now = ctx.input(|input| input.time);
        self.first_frame_time.get_or_insert(now);

        self.process_ime_events(&ctx);
        self.handle_shell_events(&ctx);
        self.handle_attention_timeout(&ctx);
        self.handle_screenshot(&ctx);

        egui::CentralPanel::default().show(ui, |ui| {
            if self.cli.frameless {
                let action = shell::custom_title_bar(ui, "♠ 德州扑克 Rust 重制版 — egui Spike");
                if action.close_requested {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if action.minimize_requested {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                if action.attention_requested {
                    self.trigger_attention(&ctx);
                }
                ui.separator();
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.content(ui, &ctx, frame);
                });
        });

        if self.cli.frameless {
            shell::handle_resize_edges(ui);
        }

        if self.frame_index == 1 {
            println!(
                "[poker-app] first frame: pixels_per_point={:.2}, content_rect={:?}",
                ctx.pixels_per_point(),
                ctx.content_rect()
            );
        }

        ctx.request_repaint_after(Duration::from_millis(16));
    }
}
