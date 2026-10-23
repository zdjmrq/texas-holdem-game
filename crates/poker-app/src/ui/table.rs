//! Table screen widgets: canvas, top bar, action bar, analysis panel and the
//! settlement dialog. All functions are stateless: the app builds a
//! TableSnapshot each frame and translates the returned action into state
//! changes.

use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, FontId, Id, Rect, Sense, Stroke, StrokeKind, Ui};
use poker_core::Card;
use poker_engine::{ActionType, LegalActions};

use crate::analysis::AnalysisSummary;
use crate::layout::WindowTier;
use crate::session::{GameOver, Settlement};
use crate::table_ctrl::{AnalysisMask, RaisePanel, format_bb};
use crate::theme::{Palette, Theme};
use crate::ui::components;

/// One seat as rendered on the table.
#[derive(Debug, Clone)]
pub struct SeatView {
    pub seat: usize,
    pub name: String,
    pub stack: u64,
    pub is_human: bool,
    pub folded: bool,
    pub all_in: bool,
    pub is_dealer: bool,
    pub is_small_blind: bool,
    pub is_big_blind: bool,
    pub is_actor: bool,
    pub is_winner: bool,
    pub last_action: Option<String>,
    pub cards: Vec<Card>,
    pub cards_visible: bool,
    pub bet: u64,
}

/// Everything the table screen needs for one frame.
#[derive(Debug, Clone)]
pub struct TableSnapshot {
    pub seats: Vec<SeatView>,
    pub community: Vec<Card>,
    pub pot: u64,
    pub phase: &'static str,
    pub hand_id: u64,
    pub big_blind: u64,
    pub variant_label: String,
    pub variant: poker_core::Variant,
    pub stakes_label: String,
    pub human_turn: bool,
    pub current_actor_name: Option<String>,
    pub legal: LegalActions,
    pub settlement: Option<Settlement>,
    pub analysis: Option<Arc<AnalysisSummary>>,
    pub analysis_pending: bool,
    pub analysis_mask: AnalysisMask,
    pub analysis_error: Option<String>,
    pub hand_name: Option<&'static str>,
    pub hand_rank_index: Option<u8>,
    pub game_over: Option<GameOver>,
    pub ai_name: String,
    pub hide_cards: bool,
    pub music_enabled: bool,
    pub show_analysis: bool,
    pub event_line: String,
    /// Window size tier (normal / compact / tiny) for responsive layout.
    pub tier: WindowTier,
    /// Active theme (dark original / light alternative).
    pub theme: Theme,
}

/// User actions produced by the table widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableUiAction {
    BackHome,
    OpenSettings,
    Fold,
    Check,
    Call,
    AllIn,
    OpenRaise,
    CloseRaise,
    ConfirmRaise,
    StepRaise(i64),
    NextHand,
    ToggleHideCards,
    ToggleAnalysis,
    ToggleMusic,
}

/// Top toolbar: navigation, hand info and quick toggles.
pub fn top_bar(ui: &mut Ui, snapshot: &TableSnapshot) -> Option<TableUiAction> {
    let mut action = None;
    crate::ui::original::header(ui, snapshot.hand_id, &snapshot.stakes_label);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("← 主页").clicked() {
            action = Some(TableUiAction::BackHome);
        }
        ui.separator();
        ui.label(egui::RichText::new(format!("第 {} 手", snapshot.hand_id)).strong());
        ui.label(format!("· {} ·", snapshot.variant_label));
        ui.label(egui::RichText::new(snapshot.phase).strong());
        ui.label(format!("底池 {}", snapshot.pot));
        ui.separator();
        ui.label(snapshot.stakes_label.as_str());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .selectable_label(snapshot.hide_cards, "隐藏手牌")
                .clicked()
            {
                action = Some(TableUiAction::ToggleHideCards);
            }
            if ui
                .selectable_label(snapshot.music_enabled, "音乐")
                .clicked()
            {
                action = Some(TableUiAction::ToggleMusic);
            }
            if ui
                .selectable_label(snapshot.show_analysis, "分析")
                .clicked()
            {
                action = Some(TableUiAction::ToggleAnalysis);
            }
            if ui.button("设置").clicked() {
                action = Some(TableUiAction::OpenSettings);
            }
        });
    });
    action
}

