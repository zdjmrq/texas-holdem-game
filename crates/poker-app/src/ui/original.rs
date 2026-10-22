//! Native rendering of the reference v1.5.8 visual language.
use crate::settings::{
    AI_DELAYS_MS, SHORT_DECK_ANTES, STACKS, STANDARD_BLINDS, Settings, VariantSetting,
};
use eframe::egui::{self, Color32, Rect, RichText, Stroke, StrokeKind, Ui};
const RED: Color32 = Color32::from_rgb(233, 69, 96);
const GOLD: Color32 = Color32::from_rgb(255, 215, 0);

pub fn background(ui: &Ui, rect: Rect) {
    let p = ui.painter();
    p.rect_filled(rect, 0, Color32::from_rgb(48, 63, 87));
    let size = 29.0;
    let mut y = rect.top();
    let mut row = 0;
    while y < rect.bottom() {
        let mut x = rect.left();
        let mut col = 0;
        while x < rect.right() {
            if (row + col) % 2 == 0 {
                p.rect_filled(
                    Rect::from_min_size(egui::pos2(x, y), egui::vec2(size, size)).intersect(rect),
                    0,
                    Color32::from_white_alpha(7),
                );
            }
            x += size;
            col += 1;
        }
        y += size;
        row += 1;
    }
}

pub fn header(ui: &mut Ui, hand: u64, stakes: &str) {
    egui::Frame::new()
        .fill(Color32::from_rgb(20, 27, 47))
        .corner_radius(16)
        .stroke(Stroke::new(1.0, RED.gamma_multiply(0.45)))
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                let mut title = egui::text::LayoutJob::default();
                for (text, color) in [("♠ ", RED), ("Texas", GOLD), (" Hold'em ♥", RED)] {
                    title.append(
                        text,
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(28.0),
                            color,
                            ..Default::default()
                        },
                    );
                }
                ui.label(title);
                ui.horizontal(|ui| {
                    let w = ui.available_width();
                    ui.add_space((w - 260.0).max(0.0) / 2.0);
                    ui.label(
                        RichText::new(format!("手牌 #{hand}     {stakes}"))
                            .color(Color32::from_gray(180))
                            .size(12.0),
                    );
                });
            });
        });
}

pub fn felt(p: &egui::Painter, rect: Rect) {
    p.rect_filled(
        rect.translate(egui::vec2(0.0, 7.0)),
        120,
        Color32::from_black_alpha(95),
    );
    p.rect_filled(rect, 120, Color32::from_rgb(93, 64, 55));
    // Concentric soft layers reproduce the original radial felt without an image asset.
    let inside = rect.shrink(8.0);
    for i in 0..24 {
        let t = i as f32 / 23.0;
        let r = inside.shrink2(egui::vec2(
            t * inside.width() * 0.19,
            t * inside.height() * 0.21,
        ));
        p.rect_filled(
            r,
            (112.0 - t * 40.0) as u8,
            Color32::from_rgb(
                (10.0 + t * 17.0) as u8,
                (40.0 + t * 54.0) as u8,
                (11.0 + t * 21.0) as u8,
            ),
        );
    }
    p.rect_stroke(
        inside,
        112,
        Stroke::new(1.0, Color32::from_white_alpha(12)),
        StrokeKind::Inside,
    );
    for (s, x) in [("♠", 0.22), ("♥", 0.5), ("♦", 0.78)] {
        p.text(
            egui::pos2(rect.left() + rect.width() * x, rect.center().y),
            egui::Align2::CENTER_CENTER,
            s,
            egui::FontId::proportional(70.0),
            Color32::from_white_alpha(5),
        );
    }
}

pub fn button(ui: &mut Ui, label: &str, selected: bool) -> bool {
    ui.add(
        egui::Button::new(RichText::new(label).color(if selected {
            GOLD
        } else {
            Color32::from_gray(205)
        }))
        .fill(if selected {
            Color32::from_rgb(81, 64, 34)
        } else {
            Color32::from_black_alpha(70)
        })
        .stroke(Stroke::new(
            1.0,
            if selected {
                GOLD.gamma_multiply(0.65)
            } else {
                Color32::from_white_alpha(40)
            },
        ))
        .corner_radius(7)
        .min_size(egui::vec2(59.0, 32.0)),
    )
    .clicked()
}

