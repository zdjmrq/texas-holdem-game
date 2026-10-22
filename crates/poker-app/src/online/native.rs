//! Native connection, lobby, table and settlement screens.
use super::{
    commands::{OnlineAction, OnlineCommand},
    mapping,
    view::{ConnStatus, OnlineEffect, OnlineView},
    worker::Worker,
};
use crate::{
    layout::WindowTier,
    settings::Settings,
    table_ctrl::RaisePanel,
    ui::table::{self, SeatView, TableSnapshot, TableUiAction},
};
use eframe::egui::{self, RichText, Ui};
use std::time::Instant;

pub struct OnlineClient {
    pub view: OnlineView,
    worker: Worker,
    pub url: String,
    room_code: String,
    raise: RaisePanel,
    analysis: crate::analysis::AnalysisService,
    hidden: Option<bool>,
}
impl OnlineClient {
    pub fn new(ctx: egui::Context) -> Self {
        let session = super::session_store::load();
        let url = session
            .as_ref()
            .map_or_else(|| "ws://127.0.0.1:3000".into(), |s| s.server_url.clone());
        let mut view = OnlineView::new(Some(url.clone()));
        view.session = session;
        Self {
            view,
            worker: Worker::new(ctx),
            url,
            room_code: "1".into(),
            raise: RaisePanel::default(),
            analysis: crate::analysis::AnalysisService::new(),
            hidden: None,
        }
    }
    pub fn set_server_url(&mut self, url: String) {
        if self.view.session.is_none() {
            self.url = url.clone();
        }
        self.view.local_server_url = Some(url.clone());
        self.view.set_server_url(url);
    }
    pub fn shutdown(&mut self) {
        self.worker.send(OnlineCommand::Shutdown);
    }
    pub fn poll(&mut self) -> Vec<OnlineEffect> {
        let mut effects = Vec::new();
        for _ in 0..256 {
            let Ok(event) = self.worker.rx.try_recv() else {
                break;
            };
            let old = self.view.session.clone();
            effects.extend(self.view.reduce(event, Instant::now()));
            if self.view.session != old {
                super::session_store::save(self.view.session.as_ref());
                if let Some(s) = self.view.session.clone() {
                    self.worker.send(OnlineCommand::SetSession(s));
                } else {
                    self.worker.send(OnlineCommand::ClearSession);
                }
            }
        }
        if self
            .view
            .pending
            .as_ref()
            .is_some_and(|p| p.sent_at.elapsed().as_secs() > 8)
        {
            self.view.pending = None;
            self.view.error_line = Some("请求超时，请重试或重新连接".into());
        }
        effects
    }
    pub fn send(&mut self, cmd: OnlineCommand) {
        self.view.error_line = None;
        if matches!(
            cmd,
            OnlineCommand::LeaveRoom { .. } | OnlineCommand::Disconnect
        ) {
            let local = self.view.local_server_url.clone();
            let status = self.view.status.clone();
            self.view = OnlineView::new(Some(self.url.clone()));
            self.view.local_server_url = local;
            if !matches!(cmd, OnlineCommand::Disconnect) {
                self.view.status = status;
            }
            self.raise.close();
            super::session_store::save(None);
        }
        self.worker.send(cmd);
    }
    pub fn handle_keys(&mut self, ctx: &egui::Context) {
        if ctx.text_edit_focused() {
            return;
        }
        if self.view.settlement_open {
            if ctx.input(|i| {
                i.key_pressed(egui::Key::N)
                    || i.key_pressed(egui::Key::Space)
                    || i.key_pressed(egui::Key::Enter)
            }) && self.view.pending.is_none()
                && let Some(code) = self.view.room_code()
            {
                self.send(OnlineCommand::ReadyNext {
                    room_code: code.into(),
                    hand_id: self.view.hand_id(),
                });
            }
            return;
        }
        if !self.view.can_act() {
            return;
        }
        if self.raise.open {
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.raise.close();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Num1) || i.key_pressed(egui::Key::ArrowLeft))
            {
                self.raise.step(-1);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Num2) || i.key_pressed(egui::Key::ArrowRight))
            {
                self.raise.step(1);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space)) {
                self.act(OnlineAction::RaiseTo(self.raise.amount));
                self.raise.close();
            }
            return;
        }
        for (key, action) in [
            (egui::Key::F, OnlineAction::Fold),
            (egui::Key::A, OnlineAction::AllIn),
            (
                egui::Key::C,
                if self.view.to_call() > 0 {
                    OnlineAction::Call
                } else {
                    OnlineAction::Check
                },
            ),
            (
                egui::Key::Space,
                if self.view.to_call() > 0 {
                    OnlineAction::Call
                } else {
                    OnlineAction::Check
                },
            ),
        ] {
            if ctx.input(|i| i.key_pressed(key)) {
                self.act(action);
                return;
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::R))
            && let Some(g) = &self.view.game
        {
            let legal = mapping::legal_from_wire(&g.legal_actions, &g.room_config);
            if legal.can_raise {
                self.raise = RaisePanel::open_for(&legal);
            }
        }
    }
    pub fn show(&mut self, ui: &mut Ui, settings: &Settings) -> bool {
        self.analysis.poll();
        if let Some(g) = &self.view.game {
            let cards = mapping::cards_from_dtos(&g.your_cards);
            let board = mapping::cards_from_dtos(&g.community_cards);
            if cards.len() == 2 {
                let variant = mapping::variant(g.is_short_deck);
                let opponents = g
                    .players
                    .iter()
                    .filter(|p| {
                        !p.folded && Some(p.id.as_str()) != self.view.my_player_id.as_deref()
                    })
                    .count()
                    .max(1);
                let key = crate::analysis::analysis_key(&cards, &board, opponents, variant);
                self.analysis
                    .request(key, &cards, &board, opponents, variant);
            }
        }
        let mut home = false;
        egui::Panel::top("online_header").show(ui, |ui| {
            crate::ui::original::header(ui, self.view.hand_id().max(0) as u64, "联网游戏");
            ui.horizontal(|ui| {
                if ui.button("← 主页").clicked() {
                    self.send(OnlineCommand::Disconnect);
                    home = true;
                }
                ui.label(self.view.status.label());
                if let Some(code) = self.view.room_code() {
                    ui.label(format!("房间 {code}"));
                }
                if ui.button("断开连接").clicked() {
                    self.send(OnlineCommand::Disconnect);
                }
            });
            if let Some(error) = &self.view.error_line {
                ui.colored_label(crate::theme::DARK.theme_red, error);
            }
            if let Some(banner) = &self.view.banner {
                ui.label(banner);
            }
            ui.label(&self.view.status_line);
        });
        if let Some(snapshot) = self.snapshot(settings) {
            let mut action = None;
            egui::Panel::bottom("online_actions").show(ui, |ui| {
                action = table::bottom_panel(ui, &snapshot, &mut self.raise, false);
                if ui.button("离开房间").clicked()
                    && let Some(code) = self.view.room_code()
                {
                    self.send(OnlineCommand::LeaveRoom {
                        room_code: code.into(),
                    });
                }
            });
            egui::CentralPanel::default().show(ui, |ui| {
                if let Some(a) = table::show(ui, &snapshot) {
                    action = Some(a);
                }
            });
            if let Some(a) = action {
                match a {
                    TableUiAction::ToggleHideCards => {
                        self.hidden = Some(!self.hidden.unwrap_or(settings.hide_hole_cards));
                    }
                    TableUiAction::OpenRaise => self.raise = RaisePanel::open_for(&snapshot.legal),
                    TableUiAction::CloseRaise => self.raise.close(),
                    TableUiAction::StepRaise(n) => self.raise.step(n),
                    TableUiAction::Fold => self.act(OnlineAction::Fold),
                    TableUiAction::Check => self.act(OnlineAction::Check),
                    TableUiAction::Call => self.act(OnlineAction::Call),
                    TableUiAction::AllIn => self.act(OnlineAction::AllIn),
                    TableUiAction::ConfirmRaise => {
                        self.act(OnlineAction::RaiseTo(self.raise.amount));
                        self.raise.close();
                    }
                    _ => {}
                }
            }
            if self.view.settlement_open
                && let Some(result) = self.view.settlement()
            {
                egui::Window::new("本手结算")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                    .show(ui.ctx(), |ui| {
                        ui.heading(format!("底池 $ {}", result.pot));
                        for row in result.rows {
                            ui.horizontal(|ui| {
                                ui.label(format!(
                                    "{}   +{}   {}",
                                    row.name, row.amount, row.hand_name
                                ));
                                for card in row.cards {
                                    crate::ui::components::mini_card_widget(ui, card);
                                }
                            });
                        }
                        let label = self.view.next_hand_label();
                        let ready = self.view.ready.as_ref().is_some_and(|r| {
                            r.players.iter().any(|p| {
                                p.ready && self.view.my_player_id.as_deref() == Some(&p.id)
                            })
                        });
                        if ui
                            .add_enabled(
                                !ready && self.view.pending.is_none(),
                                egui::Button::new(label),
                            )
                            .clicked()
                            && let Some(code) = self.view.room_code()
                        {
                            self.send(OnlineCommand::ReadyNext {
                                room_code: code.into(),
                                hand_id: self.view.hand_id(),
                            });
                        }
                    });
            }
        } else {
            egui::CentralPanel::default().show(ui, |ui| {
                crate::ui::original::background(ui, ui.max_rect());
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.heading(RichText::new("好友牌桌").color(crate::theme::DARK.gold));
                    ui.add_space(16.0);
                    egui::Frame::new()
                        .fill(crate::theme::DARK.window_bg)
                        .corner_radius(14)
                        .inner_margin(24)
                        .show(ui, |ui| {
                            ui.set_max_width(500.0);
                            if !self.view.status.is_connected() {
                                ui.label("服务器地址");
                                ui.text_edit_singleline(&mut self.url);
                                if let Some(url) = &self.view.local_server_url {
                                    ui.label(format!("本机服务：{url}"));
                                }
                                let connecting = matches!(
                                    self.view.status,
                                    ConnStatus::Connecting | ConnStatus::Reconnecting { .. }
                                );
                                if ui
                                    .add_enabled(!connecting, egui::Button::new("连接服务器"))
                                    .clicked()
                                {
                                    self.view.status = ConnStatus::Connecting;
                                    self.view.set_server_url(self.url.clone());
                                    self.send(OnlineCommand::Connect {
                                        url: self.url.clone(),
                                        session: self.view.session.clone(),
                                    });
                                }
                            } else if self.view.room.is_none() {
                                ui.label(format!("玩家：{}", settings.player_name));
                                ui.horizontal(|ui| {
                                    ui.label("房间号");
                                    ui.text_edit_singleline(&mut self.room_code);
                                });
                                let (name, options) = mapping::room_options(
                                    settings.players,
                                    settings.starting_stack,
                                    settings.small_blind,
                                    settings.big_blind,
                                    settings.ante,
                                    settings.is_short_deck(),
                                    settings.ai_delay_ms,
                                    &settings.player_name,
                                );
                                ui.add_enabled_ui(self.view.pending.is_none(), |ui| {
                                    if ui.button("进入房间").clicked() {
                                        self.send(OnlineCommand::JoinRoom {
                                            code: self.room_code.clone(),
                                            name: name.clone(),
                                            options: options.clone(),
                                        });
                                    }
                                    if ui.button("创建空闲房间").clicked() {
                                        self.send(OnlineCommand::CreateRoom { name, options });
                                    }
                                });
                            } else {
                                ui.heading(format!(
                                    "房间 {}",
                                    self.view.room_code().unwrap_or_default()
                                ));
                                ui.label("将房间号和服务器地址分享给朋友");
                                for p in &self.view.players {
                                    ui.label(format!(
                                        "{}  {}  {}",
                                        if self.view.host_id.as_deref() == Some(&p.id) {
                                            "房主"
                                        } else {
                                            "玩家"
                                        },
                                        p.name,
                                        if p.connected {
                                            "已连接"
                                        } else {
                                            "重连中"
                                        }
                                    ));
                                }
                                if self.view.is_host
                                    && ui
                                        .add_enabled(
                                            self.view.pending.is_none(),
                                            egui::Button::new("开始游戏 · 空位自动加入 AI"),
                                        )
                                        .clicked()
                                {
                                    self.send(OnlineCommand::StartGame {
                                        room_code: self.view.room_code().unwrap_or_default().into(),
                                    });
                                }
                                if !self.view.is_host {
                                    ui.label("等待房主开始游戏…");
                                }
                                if ui.button("离开房间").clicked() {
                                    self.send(OnlineCommand::LeaveRoom {
                                        room_code: self.view.room_code().unwrap_or_default().into(),
                                    });
                                }
                            }
                        });
                });
            });
        }
        home
    }
    fn act(&mut self, action: OnlineAction) {
        if self.view.can_act()
            && let Some(req) = self.view.action_request(action)
        {
            self.send(OnlineCommand::Action(req));
        }
    }
    fn snapshot(&self, settings: &Settings) -> Option<TableSnapshot> {
        let g = self.view.game.as_ref()?;
        let config = &g.room_config;
        let my = self.view.my_player_id.as_deref();
        let mut seats: Vec<_> = g
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| SeatView {
                seat: i,
                name: p.name.clone(),
                stack: mapping::chips(p.stack),
                is_human: my == Some(&p.id),
                folded: p.folded,
                all_in: p.is_all_in,
                is_dealer: g.dealer_pos == i as i64,
                is_small_blind: g.sb_idx == i as i64,
                is_big_blind: g.bb_idx == i as i64,
                is_actor: g.current_player_idx == i as i64,
                is_winner: false,
                last_action: p.last_action.as_ref().map(|a| format!("{:?}", a.action)),
                cards: if my == Some(&p.id) {
                    mapping::cards_from_dtos(&g.your_cards)
                } else {
                    Vec::new()
                },
                cards_visible: my == Some(&p.id)
                    && !self.hidden.unwrap_or(settings.hide_hole_cards),
                bet: mapping::chips(p.round_bet),
            })
            .collect();
        // Rotate recipient to the bottom; keep the wire seat identities for badges and actor flags.
        if let Some(i) = seats.iter().position(|s| s.is_human) {
            seats.rotate_left(i);
        }
        for (i, s) in seats.iter_mut().enumerate() {
            s.seat = i;
        }
        let hole = mapping::cards_from_dtos(&g.your_cards);
        let board = mapping::cards_from_dtos(&g.community_cards);
        let opponents = g
            .players
            .iter()
            .filter(|p| !p.folded && Some(p.id.as_str()) != my)
            .count()
            .max(1);
        let key = crate::analysis::analysis_key(
            &hole,
            &board,
            opponents,
            mapping::variant(g.is_short_deck),
        );
        let summary = self.analysis.get(key);
        let hand_name = summary.as_ref().and_then(|s| s.hand_name);
        let hand_rank_index = summary.as_ref().and_then(|s| s.hand_rank_index);
        Some(TableSnapshot {
            seats,
            community: mapping::cards_from_dtos(&g.community_cards),
            pot: mapping::chips(g.pot),
            phase: mapping::phase_label(g.phase),
            hand_id: g.hand_id.max(0) as u64,
            big_blind: mapping::chips(config.big_blind),
            variant_label: if g.is_short_deck {
                "短牌 6+"
            } else {
                "标准德州"
            }
            .into(),
            variant: mapping::variant(g.is_short_deck),
            stakes_label: format!("{}/{}", config.small_blind, config.big_blind),
            human_turn: self.view.can_act(),
            current_actor_name: mapping::current_actor_name(g),
            legal: mapping::legal_from_wire(&g.legal_actions, config),
            settlement: None,
            analysis: summary.clone(),
            analysis_pending: summary.is_none() && hole.len() == 2,
            analysis_mask: crate::table_ctrl::analysis_mask(
                self.hidden.unwrap_or(settings.hide_hole_cards),
                settings.show_analysis,
            ),
            analysis_error: None,
            hand_name,
            hand_rank_index,
            game_over: None,
            ai_name: "Rust AI".into(),
            hide_cards: self.hidden.unwrap_or(settings.hide_hole_cards),
            music_enabled: settings.music_enabled,
            show_analysis: settings.show_analysis,
            event_line: self.view.status_line.clone(),
            tier: WindowTier::Normal,
            theme: settings.theme,
        })
    }
}
