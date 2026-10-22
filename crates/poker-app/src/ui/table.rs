//! Table screen widgets: canvas, top bar, action bar, analysis panel and the
//! settlement dialog. All functions are stateless: the app builds a
//! TableSnapshot each frame and translates the returned action into state
//! changes.

use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Id, Rect, Sense, Stroke, StrokeKind, Ui,
};
use poker_core::Card;
use poker_engine::{ActionType, LegalActions};

use crate::analysis::AnalysisSummary;
use crate::session::{GameOver, Settlement};
use crate::table_ctrl::{AnalysisMask, RaisePanel, format_bb};
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
) -> Option<TableUiAction> {
    let mut action = None;
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
            if legal.contains(ActionType::Fold) && ui.button("弃牌 (F)").clicked() {
                action = Some(TableUiAction::Fold);
            }
            if legal.contains(ActionType::Check) && ui.button("过牌 (Space)").clicked() {
                action = Some(TableUiAction::Check);
            }
            if legal.contains(ActionType::Call)
                && ui.button(format!("跟注 {} (C)", legal.to_call)).clicked()
            {
                action = Some(TableUiAction::Call);
            }
            if legal.contains(ActionType::Raise) && ui.button("加注 (R)").clicked() {
                action = Some(TableUiAction::OpenRaise);
            }
            if legal.contains(ActionType::AllIn) && ui.button("全下 (A)").clicked() {
                action = Some(TableUiAction::AllIn);
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
                let changed = ui
                    .add(egui::Slider::new(&mut value, range).show_value(false))
                    .changed();
                if changed {
                    raise_panel.set_amount(value as u64);
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
        ui.label(match snapshot.hand_name {
            Some(name) => format!("当前牌型：{name}"),
            None => "当前牌型：-".to_owned(),
        });
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
            "快捷键：Space 过牌/跟注，F 弃牌，R 加注，A 全下，1/2 底池预设，方向键调加注，N 下一手",
        )
        .small()
        .weak(),
    );
}

/// Central table canvas: felt, community cards, pot and every seat.
pub fn show(ui: &mut Ui, snapshot: &TableSnapshot) -> Option<TableUiAction> {
    let mut action = None;
    let (rect, _response) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    let painter = ui.painter().clone();
    painter.rect_filled(rect, CornerRadius::same(0), Color32::from_rgb(14, 18, 22));
    let table_rect = rect.shrink2(egui::vec2(28.0, 18.0));
    painter.rect_filled(
        table_rect,
        CornerRadius::same(200),
        components::table_edge_color(),
    );
    painter.rect_filled(
        table_rect.shrink(9.0),
        CornerRadius::same(192),
        components::felt_color(),
    );

    let card_size = components::card_size();
    let gap = 8.0;
    let total_width = 5.0 * card_size.x + 4.0 * gap;
    let community_center = table_rect.center() + egui::vec2(0.0, -card_size.y * 0.15);
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
        community_center - egui::vec2(0.0, card_size.y * 0.5 + 20.0),
        Align2::CENTER_BOTTOM,
        format!(
            "底池 {} （{:.1} BB）",
            snapshot.pot,
            snapshot.pot as f64 / snapshot.big_blind.max(1) as f64
        ),
        FontId::proportional(18.0),
        Color32::from_rgb(255, 226, 150),
    );
    painter.text(
        community_center + egui::vec2(0.0, card_size.y * 0.5 + 20.0),
        Align2::CENTER_TOP,
        format!("{} · 第 {} 手", snapshot.phase, snapshot.hand_id),
        FontId::proportional(14.0),
        Color32::from_rgb(200, 220, 210),
    );

    let positions = components::seat_positions(snapshot.seats.len(), table_rect);
    for (seat, position) in snapshot.seats.iter().zip(positions.iter()) {
        draw_seat(
            &painter,
            *position,
            seat,
            snapshot.big_blind,
            table_rect.center(),
        );
    }

    // Clicking the human hole cards toggles their visibility.
    let human_seat = snapshot.seats.iter().find(|seat| seat.is_human);
    if let Some(human) = human_seat {
        let positions = components::seat_positions(snapshot.seats.len(), table_rect);
        if !human.cards.is_empty()
            && let Some(position) = positions.get(human.seat)
        {
            let cards_rect = hole_cards_rect(*position);
            let response = ui.interact(
                cards_rect,
                ui.scope_id().with("human-hole-cards"),
                Sense::click(),
            );
            if response.clicked() {
                action = Some(TableUiAction::ToggleHideCards);
            }
        }
    }
    action
}

fn hole_cards_rect(seat_position: egui::Pos2) -> Rect {
    let card_size = components::mini_card_size();
    let width = card_size.x * 2.0 + 4.0;
    Rect::from_center_size(
        seat_position - egui::vec2(0.0, 58.0),
        egui::vec2(width, card_size.y),
    )
}