/// Bottom action bar: legal actions, raise panel, waiting state and status line.
pub fn action_bar(
    ui: &mut Ui,
    snapshot: &TableSnapshot,
    raise_panel: &mut RaisePanel,
    focus_raise_slider: bool,
) -> Option<TableUiAction> {
    let mut action = None;
    if let Some(human) = snapshot.seats.iter().find(|s| s.is_human) {
        ui.horizontal(|ui| {
            for card in human.cards.iter().take(2) {
                let (r, response) = ui.allocate_exact_size(components::card_size(), Sense::click());
                components::draw_card(ui.painter(), r, Some(*card), snapshot.hide_cards, false);
                if response.clicked() {
                    action = Some(TableUiAction::ToggleHideCards);
                }
            }
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(&human.name).strong());
                ui.label(
                    egui::RichText::new(format!("$ {}", human.stack))
                        .size(22.0)
                        .strong()
                        .color(snapshot.theme.palette().win),
                );
                ui.label(format!("本轮投入 $ {}", human.bet));
                ui.label(egui::RichText::new("点击手牌隐藏 / 显示").size(11.0).weak());
            });
        });
        ui.add_space(8.0);
    }
    if let Some(settlement) = &snapshot.settlement {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "本手结束：底池 {}，{}",
                    settlement.pot(),
                    winner_summary(settlement)
                ))
                .strong(),
            );
            ui.label(
                egui::RichText::new("结算弹窗中可继续：N / Space 下一手，Esc 关闭")
                    .weak()
                    .small(),
            );
        });
        return action;
    }

    if snapshot.human_turn {
        ui.horizontal_wrapped(|ui| {
            let legal = &snapshot.legal;
            if legal.contains(ActionType::Fold) {
                let response = ui.add(
                    egui::Button::new(egui::RichText::new("弃牌 (F)").color(Color32::WHITE))
                        .fill(Color32::from_rgb(198, 40, 40))
                        .min_size(egui::vec2(110.0, 40.0))
                        .corner_radius(8),
                );
                components::focus_ring(ui, &response);
                if response.clicked() {
                    action = Some(TableUiAction::Fold);
                }
            }
            if legal.contains(ActionType::Check) {
                let response = ui.add(
                    egui::Button::new(egui::RichText::new("过牌 (Space)").color(Color32::WHITE))
                        .fill(Color32::from_rgb(21, 101, 192))
                        .min_size(egui::vec2(110.0, 40.0))
                        .corner_radius(8),
                );
                components::focus_ring(ui, &response);
                if response.clicked() {
                    action = Some(TableUiAction::Check);
                }
            }
            if legal.contains(ActionType::Call) {
                let response = ui.add(
                    egui::Button::new(
                        egui::RichText::new(format!("跟注 {} (C)", legal.to_call))
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(46, 125, 50))
                    .min_size(egui::vec2(120.0, 40.0))
                    .corner_radius(8),
                );
                components::focus_ring(ui, &response);
                if response.clicked() {
                    action = Some(TableUiAction::Call);
                }
            }
            if legal.contains(ActionType::Raise) {
                let response = ui.add(
                    egui::Button::new(egui::RichText::new("加注 (R)").color(Color32::WHITE))
                        .fill(Color32::from_rgb(230, 81, 0))
                        .min_size(egui::vec2(110.0, 40.0))
                        .corner_radius(8),
                );
                components::focus_ring(ui, &response);
                if response.clicked() {
                    action = Some(TableUiAction::OpenRaise);
                }
            }
            if legal.contains(ActionType::AllIn) {
                let response = ui.add(
                    egui::Button::new(egui::RichText::new("全下 (A)").color(Color32::WHITE))
                        .fill(Color32::from_rgb(136, 14, 79))
                        .min_size(egui::vec2(110.0, 40.0))
                        .corner_radius(8),
                );
                components::focus_ring(ui, &response);
                if response.clicked() {
                    action = Some(TableUiAction::AllIn);
                }
            }
            ui.separator();
            ui.label(format!(
                "你的筹码：{}",
                format_bb(stack_of_human(snapshot), snapshot.big_blind)
            ));
        });

        if raise_panel.open {
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label("加注到");
                let mut value = raise_panel.amount as f64;
                let range = raise_panel.min as f64..=raise_panel.max as f64;
                let slider = ui.add(egui::Slider::new(&mut value, range).show_value(false));
                if slider.changed() {
                    raise_panel.set_amount(value as u64);
                }
                if focus_raise_slider {
                    slider.request_focus();
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{}（{}）",
                        raise_panel.amount,
                        raise_panel.pot_fraction_label(snapshot.pot)
                    ))
                    .strong(),
                );
                if ui.button("− 单位 (1/←/↓)").clicked() {
                    action = Some(TableUiAction::StepRaise(-1));
                }
                if ui.button("+ 单位 (2/→/↑)").clicked() {
                    action = Some(TableUiAction::StepRaise(1));
                }
                if ui.button("确认加注 (Enter)").clicked() {
                    action = Some(TableUiAction::ConfirmRaise);
                }
                if ui.button("取消 (Esc)").clicked() {
                    action = Some(TableUiAction::CloseRaise);
                }
            });
        }
    } else if let Some(actor) = &snapshot.current_actor_name {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("等待 {actor} 行动…（{}）", snapshot.ai_name));
        });
    } else {
        ui.label("等待发牌…");
    }

    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("手数：{}", snapshot.hand_id));
        ui.separator();
        ui.label(format!(
            "BB：{}",
            format_bb(stack_of_human(snapshot), snapshot.big_blind)
        ));
        ui.separator();
        ui.label(
            match crate::table_ctrl::visible_hand_name(snapshot.hide_cards, snapshot.hand_name) {
                Some(name) => format!("当前牌型：{name}"),
                None => "当前牌型：-".to_owned(),
            },
        );
        ui.separator();
        ui.label(snapshot.event_line.as_str());
    });
    action
}

