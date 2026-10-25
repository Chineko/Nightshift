mod app;
mod bridge;

use std::{
    env,
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use chrono::{Local, Timelike};
use eframe::egui::{self, Color32, RichText, Stroke};
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
            .with_inner_size([920.0, 900.0])
            .with_min_inner_size([680.0, 680.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Nightshift",
        options,
        Box::new(|cc| Ok(Box::new(NightshiftApp::new(cc.egui_ctx.clone())))),
    )
}