fn row(ui: &mut Ui, width: f32, f: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
        f(ui);
    });
}
fn selector(ui: &mut Ui, label: &str, active: bool, color: Color32) -> bool {
    ui.add(
        egui::Button::new(RichText::new(label).size(15.0).color(if active {
            color
        } else {
            Color32::from_gray(175)
        }))
        .fill(if active {
            color.gamma_multiply(0.15)
        } else {
            Color32::from_white_alpha(8)
        })
        .stroke(Stroke::new(
            2.0,
            if active {
                color
            } else {
                Color32::from_white_alpha(32)
            },
        ))
        .corner_radius(10)
        .min_size(egui::vec2(146.0, 39.0)),
    )
    .clicked()
}
fn range(ui: &mut Ui, id: &str, value: &mut usize, max: usize, ticks: &[&str]) {
    let width = ui.available_width();
    let (r, response) =
        ui.allocate_exact_size(egui::vec2(width, 24.0), egui::Sense::click_and_drag());
    if response.clicked() {
        response.request_focus();
    }
    if (response.clicked() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
    {
        *value = (((pos.x - r.left()) / width).clamp(0.0, 1.0) * max as f32).round() as usize;
    }
    if response.has_focus() {
        if ui.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            *value = (*value + 1).min(max);
        }
        if ui.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            *value = value.saturating_sub(1);
        }
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, ui.is_enabled(), id));
    let track = Rect::from_center_size(r.center(), egui::vec2(width - 8.0, 5.0));
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(track.left_top(), GOLD.gamma_multiply(0.7));
    mesh.colored_vertex(track.right_top(), RED.gamma_multiply(0.7));
    mesh.colored_vertex(track.right_bottom(), RED.gamma_multiply(0.7));
    mesh.colored_vertex(track.left_bottom(), GOLD.gamma_multiply(0.7));
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
    let thumb = egui::pos2(
        track.left() + track.width() * (*value as f32 / max.max(1) as f32),
        r.center().y,
    );
    ui.painter().circle_filled(thumb, 8.0, GOLD);
    ui.painter().circle_stroke(
        thumb,
        8.0,
        Stroke::new(2.0, Color32::from_rgb(255, 243, 168)),
    );
    if response.has_focus() {
        ui.painter()
            .rect_stroke(r, 5, Stroke::new(1.0, GOLD), StrokeKind::Inside);
    }
    ui.horizontal(|ui| {
        for (i, tick) in ticks.iter().enumerate() {
            if i > 0 {
                ui.add_space(
                    ((width - 30.0 * ticks.len() as f32) / (ticks.len() - 1).max(1) as f32)
                        .max(0.0),
                );
            }
            ui.label(
                RichText::new(*tick)
                    .size(9.0)
                    .color(Color32::from_rgb(111, 116, 133)),
            );
        }
    });
}
fn caption(ui: &mut Ui, label: &str, value: String) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .size(12.0)
                .color(Color32::from_gray(170)),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).size(13.0).strong().color(GOLD));
        });
    });
}
/// Original start-screen proportions, rebuilt with native egui widgets.
pub fn home(ui: &mut Ui, settings: &mut Settings, online: &mut bool) -> u8 {
    let mut action = 0;
    background(ui, ui.max_rect());
    let margin = ((ui.available_width() - 1180.0) / 2.0).clamp(10.0, 120.0);
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: margin as i8,
            right: margin as i8,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| {
            header(
                ui,
                0,
                &format!(
                    "SB: {}     BB: {}",
                    settings.small_blind, settings.big_blind
                ),
            );
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("点击「开始游戏」开始德州扑克对局！")
                        .color(Color32::from_rgb(190, 202, 219)),
                );
            });
            ui.add_space(12.0);
            let width = ui.available_width();
            let height = 710.0;
            let (area, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
            felt(ui.painter(), area);
            let content = Rect::from_center_size(
                area.center(),
                egui::vec2(620.0_f32.min(width - 40.0), height - 25.0),
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(content)
                    .layout(egui::Layout::top_down(egui::Align::Center)),
                |ui| {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("♠  ♥  ♦  ♣")
                            .size(60.0)
                            .color(Color32::from_gray(215)),
                    );
                    ui.label(
                        RichText::new("Texas Hold'em")
                            .size(28.0)
                            .strong()
                            .color(GOLD),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("选择人数")
                            .size(13.0)
                            .color(Color32::from_gray(150)),
                    );
                    row(ui, 410.0, |ui| {
                        for n in [2, 6, 7, 8, 9, 10] {
                            if settings.is_short_deck() && n > 8 {
                                continue;
                            }
                            if button(ui, &format!("{n} 人"), settings.players == n) {
                                settings.players = n;
                            }
                        }
                    });
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("游戏模式")
                            .size(13.0)
                            .color(Color32::from_gray(140)),
                    );
                    row(ui, 300.0, |ui| {
                        if selector(
                            ui,
                            "♠ 标准德州",
                            !settings.is_short_deck(),
                            Color32::from_rgb(33, 150, 243),
                        ) {
                            settings.variant = VariantSetting::Standard;
                        }
                        if selector(
                            ui,
                            "♦ 短牌 6+",
                            settings.is_short_deck(),
                            Color32::from_rgb(76, 175, 80),
                        ) {
                            settings.variant = VariantSetting::ShortDeck;
                            settings.players = settings.players.min(8);
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("游玩方式")
                            .size(13.0)
                            .color(Color32::from_gray(140)),
                    );
                    row(ui, 300.0, |ui| {
                        if selector(ui, "本地游戏", !*online, Color32::from_rgb(76, 175, 80)) {
                            *online = false;
                        }
                        if selector(ui, "联网游戏", *online, Color32::from_rgb(76, 175, 80)) {
                            *online = true;
                        }
                    });
                    ui.add_space(12.0);
                    egui::Frame::new()
                        .fill(Color32::from_black_alpha(72))
                        .stroke(Stroke::new(1.0, GOLD.gamma_multiply(0.16)))
                        .corner_radius(12)
                        .inner_margin(16)
                        .show(ui, |ui| {
                            ui.set_width(content.width() - 34.0);
                            ui.columns(2, |columns| {
                                let ui = &mut columns[0];
                                caption(ui, "起始筹码", format!("$ {}", settings.starting_stack));
                                let mut v = STACKS
                                    .iter()
                                    .position(|x| *x == settings.starting_stack)
                                    .unwrap_or(2);
                                range(
                                    ui,
                                    "起始筹码",
                                    &mut v,
                                    4,
                                    &["5千", "1万", "2万", "5万", "10万"],
                                );
                                settings.starting_stack = STACKS[v];
                                let ui = &mut columns[1];
                                if settings.is_short_deck() {
                                    caption(ui, "前注", format!("$ {}", settings.ante));
                                    let mut v = SHORT_DECK_ANTES
                                        .iter()
                                        .position(|x| *x == settings.ante)
                                        .unwrap_or(1);
                                    range(ui, "前注", &mut v, 3, &["20", "40", "80", "100"]);
                                    settings.ante = SHORT_DECK_ANTES[v];
                                } else {
                                    caption(
                                        ui,
                                        "大小盲注",
                                        format!(
                                            "$ {} / $ {}",
                                            settings.small_blind, settings.big_blind
                                        ),
                                    );
                                    let mut v = STANDARD_BLINDS
                                        .iter()
                                        .position(|x| {
                                            *x == (settings.small_blind, settings.big_blind)
                                        })
                                        .unwrap_or(2);
                                    range(
                                        ui,
                                        "盲注",
                                        &mut v,
                                        5,
                                        &[
                                            "10/20", "20/40", "40/80", "50/100", "100/200",
                                            "200/400",
                                        ],
                                    );
                                    (settings.small_blind, settings.big_blind) = STANDARD_BLINDS[v];
                                }
                            });
                            ui.add_space(10.0);
                            row(ui, 290.0, |ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(290.0, 72.0),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        let mut v = AI_DELAYS_MS
                                            .iter()
                                            .position(|x| *x == settings.ai_delay_ms)
                                            .unwrap_or(1);
                                        caption(
                                            ui,
                                            "AI 行动节奏",
                                            ["快速", "标准", "沉浸"][v].into(),
                                        );
                                        range(ui, "AI 节奏", &mut v, 2, &["快速", "标准", "沉浸"]);
                                        settings.ai_delay_ms = AI_DELAYS_MS[v];
                                    },
                                );
                            });
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{} BB 起始深度",
                                        settings.starting_stack / settings.big_blind.max(1)
                                    ))
                                    .size(10.0)
                                    .color(Color32::from_gray(130)),
                                );
                            });
                        });
                    ui.add_space(12.0);
                    if ui
                        .add_sized(
                            [220.0, 51.0],
                            egui::Button::new(
                                RichText::new("开始游戏")
                                    .size(18.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(RED)
                            .corner_radius(12),
                        )
                        .clicked()
                    {
                        action = if *online { 3 } else { 1 };
                    }
                    ui.add_space(6.0);
                    if ui.small_button("音乐与偏好设置").clicked() {
                        action = 2;
                    }
                },
            );
        });
    action
}