fn stack_of_human(snapshot: &TableSnapshot) -> u64 {
    snapshot
        .seats
        .iter()
        .find(|seat| seat.is_human)
        .map_or(0, |seat| seat.stack)
}

fn winner_summary(settlement: &Settlement) -> String {
    if settlement.winners.is_empty() {
        return "无人获胜".to_owned();
    }
    settlement
        .winners
        .iter()
        .map(|(name, amount)| format!("{name} +{amount}"))
        .collect::<Vec<_>>()
        .join("，")
}

/// Right-hand analysis panel.
pub fn analysis_panel(ui: &mut Ui, snapshot: &TableSnapshot) {
    ui.add_space(4.0);
    ui.heading("胜率分析");
    match snapshot.analysis_mask {
        AnalysisMask::HiddenCards => {
            ui.colored_label(Color32::from_rgb(240, 190, 90), "手牌已隐藏，分析已遮蔽");
            ui.label("点击顶栏「隐藏手牌」恢复。");
            return;
        }
        AnalysisMask::Disabled => {
            ui.label("分析面板已在设置中关闭。");
            return;
        }
        AnalysisMask::Visible => {}
    }
    if let Some(summary) = &snapshot.analysis {
        ui.add(
            egui::ProgressBar::new((summary.win_pct() / 100.0) as f32)
                .text(format!("胜 {:.1}%", summary.win_pct())),
        );
        ui.add(
            egui::ProgressBar::new((summary.tie_pct() / 100.0) as f32)
                .text(format!("平 {:.1}%", summary.tie_pct())),
        );
        ui.add(
            egui::ProgressBar::new((summary.lose_pct() / 100.0) as f32)
                .text(format!("负 {:.1}%", summary.lose_pct())),
        );
        ui.label(format!("期望底池份额：{:.1}%", summary.equity_pct()));
        ui.separator();
        if let Some(name) = snapshot.hand_name.or(summary.hand_name) {
            ui.label(format!("当前牌型：{name}"));
        }
        if let Some(rank) = snapshot.hand_rank_index {
            ui.label("可击败的牌型：");
            ui.horizontal_wrapped(|ui| {
                for (index, name) in snapshot.variant.hand_type_names().iter().enumerate() {
                    let color = if index < usize::from(rank) {
                        Color32::from_rgb(120, 210, 130)
                    } else if index == usize::from(rank) {
                        Color32::from_rgb(240, 200, 100)
                    } else {
                        Color32::from_rgb(220, 120, 120)
                    };
                    ui.label(egui::RichText::new(*name).color(color).small());
                }
            });
        }
        if summary.outs_total > 0 {
            ui.label(format!(
                "outs：{}（强 {} / 薄 {}）",
                summary.outs_total, summary.outs_strong, summary.outs_thin
            ));
            if let Some(outs) = &summary.equity.outs {
                for (name, group) in outs.by_hand_type.iter().rev().take(5) {
                    ui.label(format!(
                        "{name}：{} 张 · 强 {} / 薄 {}",
                        group.count, group.strong_count, group.thin_count
                    ));
                }
            }
        } else if snapshot.community.len() == 3 || snapshot.community.len() == 4 {
            ui.label("outs：无改善牌");
        }
        ui.label(
            egui::RichText::new(format!(
                "{} 样本，{} ms",
                summary.equity.samples, summary.equity.elapsed_ms as u64
            ))
            .weak()
            .small(),
        );
    } else if snapshot.analysis_pending {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("计算中…");
        });
    } else {
        ui.label("暂无分析结果");
    }
    if let Some(error) = &snapshot.analysis_error {
        ui.colored_label(
            Color32::from_rgb(230, 120, 120),
            format!("分析失败：{error}"),
        );
    }
    ui.separator();
    ui.label(
        egui::RichText::new(
            "快捷键：Space/Enter 过牌/跟注，F 弃牌，R 加注，A 全下，加注面板 1/←/↓ 减、2/→/↑ 加一个单位，Esc 取消，N 下一手",
        )
        .small()
        .weak(),
    );
}

