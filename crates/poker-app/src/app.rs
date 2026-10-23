//! Root application: screen routing, settings, audio, analysis, table session
//! and the egui UI for every screen (home / table / settings / online).

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};
use poker_engine::{Action, ActionType, Phase};

use crate::ai_bridge;
use crate::analysis::{AnalysisService, AnalysisSummary, analysis_key};
use crate::assets;
use crate::audio::AudioManager;
use crate::cli::{CliOptions, Scenario};
use crate::fonts;
use crate::layout::WindowTier;
use crate::local_server::{LocalServerHandle, ServerInfo};
use crate::online::native::OnlineClient;
use crate::platform::{PlatformShell, ShellEvent};
use crate::screenshot::ScreenshotState;
use crate::session::{TableConfig, TableSession, advance_ai_steps, autoplay_hand};
use crate::settings::{AI_DELAYS_MS, PlayMode, Settings, VariantSetting};
use crate::table_ctrl::{self, AnalysisMask, RaisePanel, TableCommand};
use crate::theme::Theme;
use crate::ui::table::{self, SeatView, TableSnapshot, TableUiAction};

/// Top-level screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Table,
    Settings,
    Online,
}

/// The application.
pub struct PokerApp {
    cli: CliOptions,
    settings: Settings,
    last_saved_settings: Settings,
    screen: Screen,
    home_online: bool,
    session: Option<TableSession>,
    analysis: AnalysisService,
    current_analysis: Option<Arc<AnalysisSummary>>,
    analysis_key_value: Option<u64>,
    audio: AudioManager,
    platform: PlatformShell,
    font_report: fonts::FontReport,
    screenshot: Option<ScreenshotState>,
    raise_panel: RaisePanel,
    frame_index: u64,
    last_prompt_turn: Option<(u64, u64)>,
    last_settled_hand: Option<u64>,
    attention_until: Option<f64>,
    hint: Option<String>,
    open_raise_requested: bool,
    /// Logical window tier (normal / compact / tiny) computed each frame.
    window_tier: WindowTier,
    /// Drag offset of the settlement dialog (ui.md 3.9: reset on reopen).
    settlement_offset: egui::Vec2,
    /// Request keyboard focus on the settlement dialog once it opens.
    settlement_focus_pending: bool,
    /// Request keyboard focus on the raise slider once it opens.
    raise_focus_pending: bool,
    /// Native online screens and their background network worker.
    online: OnlineClient,
    /// Embedded local poker server (Lead decision A).
    local_server: Option<LocalServerHandle>,
    server_info: Option<ServerInfo>,
    server_error: Option<String>,
}

