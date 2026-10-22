//! Shared drawing helpers for cards, chips and table geometry.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind,
    Ui, Vec2,
};
use poker_core::{Card, Suit};

use crate::theme::Palette;

/// Size of a centre/community card.
#[must_use]
pub fn card_size() -> Vec2 {
    egui::vec2(58.0, 82.0)
}

/// Size of a seat hole card.
#[must_use]
pub fn mini_card_size() -> Vec2 {
    egui::vec2(38.0, 56.0)
}

/// Community card size after applying the window-tier scale.
#[must_use]
pub fn scaled_card_size(scale: f32) -> Vec2 {
    card_size() * scale
}

/// Seat hole card size after applying the window-tier scale.
#[must_use]
pub fn scaled_mini_card_size(scale: f32) -> Vec2 {
    mini_card_size() * scale
}

/// Draw the golden keyboard-focus ring used by ui.md 2.8 (3px gold).
pub fn focus_ring(ui: &Ui, response: &Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            CornerRadius::same(6),
            Stroke::new(2.0, crate::theme::DARK.gold),
            StrokeKind::Outside,
        );
    }
}

pub fn card_back_color() -> Color32 {
    Color32::from_rgb(32, 45, 132)
}

pub fn card_face_color() -> Color32 {
    Color32::from_rgb(255, 255, 255)
}

pub fn suit_color(suit: Suit) -> Color32 {
    match suit {
        Suit::Hearts | Suit::Diamonds => Color32::from_rgb(200, 40, 40),
        Suit::Spades | Suit::Clubs => Color32::from_rgb(28, 28, 32),
    }
}

pub fn felt_color(palette: &Palette) -> Color32 {
    palette.felt_outer
}

pub fn table_edge_color(palette: &Palette) -> Color32 {
    palette.table_edge
}

/// Draw one playing card (face up or face down).
pub fn draw_card(
    painter: &egui::Painter,
    rect: Rect,
    card: Option<Card>,
    face_down: bool,
    highlight: bool,
) {
    let radius = CornerRadius::same(6);
    if face_down || card.is_none() {
        painter.rect_filled(rect, radius, card_back_color());
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, Color32::from_rgb(96, 126, 196)),
            StrokeKind::Inside,
        );
        let mut x = rect.left() - rect.height();
        while x < rect.right() {
            painter.line_segment(
                [
                    egui::pos2(
                        x.max(rect.left()),
                        rect.bottom() - (rect.left() - x).max(0.0),
                    ),
                    egui::pos2(
                        (x + rect.height()).min(rect.right()),
                        rect.top() + (x + rect.height() - rect.right()).max(0.0),
                    ),
                ],
                Stroke::new(3.0, Color32::from_white_alpha(12)),
            );
            x += 10.0;
        }
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "♠",
            FontId::proportional(rect.width() * 0.55),
            Color32::from_white_alpha(55),
        );
        return;
    }
    let card = card.expect("card is Some when face up");
    painter.rect_filled(rect, radius, card_face_color());
    let stroke_color = if highlight {
        Color32::from_rgb(255, 200, 60)
    } else {
        Color32::from_rgb(120, 120, 120)
    };
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, stroke_color),
        StrokeKind::Inside,
    );
    let rank_font = if rect.width() < 36.0 { 12.0 } else { 17.0 };
    let suit_font = if rect.width() < 36.0 { 14.0 } else { 21.0 };
    let color = suit_color(card.suit());
    painter.text(
        rect.center_top() + egui::vec2(0.0, 4.0),
        Align2::CENTER_TOP,
        card.rank().as_str(),
        FontId::proportional(rank_font),
        color,
    );
    painter.text(
        rect.center_bottom() - egui::vec2(0.0, 4.0),
        Align2::CENTER_BOTTOM,
        card.suit().symbol().to_string(),
        FontId::proportional(suit_font),
        color,
    );
}

/// Empty community-card placeholder (pre-flop / not yet dealt).
pub fn draw_card_slot(painter: &egui::Painter, rect: Rect) {
    let radius = CornerRadius::same(6);
    painter.rect_filled(rect, radius, Color32::from_rgb(22, 70, 56));
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, Color32::from_rgb(60, 120, 96)),
        StrokeKind::Inside,
    );
}

/// An interactive card widget (used by the settlement dialog).
pub fn card_widget(ui: &mut Ui, card: Card) -> Response {
    let (rect, response) = ui.allocate_exact_size(card_size(), Sense::hover());
    draw_card(ui.painter(), rect, Some(card), false, false);
    response
}

/// Compact card widget for dense lists (showdown rows).
pub fn mini_card_widget(ui: &mut Ui, card: Card) -> Response {
    let (rect, response) = ui.allocate_exact_size(mini_card_size(), Sense::hover());
    draw_card(ui.painter(), rect, Some(card), false, false);
    response
}

/// Seat centre positions around an ellipse; seat 0 is the bottom (human).
#[must_use]
pub fn seat_positions(count: usize, area: Rect) -> Vec<Pos2> {
    if count == 0 {
        return Vec::new();
    }
    let mut positions = vec![egui::pos2(area.center().x, area.bottom() - 83.0)];
    let ai = count.saturating_sub(1);
    if ai == 0 {
        return positions;
    }
    let cap = ((area.width() / 130.0) as usize).clamp(3, 7);
    let sides = ai.saturating_sub(cap).min(2);
    let top = ai - sides;
    let margin = 66.0 + if sides > 0 { 20.0 } else { 0.0 };
    let y = area.top() + 117.0;
    if sides > 0 {
        positions.push(egui::pos2(area.left() + 66.0, area.bottom() - 112.0));
    }
    for i in 0..top {
        let t = if top > 1 {
            i as f32 / (top - 1) as f32
        } else {
            0.5
        };
        positions.push(egui::pos2(
            area.left() + margin + (area.width() - margin * 2.0) * t,
            y + 24.0 * (t - 0.5).abs() * 2.0,
        ));
    }
    if sides > 1 {
        positions.push(egui::pos2(area.right() - 66.0, area.bottom() - 95.0));
    }
    positions
}

/// A rounded panel used for seat boxes.
pub fn seat_color(palette: &Palette, is_human: bool, is_actor: bool, folded: bool) -> Color32 {
    if folded {
        return palette.seat_folded;
    }
    if is_actor {
        return palette.seat_actor;
    }
    if is_human {
        return palette.seat_human;
    }
    palette.seat_bg
}