/// Central table canvas: felt, community cards, pot and every seat.
pub fn show(ui: &mut Ui, snapshot: &TableSnapshot) -> Option<TableUiAction> {
    let action = None;
    let palette = snapshot.theme.palette();
    let scale = snapshot.tier.scale();
    let (rect, _response) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let painter = ui.painter().clone();
    crate::ui::original::background(ui, rect);
    let table_rect = rect.shrink2(egui::vec2(12.0, 12.0));
    crate::ui::original::felt(&painter, table_rect);

    let card_size = components::scaled_card_size(scale);
    let gap = 8.0 * scale;
    let total_width = 5.0 * card_size.x + 4.0 * gap;
    let community_center = table_rect.center() + egui::vec2(0.0, table_rect.height() * 0.08);
    let first_x = community_center.x - total_width * 0.5;
    for index in 0..5 {
        let card_rect = Rect::from_min_size(
            egui::pos2(
                first_x + index as f32 * (card_size.x + gap),
                community_center.y - card_size.y * 0.5,
            ),
            card_size,
        );
        match snapshot.community.get(index).copied() {
            Some(card) => {
                let highlight = index + 1 == snapshot.community.len();
                components::draw_card(&painter, card_rect, Some(card), false, highlight);
            }
            None => components::draw_card_slot(&painter, card_rect),
        }
    }

    painter.text(
        community_center + egui::vec2(0.0, card_size.y * 0.5 + 48.0),
        Align2::CENTER_BOTTOM,
        format!(
            "底池 {} （{:.1} BB）",
            snapshot.pot,
            snapshot.pot as f64 / snapshot.big_blind.max(1) as f64
        ),
        FontId::proportional(18.0 * scale),
        palette.gold,
    );
    painter.text(
        community_center - egui::vec2(0.0, card_size.y * 0.5 + 30.0),
        Align2::CENTER_TOP,
        format!("{} · 第 {} 手", snapshot.phase, snapshot.hand_id),
        FontId::proportional(14.0 * scale),
        palette.text_secondary,
    );

    let positions = components::seat_positions(snapshot.seats.len(), table_rect);
    for (seat, position) in snapshot.seats.iter().zip(positions.iter()) {
        if seat.is_human {
            continue;
        }
        draw_seat(
            &painter,
            *position,
            seat,
            snapshot.big_blind,
            table_rect.center(),
            scale,
            palette,
        );
    }

    action
}