fn draw_seat(
    painter: &egui::Painter,
    position: egui::Pos2,
    seat: &SeatView,
    big_blind: u64,
    table_center: egui::Pos2,
) {
    let box_rect = Rect::from_center_size(position, egui::vec2(158.0, 74.0));
    painter.rect_filled(
        box_rect,
        CornerRadius::same(10),
        components::seat_color(seat.is_human, seat.is_actor, seat.folded),
    );
    let border = if seat.is_winner {
        Color32::from_rgb(255, 205, 70)
    } else if seat.is_actor {
        Color32::from_rgb(255, 190, 60)
    } else {
        Color32::from_rgb(90, 100, 108)
    };
    painter.rect_stroke(
        box_rect,
        CornerRadius::same(10),
        Stroke::new(
            if seat.is_actor || seat.is_winner {
                2.5
            } else {
                1.0
            },
            border,
        ),
        StrokeKind::Inside,
    );

    let avatar_center = box_rect.left_center() + egui::vec2(26.0, 0.0);
    let avatar_color = if seat.is_human {
        Color32::from_rgb(70, 130, 180)
    } else {
        Color32::from_rgb(110, 90, 140)
    };
    painter.circle_filled(avatar_center, 19.0, avatar_color);
    let initial = seat.name.chars().next().unwrap_or('?').to_string();
    painter.text(
        avatar_center,
        Align2::CENTER_CENTER,
        initial,
        FontId::proportional(17.0),
        Color32::WHITE,
    );

    let text_left = box_rect.left() + 50.0;
    painter.text(
        egui::pos2(text_left, box_rect.top() + 7.0),
        Align2::LEFT_TOP,
        &seat.name,
        FontId::proportional(15.0),
        if seat.folded {
            Color32::from_gray(150)
        } else {
            Color32::WHITE
        },
    );
    painter.text(
        egui::pos2(text_left, box_rect.top() + 27.0),
        Align2::LEFT_TOP,
        format_bb(seat.stack, big_blind),
        FontId::proportional(13.0),
        Color32::from_rgb(210, 220, 200),
    );
    if let Some(last_action) = &seat.last_action {
        painter.text(
            egui::pos2(text_left, box_rect.top() + 48.0),
            Align2::LEFT_TOP,
            last_action,
            FontId::proportional(11.5),
            Color32::from_rgb(240, 200, 120),
        );
    }

    let mut badge_x = box_rect.left() + 4.0;
    for (label, color) in [
        (
            seat.is_dealer.then_some("D"),
            Color32::from_rgb(240, 240, 240),
        ),
        (
            seat.is_small_blind.then_some("SB"),
            Color32::from_rgb(120, 180, 240),
        ),
        (
            seat.is_big_blind.then_some("BB"),
            Color32::from_rgb(160, 220, 160),
        ),
        (
            seat.all_in.then_some("ALL-IN"),
            Color32::from_rgb(230, 110, 110),
        ),
    ] {
        if let Some(label) = label {
            let width = if label == "ALL-IN" { 42.0 } else { 18.0 };
            let badge = Rect::from_min_size(
                egui::pos2(badge_x, box_rect.top() - 9.0),
                egui::vec2(width, 16.0),
            );
            painter.rect_filled(badge, CornerRadius::same(8), color);
            painter.text(
                badge.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(10.0),
                Color32::from_rgb(20, 20, 24),
            );
            badge_x += width + 3.0;
        }
    }

    if !seat.cards.is_empty() && !seat.folded {
        let cards_rect = hole_cards_rect(position);
        let card_size = components::mini_card_size();
        for (index, card) in seat.cards.iter().take(2).enumerate() {
            let rect = Rect::from_min_size(
                egui::pos2(
                    cards_rect.left() + index as f32 * (card_size.x + 4.0),
                    cards_rect.top(),
                ),
                card_size,
            );
            components::draw_card(
                painter,
                rect,
                Some(*card),
                !seat.cards_visible,
                seat.is_actor,
            );
        }
    }

    if seat.bet > 0 {
        let direction = (table_center - position).normalized();
        painter.text(
            position + direction * 88.0,
            Align2::CENTER_CENTER,
            format!("● {}", seat.bet),
            FontId::proportional(14.0),
            Color32::from_rgb(255, 215, 120),
        );
    }
}

/// Settlement dialog (modal). Returns the action chosen by the player.
pub fn settlement_modal(ctx: &egui::Context, snapshot: &TableSnapshot) -> Option<TableUiAction> {
    let settlement = snapshot.settlement.as_ref()?;
    let mut action = None;
    let modal = egui::Modal::new(Id::unique("settlement-modal")).show(ctx, |ui| {
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
            if settlement.game_over.is_none() && ui.button("下一手 (N)").clicked() {
                action = Some(TableUiAction::NextHand);
            }
            if ui.button("回主页").clicked() {
                action = Some(TableUiAction::BackHome);
            }
        });
    });
    let _ = modal;
    action
}