impl PokerApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        cli: CliOptions,
        mut settings: Settings,
        warning: Option<String>,
        icon: Option<egui::IconData>,
    ) -> Self {
        let _ = &icon;
        if let Some(theme) = cli.theme {
            settings.theme = theme;
        }
        settings.theme.apply(&cc.egui_ctx);
        println!("[poker-app] theme: {}", settings.theme.label());
        let font_report = fonts::install(&cc.egui_ctx);
        println!("[poker-app] {}", font_report.summary());
        println!("[poker-app] music tracks: {}", assets::music_tracks().len());

        if cli.frameless {
            settings.custom_titlebar = true;
        }
        let platform = PlatformShell::init(icon.as_ref());
        println!("[poker-app] platform: {}", platform.description());
        let screenshot = cli
            .screenshot
            .clone()
            .map(|path| ScreenshotState::new(path, cli.screenshot_frame));
        let audio = AudioManager::new(cli.no_audio);
        let open_raise_requested = cli.open_raise;
        let online = OnlineClient::new(cc.egui_ctx.clone());
        let local_server = if cli.no_server {
            None
        } else {
            Some(LocalServerHandle::start())
        };
        let last_saved_settings = settings.clone();
        let mut app = Self {
            cli,
            settings,
            last_saved_settings,
            screen: Screen::Home,
            home_online: false,
            session: None,
            analysis: AnalysisService::new(),
            current_analysis: None,
            analysis_key_value: None,
            audio,
            platform,
            font_report,
            screenshot,
            raise_panel: RaisePanel::default(),
            frame_index: 0,
            last_prompt_turn: None,
            last_settled_hand: None,
            attention_until: None,
            hint: warning,
            open_raise_requested,
            window_tier: WindowTier::Normal,
            settlement_offset: egui::Vec2::ZERO,
            settlement_focus_pending: false,
            raise_focus_pending: false,
            online,
            local_server,
            server_info: None,
            server_error: None,
        };
        if app.cli.low_memory {
            app.analysis.set_cache_limit(4);
            app.online.set_low_memory();
            cc.egui_ctx
                .all_styles_mut(|style| style.animation_time = 0.0);
        }
        app.sync_audio_settings();
        app.setup_scenario();
        if app.cli.hide_cards {
            app.settings.hide_hole_cards = true;
        }
        app
    }

    fn setup_scenario(&mut self) {
        let Some(scenario) = self.cli.scenario else {
            return;
        };
        match scenario {
            Scenario::Home => {
                self.screen = Screen::Home;
            }
            Scenario::Settings => {
                self.screen = Screen::Settings;
            }
            Scenario::Online => {
                self.screen = Screen::Online;
            }
            Scenario::Table
            | Scenario::TableFlop
            | Scenario::TableShortDeck
            | Scenario::Settlement => {
                if scenario == Scenario::TableShortDeck {
                    self.settings.variant = VariantSetting::ShortDeck;
                    self.settings.players = self.settings.players.min(8);
                }
                self.start_session_fast();
                match scenario {
                    Scenario::Table | Scenario::TableShortDeck => {
                        if let Some(session) = &mut self.session {
                            advance_ai_steps(session, 200);
                        }
                    }
                    Scenario::TableFlop => self.autoplay_to_flop(),
                    Scenario::Settlement => {
                        if let Some(session) = &mut self.session {
                            autoplay_hand(session);
                        }
                    }
                    _ => {}
                }
                self.screen = Screen::Table;
            }
        }
        println!("[poker-app] scenario: {}", scenario.label());
    }

    fn start_session_fast(&mut self) {
        let seed = self.cli.scenario_seed();
        let config = TableConfig::fast(&self.settings, seed.clone());
        let human_name = self.settings.player_name.clone();
        let ai =
            ai_bridge::create_policy(&seed, config.variant.is_short_deck(), config.starting_stack);
        let mut session = TableSession::new(&config, &human_name, ai);
        if !session.start() {
            self.hint = Some("无法开始对局：玩家数量不足".to_owned());
        }
        self.session = Some(session);
        self.raise_panel.close();
        self.analysis_key_value = None;
        self.current_analysis = None;
        self.last_prompt_turn = None;
        self.last_settled_hand = None;
    }

    fn autoplay_to_flop(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        for _ in 0..400 {
            if session.last_settlement().is_some() || session.game().community_cards().len() >= 3 {
                break;
            }
            if session.is_human_turn() {
                let legal = session.legal_actions();
                let Some(action) = table_ctrl::primary_action(&legal) else {
                    break;
                };
                if session.human_act(action).is_err() {
                    break;
                }
            } else if !session.advance_ai_immediately() {
                break;
            }
        }
    }

    fn start_local_game(&mut self) {
        self.settings.player_name = crate::settings::sanitize_name(&self.settings.player_name);
        self.settings = self.settings.clone().sanitized();
        let config = TableConfig::from_settings(&self.settings, self.cli.seed.clone());
        let ai_seed = self
            .cli
            .seed
            .clone()
            .unwrap_or_else(poker_core::RuntimeRng::fresh_seed);
        let human_name = self.settings.player_name.clone();
        let ai = ai_bridge::create_policy(
            &ai_seed,
            config.variant.is_short_deck(),
            config.starting_stack,
        );
        let mut session = TableSession::new(&config, &human_name, ai);
        let started = session.start();
        self.session = Some(session);
        self.screen = Screen::Table;
        self.raise_panel.close();
        self.analysis_key_value = None;
        self.current_analysis = None;
        self.last_prompt_turn = None;
        self.last_settled_hand = None;
        self.hint = Some(if started {
            format!(
                "本地对局开始：{}，{} 人，{}",
                self.settings.variant.label(),
                self.settings.players,
                self.settings.stakes_label()
            )
        } else {
            "无法开始对局：玩家人数不足".to_owned()
        });
    }

    fn back_home(&mut self) {
        self.screen = Screen::Home;
        self.session = None;
        self.raise_panel.close();
        self.analysis_key_value = None;
        self.current_analysis = None;
        self.last_prompt_turn = None;
        self.last_settled_hand = None;
        self.platform.set_alert_icon(false);
        // Ask the per-frame attention handler to clear any pending flash.
        self.attention_until = Some(0.0);
    }

    fn sync_audio_settings(&mut self) {
        self.audio.set_volume(self.settings.music_volume);
        self.audio.set_mode(self.settings.play_mode);
        self.audio.set_sound_enabled(self.settings.sound_enabled);
        self.audio.set_enabled(self.settings.music_enabled);
        if let Some(error) = self.audio.last_error().map(ToOwned::to_owned) {
            self.hint = Some(format!("音频不可用：{error}"));
        }
    }

    fn toggle_music(&mut self) {
        self.audio.toggle_music();
        self.settings.music_enabled = self.audio.is_enabled();
        if let Some(error) = self.audio.last_error().map(ToOwned::to_owned) {
            self.hint = Some(format!("音频不可用：{error}"));
        }
    }

    fn title_bar(&mut self, ui: &mut egui::Ui) {
        let action = crate::shell::custom_title_bar(ui, "♠ 德州扑克 Rust 重制版");
        let ctx = ui.ctx().clone();
        if action.close_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if action.minimize_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if action.attention_requested {
            crate::shell::request_attention(&ctx, true);
            self.attention_until = Some(ctx.input(|input| input.time) + 4.0);
            self.platform.set_alert_icon(true);
            self.hint = Some("已请求任务栏闪烁（4 秒后自动停止）".to_owned());
        }
    }

    // ---------------------------------------------------------------- screens

    fn home_ui(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    match crate::ui::original::home(ui, &mut self.settings, &mut self.home_online) {
                        3 => self.screen = Screen::Online,
                        1 => self.start_local_game(),
                        2 => self.screen = Screen::Settings,
                        _ => {}
                    }
                });
            });
    }

    fn close_settings(&mut self) {
        self.screen = if self.session.is_some() {
            Screen::Table
        } else {
            Screen::Home
        };
        if let Some(session) = &mut self.session {
            session.set_ai_delay(self.settings.ai_delay());
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("settings_top").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(if self.session.is_some() {
                        "← 返回牌桌"
                    } else {
                        "← 返回主页"
                    })
                    .clicked()
                {
                    self.close_settings();
                }
                ui.heading("设置");
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.settings_content(ui);
            });
        });
    }

    fn settings_content(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("玩家昵称");
            ui.add(egui::TextEdit::singleline(&mut self.settings.player_name).desired_width(180.0));
        });
        ui.label("AI 推演：深度（本地和联机均已启用）");
        ui.add_space(6.0);
        ui.heading("音频");
        let music_changed = ui
            .checkbox(&mut self.settings.music_enabled, "启用背景音乐")
            .changed();
        if ui
            .checkbox(
                &mut self.settings.sound_enabled,
                "启用提示音（轮到你 / 结算）",
            )
            .changed()
        {
            self.audio.set_sound_enabled(self.settings.sound_enabled);
        }
        ui.horizontal(|ui| {
            if ui.button("上一首").clicked() {
                self.audio.previous_track();
            }
            if ui.button("下一首").clicked() {
                self.audio.next_track();
            }
            ui.label(format!(
                "当前：{}（{} 首）",
                self.audio.current_title(),
                self.audio.track_count()
            ));
            if self.audio.is_playing() {
                ui.label(RichText::new("播放中").weak());
            }
        });
        if ui
            .add(egui::Slider::new(&mut self.settings.music_volume, 0.0..=1.0).text("音量"))
            .changed()
        {
            self.audio.set_volume(self.settings.music_volume);
        }
        egui::ComboBox::from_id_salt("settings_play_mode")
            .selected_text(self.settings.play_mode.label())
            .show_ui(ui, |ui| {
                for mode in PlayMode::ALL {
                    ui.selectable_value(&mut self.settings.play_mode, mode, mode.label());
                }
            });
        if let Some(error) = self.audio.last_error().map(ToOwned::to_owned) {
            ui.colored_label(
                Color32::from_rgb(230, 120, 120),
                format!("音频错误：{error}"),
            );
        }
        if music_changed {
            self.audio.set_enabled(self.settings.music_enabled);
            self.settings.music_enabled = self.audio.is_enabled();
        }
        self.audio.set_mode(self.settings.play_mode);

        ui.separator();
        ui.heading("显示与窗口");
        let mut fullscreen = self.settings.fullscreen;
        if ui.checkbox(&mut fullscreen, "全屏").changed() {
            self.settings.fullscreen = fullscreen;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Fullscreen(fullscreen));
        }
        let mut custom_titlebar = self.settings.custom_titlebar;
        if ui
            .checkbox(&mut custom_titlebar, "自定义标题栏（无边框窗口）")
            .changed()
        {
            self.settings.custom_titlebar = custom_titlebar;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Decorations(!custom_titlebar));
        }
        ui.checkbox(&mut self.settings.show_analysis, "显示分析面板");
        ui.checkbox(&mut self.settings.hide_hole_cards, "默认隐藏手牌");
        ui.horizontal(|ui| {
            ui.label("主题");
            for theme in Theme::ALL {
                if ui
                    .selectable_label(self.settings.theme == theme, theme.label())
                    .clicked()
                {
                    self.settings.theme = theme;
                    theme.apply(ui.ctx());
                }
            }
        });

        ui.separator();
        ui.heading("对局");
        ui.horizontal(|ui| {
            ui.label("AI 速度");
            egui::ComboBox::from_id_salt("settings_ai_delay")
                .selected_text(self.settings.ai_delay_label())
                .show_ui(ui, |ui| {
                    for delay in AI_DELAYS_MS {
                        ui.selectable_value(
                            &mut self.settings.ai_delay_ms,
                            delay,
                            format!("{delay} ms"),
                        );
                    }
                });
        });
        ui.label(format!(
            "当前：{}，{} 人，{}，{}",
            self.settings.variant.label(),
            self.settings.players,
            self.settings.starting_stack,
            self.settings.stakes_label()
        ));

        ui.separator();
        ui.heading("字体与存档");
        ui.label(self.font_report.summary());
        ui.label(format!("设置文件：{}", assets::settings_path().display()));
        ui.horizontal(|ui| {
            if ui.button("立即保存设置").clicked() {
                self.save_settings_now();
            }
            if ui.button("恢复默认设置").clicked() {
                self.settings = Settings::default();
                self.settings.theme.apply(ui.ctx());
                self.sync_audio_settings();
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                self.hint = Some("已恢复默认设置".to_owned());
            }
        });
        if let Some(hint) = &self.hint {
            ui.colored_label(Color32::from_rgb(240, 190, 90), hint);
        }
    }

    fn online_ui(&mut self, ui: &mut egui::Ui) {
        if self.online.show(ui, &self.settings) {
            self.screen = Screen::Home;
        }
    }

    fn table_ui(&mut self, ui: &mut egui::Ui) {
        if self.session.is_none() {
            self.screen = Screen::Home;
            return;
        }
        let snapshot = self.build_snapshot();
        let mut pending = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                crate::ui::original::background(ui, ui.max_rect());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let margin = ((ui.available_width() - 1180.0) / 2.0).clamp(10.0, 120.0);
                        egui::Frame::new()
                            .inner_margin(egui::Margin {
                                left: margin as i8,
                                right: margin as i8,
                                top: 10,
                                bottom: 10,
                            })
                            .show(ui, |ui| {
                                if !self.window_tier.toolbar_detached() || self.cli.toolbar_pinned {
                                    if let Some(a) = table::top_bar(ui, &snapshot) {
                                        pending = Some(a);
                                    }
                                } else {
                                    ui.label(&snapshot.event_line);
                                    if ui.ctx().input(|i| {
                                        i.pointer.hover_pos().is_some_and(|p| p.y < 80.0)
                                    }) {
                                        egui::Area::new(egui::Id::unique("detached-table-header"))
                                            .order(egui::Order::Foreground)
                                            .fixed_pos(egui::pos2(10.0, 0.0))
                                            .show(ui.ctx(), |ui| {
                                                ui.set_width(
                                                    ui.ctx().content_rect().width() - 20.0,
                                                );
                                                egui::Frame::new()
                                                    .fill(snapshot.theme.palette().panel_bg)
                                                    .inner_margin(8)
                                                    .show(ui, |ui| {
                                                        if let Some(a) =
                                                            table::top_bar(ui, &snapshot)
                                                        {
                                                            pending = Some(a);
                                                        }
                                                    });
                                            });
                                    }
                                }
                                ui.add_space(10.0);
                                let height = if self.window_tier == WindowTier::Normal {
                                    520.0
                                } else {
                                    480.0
                                };
                                ui.allocate_ui_with_layout(
                                    egui::vec2(ui.available_width(), height),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        table::show(ui, &snapshot);
                                    },
                                );
                                ui.add_space(12.0);
                                egui::Frame::new()
                                    .fill(self.settings.theme.palette().panel_bg)
                                    .corner_radius(14)
                                    .inner_margin(12)
                                    .show(ui, |ui| {
                                        if let Some(a) = table::bottom_panel(
                                            ui,
                                            &snapshot,
                                            &mut self.raise_panel,
                                            self.raise_focus_pending,
                                        ) {
                                            pending = Some(a);
                                        }
                                    });
                            });
                    });
            });
        self.raise_focus_pending = false;
        let modal = table::settlement_modal(
            ui.ctx(),
            &snapshot,
            &mut self.settlement_offset,
            self.settlement_focus_pending,
        );
        self.settlement_focus_pending = false;
        if let Some(a) = modal.action {
            pending = Some(a);
        }
        if let Some(a) = pending {
            self.apply_table_action(a);
        }
    }

    // ------------------------------------------------------------- app logic

    fn build_snapshot(&self) -> TableSnapshot {
        let session = self.session.as_ref().expect("session");
        let game = session.game();
        let settlement = session.last_settlement().cloned();
        let winners: Vec<String> = settlement
            .as_ref()
            .map(|settlement| {
                settlement
                    .result
                    .winner_amounts
                    .iter()
                    .map(|(id, _)| id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let showdown_by_seat: std::collections::HashMap<usize, Vec<poker_core::Card>> = settlement
            .as_ref()
            .map(|settlement| {
                settlement
                    .showdown
                    .iter()
                    .map(|entry| (entry.seat, entry.cards.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let seats = game
            .players()
            .iter()
            .enumerate()
            .map(|(seat, player)| {
                let showdown_cards = showdown_by_seat.get(&seat).cloned().unwrap_or_default();
                let revealed = !showdown_cards.is_empty();
                let is_human = seat == session.human_seat();
                let cards = if is_human {
                    player.hole_cards.clone()
                } else if revealed {
                    showdown_cards
                } else {
                    player.hole_cards.clone()
                };
                let cards_visible =
                    (is_human && !self.settings.hide_hole_cards) || (!is_human && revealed);
                SeatView {
                    seat,
                    name: player.name.clone(),
                    stack: player.stack,
                    is_human: seat == session.human_seat(),
                    folded: player.folded,
                    all_in: player.is_all_in,
                    is_dealer: game.dealer_pos() == seat as i32,
                    is_small_blind: game.sb_idx() == seat as i32,
                    is_big_blind: game.bb_idx() == seat as i32,
                    is_actor: game.current_player_idx() == seat as i32,
                    is_winner: winners.contains(&player.id),
                    last_action: player
                        .last_action
                        .map(|last| format!("{} {}", label_for_action(last.action), last.amount)),
                    cards,
                    cards_visible,
                    bet: player.round_bet,
                }
            })
            .collect();
        let current_actor_name = session
            .current_actor()
            .and_then(|seat| game.players().get(seat))
            .map(|player| player.name.clone());
        let mask =
            table_ctrl::analysis_mask(self.settings.hide_hole_cards, self.settings.show_analysis);
        let hand_name = if mask == AnalysisMask::Visible {
            session.human_hand().map(|hand| hand.name())
        } else {
            None
        };
        let event_line = self
            .hint
            .clone()
            .or_else(|| session.events().last().cloned())
            .unwrap_or_default();
        TableSnapshot {
            seats,
            community: game.community_cards().to_vec(),
            pot: game.pot(),
            phase: phase_label(game.phase()),
            hand_id: game.hand_id(),
            big_blind: session.big_blind(),
            variant_label: self.settings.variant.label().to_owned(),
            variant: self.settings.variant(),
            stakes_label: self.settings.stakes_label(),
            human_turn: session.is_human_turn(),
            current_actor_name,
            legal: session.legal_actions(),
            settlement,
            analysis: self.current_analysis.clone(),
            analysis_pending: self.analysis.pending_key().is_some(),
            analysis_mask: mask,
            analysis_error: self.analysis.last_error().map(ToOwned::to_owned),
            hand_name,
            hand_rank_index: if mask == AnalysisMask::Visible {
                session.human_hand().map(|hand| hand.rank_index())
            } else {
                None
            },
            game_over: session.game_over(),
            ai_name: session.ai_name().to_owned(),
            hide_cards: self.settings.hide_hole_cards,
            music_enabled: self.audio.is_enabled(),
            show_analysis: self.settings.show_analysis,
            event_line,
            tier: self.window_tier,
            theme: self.settings.theme,
        }
    }

    fn apply_table_action(&mut self, action: TableUiAction) {
        match action {
            TableUiAction::BackHome => self.back_home(),
            TableUiAction::OpenSettings => self.screen = Screen::Settings,
            TableUiAction::Fold => self.human_action(Action::Fold),
            TableUiAction::Check => self.human_action(Action::Check),
            TableUiAction::Call => self.human_action(Action::Call),
            TableUiAction::AllIn => self.human_action(Action::AllIn),
            TableUiAction::OpenRaise => self.open_raise_panel(),
            TableUiAction::CloseRaise => self.raise_panel.close(),
            TableUiAction::ConfirmRaise => {
                if self.raise_panel.open && self.is_human_turn() {
                    let action = self.raise_panel.action();
                    self.raise_panel.close();
                    self.human_action(action);
                }
            }
            TableUiAction::StepRaise(units) => {
                if self.raise_panel.open {
                    self.raise_panel.step(units);
                }
            }
            TableUiAction::NextHand => self.next_hand_or_home(),
            TableUiAction::ToggleHideCards => {
                self.settings.hide_hole_cards = !self.settings.hide_hole_cards;
            }
            TableUiAction::ToggleAnalysis => {
                self.settings.show_analysis = !self.settings.show_analysis;
            }
            TableUiAction::ToggleMusic => self.toggle_music(),
        }
    }

    fn human_action(&mut self, action: Action) {
        let Some(session) = &mut self.session else {
            return;
        };
        match session.human_act(action) {
            Ok(()) => {
                self.raise_panel.close();
                self.analysis_key_value = None;
                self.current_analysis = None;
                self.hint = None;
            }
            Err(error) => {
                self.hint = Some(format!("操作失败：{error}"));
            }
        }
    }

    fn is_human_turn(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(TableSession::is_human_turn)
    }

    fn open_raise_panel(&mut self) {
        let Some(session) = &self.session else {
            return;
        };
        if !session.is_human_turn() {
            return;
        }
        let legal = session.legal_actions();
        if !legal.contains(ActionType::Raise) {
            return;
        }
        let big_blind = session.big_blind();
        let default = table_ctrl::default_raise_amount(&legal, session.game().phase(), big_blind);
        let mut panel = RaisePanel::open_for(&legal);
        panel.set_amount(default);
        self.raise_panel = panel;
        // ui.md 4.5: opening the raise panel moves focus to the amount slider.
        self.raise_focus_pending = true;
    }

    fn next_hand_or_home(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        // N is a global table shortcut: ignore it while a hand is still live.
        if session.last_settlement().is_none() {
            return;
        }
        if session.game_over().is_some() {
            self.back_home();
            return;
        }
        if !session.next_hand() {
            self.hint = Some("无法开始下一手，请检查牌桌状态".to_owned());
            return;
        }
        self.raise_panel.close();
        self.analysis_key_value = None;
        self.current_analysis = None;
        self.hint = None;
    }

    fn apply_primary(&mut self) {
        match self.screen {
            Screen::Home => self.start_local_game(),
            Screen::Settings => {}
            Screen::Online => {}
            Screen::Table => {
                if self.raise_panel.open {
                    let action = self.raise_panel.action();
                    self.raise_panel.close();
                    self.human_action(action);
                    return;
                }
                if self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.last_settlement().is_some())
                {
                    self.next_hand_or_home();
                    return;
                }
                if let Some(session) = &self.session
                    && session.is_human_turn()
                {
                    let legal = session.legal_actions();
                    if let Some(action) = table_ctrl::primary_action(&legal) {
                        self.human_action(action);
                    }
                }
            }
        }
    }

    fn pressed_table_command(ctx: &egui::Context) -> Option<TableCommand> {
        if ctx.text_edit_focused() {
            return None;
        }
        let keys = [
            egui::Key::F,
            egui::Key::C,
            egui::Key::R,
            egui::Key::A,
            egui::Key::N,
            egui::Key::Escape,
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::ArrowUp,
            egui::Key::ArrowDown,
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
        ];
        for key in keys {
            if ctx.input(|input| input.key_pressed(key)) {
                return table_ctrl::shortcut_for(key, false);
            }
        }
        None
    }

    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        if self.screen == Screen::Online {
            self.online.handle_keys(ctx);
            return;
        }
        if ctx.text_edit_focused() {
            return;
        }
        if let Some(command) = Self::pressed_table_command(ctx) {
            self.apply_command(command);
        }
        if ctx.input(|input| {
            input.key_pressed(egui::Key::Space) || input.key_pressed(egui::Key::Enter)
        }) {
            self.apply_primary();
        }
    }

    fn apply_command(&mut self, command: TableCommand) {
        if self.screen != Screen::Table {
            if matches!(command, TableCommand::DismissOverlay | TableCommand::Back) {
                if self.screen == Screen::Settings {
                    self.close_settings();
                } else {
                    self.screen = Screen::Home;
                }
            }
            return;
        }
        match command {
            TableCommand::Fold => {
                if self.is_human_turn() {
                    self.human_action(Action::Fold);
                }
            }
            TableCommand::Check | TableCommand::Call => {
                if self.is_human_turn() {
                    let legal = self
                        .session
                        .as_ref()
                        .map(TableSession::legal_actions)
                        .unwrap_or_else(empty_legal);
                    if let Some(action) = table_ctrl::primary_action(&legal) {
                        self.human_action(action);
                    }
                }
            }
            TableCommand::AllIn => {
                if self.is_human_turn() {
                    self.human_action(Action::AllIn);
                }
            }
            TableCommand::OpenRaise => self.open_raise_panel(),
            TableCommand::CloseRaise => self.raise_panel.close(),
            TableCommand::ConfirmRaise => {
                if self.raise_panel.open && self.is_human_turn() {
                    let action = self.raise_panel.action();
                    self.raise_panel.close();
                    self.human_action(action);
                }
            }
            TableCommand::RaiseStep(units) => {
                if self.raise_panel.open {
                    self.raise_panel.step(units);
                }
            }
            TableCommand::NextHand => self.next_hand_or_home(),
            TableCommand::Back => self.back_home(),
            TableCommand::ToggleHideCards => {
                self.settings.hide_hole_cards = !self.settings.hide_hole_cards;
            }
            TableCommand::ToggleMusic => self.toggle_music(),
            TableCommand::ToggleAnalysis => {
                self.settings.show_analysis = !self.settings.show_analysis;
            }
            TableCommand::DismissOverlay => {
                if self.raise_panel.open {
                    self.raise_panel.close();
                } else if self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.last_settlement().is_some())
                    && self
                        .session
                        .as_ref()
                        .is_some_and(|session| session.game_over().is_some())
                {
                    self.back_home();
                }
            }
        }
    }

    fn tick_session(&mut self) {
        if self.screen != Screen::Table {
            return;
        }
        if let Some(session) = &mut self.session {
            session.tick(Instant::now());
        }
    }

    fn maybe_request_analysis(&mut self) {
        if self.screen != Screen::Table {
            return;
        }
        let Some(session) = &self.session else {
            return;
        };
        if !self.settings.show_analysis || self.settings.hide_hole_cards {
            self.current_analysis = None;
            self.analysis_key_value = None;
            return;
        }
        if !session.game().phase().is_active() || session.human().hole_cards.len() != 2 {
            return;
        }
        let hole = session.human().hole_cards.clone();
        let board = session.game().community_cards().to_vec();
        let opponents = session.live_opponents();
        let variant = session.game().config().variant;
        let key = analysis_key(&hole, &board, opponents, variant);
        if self.analysis_key_value != Some(key) {
            self.analysis_key_value = Some(key);
            self.analysis
                .request(key, &hole, &board, opponents, variant);
        }
        self.current_analysis = self.analysis.get(key);
    }

    fn prompt_effects(&mut self, ctx: &egui::Context) {
        let Some(session) = &self.session else {
            return;
        };
        let human_turn = session.is_human_turn();
        let turn_key = (session.game().hand_id(), session.game().turn_id());
        let settled_hand = session
            .last_settlement()
            .map(|settlement| settlement.hand_id);
        if human_turn && self.last_prompt_turn != Some(turn_key) {
            self.last_prompt_turn = Some(turn_key);
            self.audio.play_turn_prompt();
            let focused = ctx.input(|input| input.viewport().focused.unwrap_or(false));
            if !focused {
                crate::shell::request_attention(ctx, true);
                self.attention_until = Some(ctx.input(|input| input.time) + 4.0);
                self.platform.set_alert_icon(true);
            }
        }
        if let Some(hand) = settled_hand
            && self.last_settled_hand != Some(hand)
        {
            self.last_settled_hand = Some(hand);
            self.audio.play_settlement_prompt();
            // ui.md 3.9: the dialog is re-centred and gets keyboard focus on open.
            self.settlement_offset = egui::Vec2::ZERO;
            self.settlement_focus_pending = true;
        }
    }

    /// Drain the embedded local server startup result (URL arrives async).
    fn poll_embedded_server(&mut self) {
        let mut results = Vec::new();
        if let Some(server) = &self.local_server {
            while let Some(result) = server.poll() {
                results.push(result);
            }
        }
        for result in results {
            match result {
                Ok(info) => {
                    println!("[poker-app] embedded server: {}", info.url);
                    self.online.set_server_url(info.url.clone());
                    self.server_info = Some(info);
                    self.server_error = None;
                }
                Err(error) => {
                    eprintln!("[poker-app] {error}");
                    self.server_error = Some(error);
                }
            }
        }
    }

    /// Drive the online controller and fold its events into the app hint.
    fn poll_online_events(&mut self, ctx: &egui::Context) {
        for effect in self.online.poll() {
            match effect {
                crate::online::view::OnlineEffect::Hint(message) => self.hint = Some(message),
                crate::online::view::OnlineEffect::PlayTurnPrompt => self.audio.play_turn_prompt(),
                crate::online::view::OnlineEffect::PlaySettlementPrompt => {
                    self.audio.play_settlement_prompt()
                }
                crate::online::view::OnlineEffect::RequestAttention => {
                    if !ctx.input(|i| i.viewport().focused.unwrap_or(false)) {
                        crate::shell::request_attention(ctx, true);
                        self.attention_until = Some(ctx.input(|i| i.time) + 4.0);
                        self.platform.set_alert_icon(true);
                    }
                }
            }
        }
    }

    fn handle_platform_events(&mut self, ctx: &egui::Context) {
        for event in self.platform.poll_events() {
            match event {
                ShellEvent::ShowWindow => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
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
            crate::shell::clear_attention(ctx);
            self.attention_until = None;
            self.platform.set_alert_icon(false);
        }
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(screenshot) = self.screenshot.as_mut() else {
            return;
        };
        if screenshot.is_done() {
            match screenshot.take_result() {
                Some(Ok(())) => println!("[poker-app] screenshot saved, exiting"),
                Some(Err(error)) => eprintln!("[poker-app] screenshot failed: {error}"),
                None => {}
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            screenshot.request_if_due(ctx, self.frame_index);
            ctx.request_repaint();
        }
    }

    fn save_settings_now(&mut self) {
        self.settings.player_name = crate::settings::sanitize_name(&self.settings.player_name);
        self.settings = self.settings.clone().sanitized();
        match self.settings.save() {
            Ok(()) => {
                self.last_saved_settings = self.settings.clone();
                self.hint = Some("设置已保存".to_owned());
            }
            Err(error) => self.hint = Some(format!("设置保存失败：{error}")),
        }
    }

    fn persist_if_changed(&mut self) {
        if self.cli.scenario.is_some() || self.cli.screenshot.is_some() {
            return;
        }
        if self.settings != self.last_saved_settings && self.settings.save().is_ok() {
            self.last_saved_settings = self.settings.clone();
        }
    }
}

impl eframe::App for PokerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frame_index = self.frame_index.saturating_add(1);
        self.window_tier = WindowTier::from_height(ctx.content_rect().height());
        self.poll_embedded_server();
        self.poll_online_events(&ctx);
        self.analysis.poll();
        self.audio.tick();
        self.handle_screenshot(&ctx);
        self.handle_platform_events(&ctx);
        self.handle_keyboard(&ctx);
        self.tick_session();
        if self.open_raise_requested && self.screen == Screen::Table && self.is_human_turn() {
            self.open_raise_panel();
            self.open_raise_requested = false;
        }
        self.maybe_request_analysis();
        self.prompt_effects(&ctx);

        if self.settings.custom_titlebar {
            self.title_bar(ui);
        }
        match self.screen {
            Screen::Home => self.home_ui(ui),
            Screen::Table => self.table_ui(ui),
            Screen::Settings => self.settings_ui(ui),
            Screen::Online => self.online_ui(ui),
        }
        if self.settings.custom_titlebar {
            crate::shell::handle_resize_edges(ui);
        }
        self.handle_attention_timeout(&ctx);
        self.persist_if_changed();
        ctx.request_repaint_after(Duration::from_millis(if self.cli.low_memory {
            100
        } else {
            16
        }));
    }

    fn on_exit(&mut self) {
        // Deterministic screenshot/smoke runs must not pollute the user's
        // persisted settings (e.g. --hide-cards) for later runs.
        if self.cli.scenario.is_none() && self.cli.screenshot.is_none() {
            let _ = self.settings.save();
        }
        self.online.shutdown();
        if let Some(mut server) = self.local_server.take() {
            server.shutdown();
        }
    }
}

fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Idle => "空闲",
        Phase::Preflop => "翻牌前",
        Phase::Flop => "翻牌",
        Phase::Turn => "转牌",
        Phase::River => "河牌",
        Phase::Showdown => "摊牌",
    }
}

fn empty_legal() -> poker_engine::LegalActions {
    poker_engine::LegalActions {
        actions: Vec::new(),
        to_call: 0,
        can_raise: false,
        min_raise_to: 0,
        max_raise_to: 0,
        wager_unit: 1,
    }
}

fn label_for_action(action: poker_engine::ActionLabel) -> &'static str {
    match action {
        poker_engine::ActionLabel::Fold => "弃牌",
        poker_engine::ActionLabel::Check => "过牌",
        poker_engine::ActionLabel::Call => "跟注",
        poker_engine::ActionLabel::Raise => "加注",
        poker_engine::ActionLabel::AllIn => "全下",
        poker_engine::ActionLabel::SmallBlind => "小盲",
        poker_engine::ActionLabel::BigBlind => "大盲",
        poker_engine::ActionLabel::Ante => "前注",
        poker_engine::ActionLabel::DealerAnte => "庄家前注",
    }
}