fn draw_seat(
    painter: &egui::Painter,
    position: egui::Pos2,
    seat: &SeatView,
    _big_blind: u64,
    _table_center: egui::Pos2,
    scale: f32,
    palette: &Palette,
) {
    let width = 104.0 * scale;
    let box_rect = Rect::from_center_size(position, egui::vec2(width, 210.0 * scale));
    let color = if seat.is_actor {
        palette.gold
    } else if seat.is_winner {
        palette.win
    } else {
        Color32::from_white_alpha(38)
    };
    painter.rect_filled(
        box_rect,
        10,
        if seat.is_actor {
            Color32::from_rgba_unmultiplied(76, 66, 20, 235)
        } else {
            Color32::from_black_alpha(if seat.folded { 80 } else { 155 })
        },
    );
    painter.rect_stroke(
        box_rect,
        10,
        Stroke::new(if seat.is_actor { 2.0 } else { 1.0 }, color),
        StrokeKind::Inside,
    );
    let opacity = if seat.folded { 0.45 } else { 1.0 };
    let center_x = position.x;
    let avatar = if seat.is_human {
        "♠"
    } else {
        ["😎", "🤠", "🕶", "🎩", "👑", "🦊", "🐺", "🦅", "🐯"][seat.seat.saturating_sub(1) % 9]
    };
    painter.text(
        egui::pos2(center_x, box_rect.top() + 23.0 * scale),
        Align2::CENTER_CENTER,
        avatar,
        FontId::proportional(22.0 * scale),
        palette.gold.gamma_multiply(opacity),
    );
    painter.text(
        egui::pos2(center_x, box_rect.top() + 47.0 * scale),
        Align2::CENTER_CENTER,
        &seat.name,
        FontId::proportional(11.0 * scale),
        Color32::WHITE.gamma_multiply(opacity),
    );
    painter.text(
        egui::pos2(center_x, box_rect.top() + 65.0 * scale),
        Align2::CENTER_CENTER,
        format!("$ {}", seat.stack),
        FontId::proportional(14.0 * scale),
        palette.win.gamma_multiply(opacity),
    );
    let card_size = components::scaled_mini_card_size(scale);
    let cards = Rect::from_center_size(
        egui::pos2(center_x, box_rect.top() + 100.0 * scale),
        egui::vec2(card_size.x * 2.0 + 4.0 * scale, card_size.y),
    );
    if !seat.folded {
        for index in 0..2 {
            let r = Rect::from_min_size(
                cards.min + egui::vec2(index as f32 * (card_size.x + 4.0 * scale), 0.0),
                card_size,
            );
            components::draw_card(
                painter,
                r,
                seat.cards.get(index).copied(),
                !seat.cards_visible,
                false,
            );
        }
    }
    let badge = if seat.is_dealer {
        "D"
    } else if seat.is_small_blind {
        "SB"
    } else if seat.is_big_blind {
        "BB"
    } else {
        ""
    };
    if !badge.is_empty() {
        let c = box_rect.left_top() + egui::vec2(10.0, 10.0);
        painter.circle_filled(c, 10.0, palette.gold);
        painter.text(
            c,
            Align2::CENTER_CENTER,
            badge,
            FontId::proportional(9.0),
            Color32::BLACK,
        );
    }
    let status = if seat.folded {
        "已弃牌".to_owned()
    } else if seat.all_in {
        "ALL IN".to_owned()
    } else if seat.is_actor {
        "思考中 ···".to_owned()
    } else {
        seat.last_action.clone().unwrap_or_default()
    };
    painter.text(
        egui::pos2(center_x, box_rect.bottom() - 13.0 * scale),
        Align2::CENTER_CENTER,
        status,
        FontId::proportional(11.0 * scale),
        palette.gold.gamma_multiply(opacity),
    );
    if seat.bet > 0 {
        painter.text(
            egui::pos2(center_x, box_rect.bottom() - 45.0 * scale),
            Align2::CENTER_CENTER,
            format!("● $ {}", seat.bet),
            FontId::proportional(12.0 * scale),
            palette.gold,
        );
    }
}

/// What the settlement dialog produced this frame.
#[derive(Debug, Default)]
pub struct SettlementModalResult {
    pub action: Option<TableUiAction>,
    pub next_button: Option<egui::Id>,
    pub home_button: Option<egui::Id>,
    pub dragged: bool,
}

