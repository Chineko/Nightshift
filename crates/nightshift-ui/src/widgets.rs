#![allow(dead_code)]

use std::ops::RangeInclusive;

use eframe::egui::{self, Color32, Response, RichText, Sense, Stroke};

const PANEL_ALT: Color32 = Color32::from_rgb(30, 31, 38);
const TRACK: Color32 = Color32::from_rgb(48, 52, 66);
const MUTED: Color32 = Color32::from_rgb(157, 159, 170);
const PURPLE: Color32 = Color32::from_rgb(183, 141, 255);
const PURPLE_DARK: Color32 = Color32::from_rgb(62, 50, 83);
const TEXT: Color32 = Color32::from_rgb(238, 238, 242);

pub(crate) fn pill_toggle(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut bool,
    enabled: bool,
) -> Response {
    let mut toggle_response = None;
    ui.horizontal(|ui| {
        ui.add_enabled(
            enabled,
            egui::Label::new(
                RichText::new(label)
                    .size(16.0)
                    .color(if enabled { TEXT } else { MUTED }),
            ),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let desired = egui::vec2(52.0, 30.0);
            let (rect, mut response) = ui.allocate_exact_size(
                desired,
                if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            if enabled && response.clicked() {
                *value = !*value;
                response.mark_changed();
            }

            let radius = rect.height() * 0.5;
            let fill = if *value { PURPLE_DARK } else { PANEL_ALT };
            let stroke = if *value {
                Stroke::new(1.0, PURPLE)
            } else {
                Stroke::new(1.0, TRACK)
            };
            ui.painter()
                .rect(rect, radius, fill, stroke, egui::StrokeKind::Inside);

            let knob_radius = 10.0;
            let knob_x = if *value {
                rect.right() - radius
            } else {
                rect.left() + radius
            };
            ui.painter().circle_filled(
                egui::pos2(knob_x, rect.center().y),
                knob_radius,
                if *value { PURPLE } else { MUTED },
            );
            toggle_response = Some(response);
        });
    });
    toggle_response.expect("pill toggle always allocates its switch")
}

pub(crate) fn overlay_slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
) -> Response {
    let min = *range.start();
    let max = *range.end();
    let desired = egui::vec2(ui.available_width().max(120.0), 34.0);
    let (rect, mut response) = ui.allocate_exact_size(desired, Sense::click_and_drag());

    if (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let t = ((pointer.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let next = min + (max - min) * t;
        if (*value - next).abs() > f32::EPSILON {
            *value = next;
            response.mark_changed();
        }
    }

    let t = if max > min {
        ((*value - min) / (max - min)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let track = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), 8.0));
    ui.painter().rect_filled(track, 4.0, TRACK);
    let fill = egui::Rect::from_min_max(
        track.left_top(),
        egui::pos2(track.left() + track.width() * t, track.bottom()),
    );
    if fill.width() > 0.0 {
        ui.painter().rect_filled(fill, 4.0, PURPLE);
    }
    let knob = egui::pos2(track.left() + track.width() * t, track.center().y);
    ui.painter().circle_filled(knob, 10.0, PURPLE);
    ui.painter().circle_filled(knob, 5.0, TEXT);

    response
}
