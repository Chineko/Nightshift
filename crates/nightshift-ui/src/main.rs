mod app;
mod bridge;
mod widgets;

use std::{
    env,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use chrono::{Local, Timelike};
use eframe::egui::{self, Color32, FontId, RichText, Stroke, TextStyle};
use nightshift_protocol::{
    AppState, ClientMessage, HrProviderStatus, InviteProfile, OverlaySpawnMode, PROTOCOL_VERSION,
    ServerMessage, SleepPhase, StateEvent, UiCommand, VrchatStatus,
};
use nightshift_ui::{invite_settings_ui::InviteSettingsUi, vrchat_auth_ui::VrchatAuthUi};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::mpsc as tokio_mpsc,
};

use app::NightshiftApp;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_title("Nightshift")
            .with_inner_size([980.0, 920.0])
            .with_min_inner_size([720.0, 700.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Nightshift",
        options,
        Box::new(|cc| {
            let mut style = (*cc.egui_ctx.style()).clone();
            style
                .text_styles
                .insert(TextStyle::Body, FontId::proportional(16.0));
            style
                .text_styles
                .insert(TextStyle::Button, FontId::proportional(16.0));
            style
                .text_styles
                .insert(TextStyle::Heading, FontId::proportional(24.0));
            style
                .text_styles
                .insert(TextStyle::Small, FontId::proportional(13.5));
            style.spacing.interact_size.y = 34.0;
            style.spacing.item_spacing = egui::vec2(10.0, 10.0);
            style.visuals.slider_trailing_fill = true;
            cc.egui_ctx.set_style(style);
            Ok(Box::new(NightshiftApp::new(cc.egui_ctx.clone())))
        }),
    )
}