/// Settlement dialog (modal). Returns the chosen action plus the focusable
/// button ids (used by the app for the initial focus and the Tab focus trap).
pub fn settlement_modal(
    ctx: &egui::Context,
    snapshot: &TableSnapshot,
    offset: &mut egui::Vec2,
    focus_primary: bool,
) -> SettlementModalResult {
    let mut result = SettlementModalResult::default();
    let Some(settlement) = snapshot.settlement.as_ref() else {
        return result;
    };
    let palette = snapshot.theme.palette();
    let modal_id = Id::unique("settlement-modal");
    // ui.md 3.9: the dialog is draggable and resets to the centre on reopen.
    let screen = ctx.content_rect();
    let max_offset = egui::vec2(
        (screen.width() * 0.5 - 160.0).max(0.0),
        (screen.height() * 0.5 - 120.0).max(0.0),
    );
    *offset = egui::vec2(
        offset.x.clamp(-max_offset.x, max_offset.x),
        offset.y.clamp(-max_offset.y, max_offset.y),
    );
    let area = egui::Modal::default_area(modal_id).anchor(Align2::CENTER_CENTER, *offset);
    egui::Modal::new(modal_id)
        .area(area)
        .backdrop_color(palette.overlay)
        .show(ctx, |ui| {
            let handle = ui
                .horizontal(|ui| {
                    ui.label(egui::RichText::new("⣿ 拖动").small().weak());
                })
                .response
                .interact(Sense::drag());
            if handle.dragged() {
                let delta = ctx.input(|input| input.pointer.delta());
                *offset += delta;
                result.dragged = true;
                ctx.request_repaint();
            }
            ui.set_min_width(480.0);
            ui.heading(if settlement.game_over.is_some() {
                "对局结束"
            } else {
                "本手结算"
            });
            ui.separator();
            ui.label(
                egui::RichText::new(format!(
                    "第 {} 手 · 底池 {}",
                    settlement.hand_id,
                    settlement.pot()
                ))
                .strong(),
            );
            if !settlement.winners.is_empty() {
                let hand_by_name = |name: &str| {
                    settlement
                        .showdown
                        .iter()
                        .find(|entry| entry.name == name)
                        .map(|entry| entry.hand_name)
                };
                let winners = settlement
                    .winners
                    .iter()
                    .map(|(name, amount)| match hand_by_name(name) {
                        Some(hand) => format!("{name} +{amount}（{hand}）"),
                        None => format!("{name} +{amount}"),
                    })
                    .collect::<Vec<_>>()
                    .join("，");
                ui.label(format!("赢家：{winners}"));
            }
            let color = if settlement.human_delta >= 0 {
                Color32::from_rgb(110, 220, 130)
            } else {
                Color32::from_rgb(230, 120, 120)
            };
            ui.colored_label(
                color,
                format!(
                    "你的变化：{}{} 筹码",
                    if settlement.human_delta >= 0 { "+" } else { "" },
                    settlement.human_delta
                ),
            );
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label("公共牌：");
                for card in &settlement.board {
                    components::card_widget(ui, *card);
                }
            });
            if !settlement.showdown.is_empty() {
                ui.separator();
                ui.label("摊牌：");
                let winner_names: Vec<&str> = settlement
                    .winners
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect();
                egui::ScrollArea::vertical()
                    .max_height(340.0)
                    .show(ui, |ui| {
                        for entry in &settlement.showdown {
                            let is_winner = winner_names.contains(&entry.name.as_str());
                            ui.horizontal(|ui| {
                                let prefix = if is_winner { "[赢] " } else { "" };
                                ui.label(format!("{prefix}{}：", entry.name));
                                for card in &entry.cards {
                                    components::mini_card_widget(ui, *card);
                                }
                                ui.label(entry.hand_name);
                            });
                        }
                    });
            }
            if let Some(game_over) = settlement.game_over {
                ui.separator();
                ui.colored_label(
                    Color32::from_rgb(240, 190, 90),
                    format!("{}，{}", game_over.label(), "点击回主页重新开始"),
                );
            }
            ui.add_space(6.0);
            ui.separator();
            ui.horizontal(|ui| {
                if settlement.game_over.is_none() {
                    let response = ui.button("下一手 (N)");
                    components::focus_ring(ui, &response);
                    result.next_button = Some(response.id);
                    if focus_primary {
                        response.request_focus();
                    }
                    if response.clicked() {
                        result.action = Some(TableUiAction::NextHand);
                    }
                }
                let response = ui.button("回主页");
                components::focus_ring(ui, &response);
                result.home_button = Some(response.id);
                if focus_primary && settlement.game_over.is_some() {
                    response.request_focus();
                }
                if response.clicked() {
                    result.action = Some(TableUiAction::BackHome);
                }
            });
        });
    result
}

/// Reference bottom panel: player cards/actions on the left, analysis on the right.
pub fn bottom_panel(
    ui: &mut Ui,
    snapshot: &TableSnapshot,
    raise: &mut RaisePanel,
    focus: bool,
) -> Option<TableUiAction> {
    let mut action = None;
    if snapshot.show_analysis && ui.available_width() > 940.0 {
        ui.horizontal_top(|ui| {
            let width = (ui.available_width() - 300.0).max(500.0);
            ui.allocate_ui_with_layout(
                egui::vec2(width, 190.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    action = action_bar(ui, snapshot, raise, focus);
                },
            );
            ui.separator();
            ui.allocate_ui_with_layout(
                egui::vec2(280.0, 190.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    analysis_panel(ui, snapshot);
                },
            );
        });
    } else {
        action = action_bar(ui, snapshot, raise, focus);
    }
    action
}
