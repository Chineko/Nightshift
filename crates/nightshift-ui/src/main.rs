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

const BG: Color32 = Color32::from_rgb(12, 13, 17);
const PANEL: Color32 = Color32::from_rgb(23, 24, 30);
const PANEL_ALT: Color32 = Color32::from_rgb(30, 31, 38);
const MUTED: Color32 = Color32::from_rgb(157, 159, 170);
const PURPLE: Color32 = Color32::from_rgb(183, 141, 255);
const AMBER: Color32 = Color32::from_rgb(245, 173, 70);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Settings,
}

enum UiUpdate {
    Connected(bool),
    Message(ServerMessage),
}

struct Bridge {
    commands: tokio_mpsc::UnboundedSender<UiCommand>,
    updates: mpsc::Receiver<UiUpdate>,
}

impl Bridge {
    fn spawn(ctx: egui::Context) -> Self {
        let (command_tx, command_rx) = tokio_mpsc::unbounded_channel();
        let (update_tx, update_rx) = mpsc::channel();

        thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("failed to create Nightshift UI runtime");
            runtime.block_on(connection_loop(ctx, command_rx, update_tx));
        });

        Self {
            commands: command_tx,
            updates: update_rx,
        }
    }

    fn send(&self, command: UiCommand) {
        let _ = self.commands.send(command);
    }
}

fn socket_path() -> Option<PathBuf> {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|path| path.join("nightshift").join("nightshift.sock"))
}

fn discard_queued_commands(commands: &mut tokio_mpsc::UnboundedReceiver<UiCommand>) -> bool {
    loop {
        match commands.try_recv() {
            Ok(_) => {}
            Err(tokio_mpsc::error::TryRecvError::Empty) => return true,
            Err(tokio_mpsc::error::TryRecvError::Disconnected) => return false,
        }
    }
}

async fn connection_loop(
    ctx: egui::Context,
    mut commands: tokio_mpsc::UnboundedReceiver<UiCommand>,
    updates: mpsc::Sender<UiUpdate>,
) {
    loop {
        if !discard_queued_commands(&mut commands) {
            return;
        }

        let Some(path) = socket_path() else {
            let _ = updates.send(UiUpdate::Connected(false));
            ctx.request_repaint();
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        };

        let stream = match UnixStream::connect(&path).await {
            Ok(stream) => stream,
            Err(_) => {
                let _ = updates.send(UiUpdate::Connected(false));
                ctx.request_repaint();
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let hello = ClientMessage::Hello {
            protocol: PROTOCOL_VERSION,
            client: "nightshift-ui".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
        };

        if write_json_line(&mut write_half, &hello).await.is_err() {
            continue;
        }

        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => continue,
            Ok(_) => match serde_json::from_str::<ServerMessage>(line.trim_end()) {
                Ok(ServerMessage::Hello { protocol, .. }) if protocol == PROTOCOL_VERSION => {}
                Ok(ServerMessage::Error { .. }) | Ok(_) | Err(_) => continue,
            },
        }

        if updates.send(UiUpdate::Connected(true)).is_err() {
            return;
        }
        ctx.request_repaint();

        loop {
            line.clear();
            tokio::select! {
                read = reader.read_line(&mut line) => {
                    match read {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            if let Ok(message) = serde_json::from_str::<ServerMessage>(line.trim_end()) {
                                if updates.send(UiUpdate::Message(message)).is_err() {
                                    return;
                                }
                                ctx.request_repaint();
                            }
                        }
                    }
                }
                command = commands.recv() => {
                    let Some(command) = command else { return; };
                    let message = ClientMessage::Command { command };
                    if write_json_line(&mut write_half, &message).await.is_err() {
                        break;
                    }
                }
            }
        }

        let _ = updates.send(UiUpdate::Connected(false));
        ctx.request_repaint();
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn write_json_line(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    message: &ClientMessage,
) -> Result<(), serde_json::Error> {
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    if writer.write_all(&bytes).await.is_err() {
        return Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "Nightshift IPC disconnected",
        )));
    }
    Ok(())
}

struct NightshiftApp {
    page: Page,
    state: AppState,
    daemon_connected: bool,
    bridge: Bridge,
    vrchat_auth: VrchatAuthUi,
    invite_settings: InviteSettingsUi,
    brightness_send_at: Instant,
    hr_device_filter_draft: String,
}

impl NightshiftApp {
    fn new(ctx: egui::Context) -> Self {
        Self {
            page: Page::Home,
            state: AppState::default(),
            daemon_connected: false,
            bridge: Bridge::spawn(ctx.clone()),
            vrchat_auth: VrchatAuthUi::new(ctx),
            invite_settings: InviteSettingsUi::default(),
            brightness_send_at: Instant::now() - Duration::from_secs(1),
            hr_device_filter_draft: String::new(),
        }
    }

    fn send_command(&self, command: UiCommand) {
        if self.daemon_connected {
            self.bridge.send(command);
        }
    }

    fn set_full_state(&mut self, state: AppState) {
        self.hr_device_filter_draft = state.heart_rate.device_filter.clone().unwrap_or_default();
        self.state = state;
    }

    fn drain_updates(&mut self) {
        self.vrchat_auth.drain_updates();
        while let Ok(update) = self.bridge.updates.try_recv() {
            match update {
                UiUpdate::Connected(connected) => self.daemon_connected = connected,
                UiUpdate::Message(ServerMessage::FullState { state }) => self.set_full_state(state),
                UiUpdate::Message(ServerMessage::Event { event }) => self.apply_event(event),
                UiUpdate::Message(ServerMessage::Hello { .. }) => {}
                UiUpdate::Message(ServerMessage::Error { .. }) => {}
            }
        }
    }

    fn apply_event(&mut self, event: StateEvent) {
        match event {
            StateEvent::SleepChanged(state) => self.state.sleep = state,
            StateEvent::BrightnessChanged(state) => self.state.brightness = state,
            StateEvent::HeartRateChanged(state) => {
                let canonical = self.state.heart_rate.device_filter.as_deref().unwrap_or("");
                if self.hr_device_filter_draft.trim() == canonical {
                    self.hr_device_filter_draft = state.device_filter.clone().unwrap_or_default();
                }
                self.state.heart_rate = state;
            }
            StateEvent::OscChanged(state) => self.state.osc = state,
            StateEvent::VrchatChanged(state) => self.state.vrchat = state,
            StateEvent::InvitesChanged(state) => self.state.invites = state,
            StateEvent::OverlayChanged(state) => self.state.overlay = state,
            StateEvent::SettingsChanged(state) => self.state.settings = state,
            StateEvent::ConnectionChanged(state) => self.state.connection = state,
        }
    }

    fn greeting(&self) -> String {
        let phrase = match Local::now().hour() {
            5..=11 => "Good morning",
            12..=17 => "Good afternoon",
            _ => "Good evening",
        };

        match self.state.vrchat.display_name.as_deref() {
            Some(name) if self.state.vrchat.connected => format!("{phrase}, {name}."),
            _ => format!("{phrase}."),
        }
    }

    fn nav(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(RichText::new("NIGHTSHIFT").strong().size(20.0));
            ui.add_space(28.0);
            if ui.selectable_label(self.page == Page::Home, "Home").clicked() {
                self.page = Page::Home;
            }
            if ui.selectable_label(self.page == Page::Settings, "Settings").clicked() {
                self.page = Page::Settings;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let status = if self.daemon_connected { "Connected" } else { "Disconnected" };
                let color = if self.daemon_connected { Color32::from_rgb(102, 220, 145) } else { MUTED };
                ui.label(RichText::new(format!("● {status}")).color(color));
            });
        });
        ui.separator();
    }

    fn sleep_card(&mut self, ui: &mut egui::Ui) {
        let sleeping = matches!(self.state.sleep.phase, SleepPhase::Sleeping | SleepPhase::EnteringSleep);
        let accent = if sleeping { PURPLE } else { AMBER };
        let title = match self.state.sleep.phase {
            SleepPhase::Awake => "Awake",
            SleepPhase::EnteringSleep => "Entering sleep mode…",
            SleepPhase::Sleeping => "Sleep mode active",
            SleepPhase::Waking => "Waking up…",
        };

        egui::Frame::new()
            .fill(PANEL)
            .stroke(Stroke::new(1.0, Color32::from_gray(48)))
            .corner_radius(16)
            .inner_margin(20)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new(if sleeping { "☾  Sleep mode" } else { "☀  Sleep mode" }).color(accent).strong().size(18.0));
                ui.add_space(12.0);
                ui.label(RichText::new(title).size(26.0).strong());
                ui.label(RichText::new(if sleeping {
                    "Your display is dimmed and sleep automation is active."
                } else {
                    "Dimming, status and invite automation are ready."
                }).color(MUTED));
                ui.add_space(16.0);

                let button_text = if sleeping { "Wake up" } else { "Enter sleep mode" };
                if ui.add_sized([ui.available_width(), 38.0], egui::Button::new(RichText::new(button_text).strong()).fill(accent)).clicked() {
                    self.send_command(if sleeping { UiCommand::Wake } else { UiCommand::EnterSleep });
                }

                ui.add_space(16.0);
                ui.separator();
                status_row(ui, "Current brightness", &format!("{}%", (self.state.brightness.current * 100.0).round() as u32));
                status_row(ui, "Sleep target", &format!("{}%", (self.state.brightness.sleep_target * 100.0).round() as u32));
                status_row(ui, "Invite policy", invite_profile_name(self.state.invites.profile));
                status_row(ui, "Automation", if sleeping { "Active" } else { "Ready" });
            });
    }

    fn brightness_card(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new().fill(PANEL).corner_radius(16).inner_margin(20).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Brightness").strong().size(17.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{}%", (self.state.brightness.current * 100.0).round() as u32)).color(PURPLE).strong());
                });
            });
            ui.add_space(8.0);
            let mut value = self.state.brightness.current;
            let response = ui.add(egui::Slider::new(&mut value, 0.02..=1.0).show_value(false));

            if response.changed() {
                self.state.brightness.current = value;
                if !response.drag_stopped() && self.brightness_send_at.elapsed() >= Duration::from_millis(40) {
                    self.send_command(UiCommand::SetBrightness(value));
                    self.brightness_send_at = Instant::now();
                }
            }
            if response.drag_stopped() {
                self.send_command(UiCommand::SetBrightness(self.state.brightness.current));
                self.brightness_send_at = Instant::now();
            }

            let sync = if self.state.brightness.manual_override { "Manual override" } else if self.state.brightness.transitioning { "Fading" } else { "Synced" };
            ui.label(RichText::new(sync).color(MUTED).small());
        });
    }

    fn glance_cards(&self, ui: &mut egui::Ui) {
        let heart_rate = self
            .state
            .heart_rate
            .bpm
            .map(|bpm| format!("{bpm} BPM"))
            .unwrap_or_else(|| hr_provider_status_name(self.state.heart_rate.provider_status).into());
        ui.columns(2, |columns| {
            small_card(&mut columns[0], "♥  Heart rate", &heart_rate);
            small_card(&mut columns[1], "≋  OSC automation", if self.state.osc.enabled { "Running" } else { "Off" });
        });
        ui.add_space(8.0);
        ui.columns(2, |columns| {
            small_card(&mut columns[0], "●  VRChat status", vrchat_status_name(self.state.vrchat.status));
            small_card(&mut columns[1], "◎  Invite rules", invite_profile_name(self.state.invites.profile));
        });
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new(self.greeting()).size(28.0).strong());
        ui.label(RichText::new("Settle in. Nightshift is ready when you are.").color(MUTED));
        ui.add_space(20.0);
        self.sleep_card(ui);
        ui.add_space(12.0);
        self.brightness_card(ui);
        ui.add_space(16.0);
        ui.label(RichText::new("At a glance").strong().size(18.0));
        ui.add_space(8.0);
        self.glance_cards(ui);
    }

    fn heart_rate_settings(&mut self, ui: &mut egui::Ui) {
        let mut monitoring = self.state.heart_rate.enabled;
        if ui.checkbox(&mut monitoring, "Heart-rate monitoring").changed() {
            self.send_command(UiCommand::SetHrMonitoring(monitoring));
        }

        status_row(ui, "Provider", hr_provider_status_name(self.state.heart_rate.provider_status));
        status_row(
            ui,
            "Connected device",
            self.state.heart_rate.connected_device.as_deref().unwrap_or("—"),
        );
        if let Some(error) = self.state.heart_rate.last_error.as_deref() {
            ui.label(RichText::new(error).color(AMBER).small());
        }

        ui.add_space(8.0);
        ui.label("Discovered devices");
        let devices = self.state.heart_rate.available_devices.clone();
        if devices.is_empty() {
            let message = if self.state.heart_rate.provider_status == HrProviderStatus::Searching {
                "Scanning for BLE heart-rate devices…"
            } else {
                "No devices from the latest scan."
            };
            ui.label(RichText::new(message).color(MUTED).small());
        } else {
            for device in devices {
                let label = match device.name.as_deref() {
                    Some(name) => format!("{name}  ·  {}", device.address),
                    None => device.address.clone(),
                };
                let selected = self.state.heart_rate.device_filter.as_deref() == Some(device.id.as_str());
                if ui.selectable_label(selected, label).clicked() {
                    self.hr_device_filter_draft = device.id.clone();
                    self.send_command(UiCommand::SetHrDeviceFilter(Some(device.id)));
                }
            }
        }

        ui.add_space(8.0);
        ui.label("Preferred BLE device");
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.hr_device_filter_draft)
                    .hint_text("Auto-select, name, address, or provider ID"),
            );
            let canonical = self.state.heart_rate.device_filter.as_deref().unwrap_or("");
            let draft = self.hr_device_filter_draft.trim();
            let changed = draft != canonical;
            if ui.add_enabled(changed, egui::Button::new("Apply")).clicked()
                || (response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) && changed)
            {
                self.send_command(UiCommand::SetHrDeviceFilter(
                    (!draft.is_empty()).then(|| draft.to_string()),
                ));
            }
            if ui.button("Auto").clicked() {
                self.hr_device_filter_draft.clear();
                self.send_command(UiCommand::SetHrDeviceFilter(None));
            }
        });
        ui.label(
            RichText::new("Choose a discovered device or use a name, Bluetooth address, or provider ID. Empty means auto-select.")
                .color(MUTED)
                .small(),
        );

        ui.add_space(8.0);
        let mut threshold = self.state.heart_rate.threshold_bpm;
        ui.label("Sleep threshold");
        if ui
            .add(egui::Slider::new(&mut threshold, 20..=240).suffix(" BPM"))
            .changed()
        {
            self.send_command(UiCommand::SetHrThreshold(threshold));
        }

        let mut calm = self.state.heart_rate.calm_period_seconds;
        ui.label("Calm period below threshold");
        if ui
            .add(egui::Slider::new(&mut calm, 10..=3_600).suffix(" s"))
            .changed()
        {
            self.send_command(UiCommand::SetHrCalmPeriod(calm));
        }

        let mut cooldown = self.state.heart_rate.cooldown_seconds;
        ui.label("Manual-wake cooldown");
        if ui
            .add(egui::Slider::new(&mut cooldown, 0..=24 * 60 * 60).suffix(" s"))
            .changed()
        {
            self.send_command(UiCommand::SetHrCooldown(cooldown));
        }
        ui.label(
            RichText::new("Changing HR tuning resets calm/retrigger progress but preserves the manual-wake safety cooldown.")
                .color(MUTED)
                .small(),
        );
    }

    fn overlay_settings(&mut self, ui: &mut egui::Ui) {
        status_row(
            ui,
            "Runtime",
            if self.state.overlay.runtime_connected {
                "Connected"
            } else {
                "Disconnected"
            },
        );
        status_row(
            ui,
            "Runtime version",
            self.state
                .overlay
                .runtime_client_version
                .as_deref()
                .unwrap_or("—"),
        );

        ui.add_space(8.0);
        let visibility_label = if self.state.overlay.visible {
            "Hide overlay"
        } else {
            "Show overlay"
        };
        if ui.button(visibility_label).clicked() {
            self.send_command(if self.state.overlay.visible {
                UiCommand::HideOverlay
            } else {
                UiCommand::ShowOverlay
            });
        }

        ui.add_space(8.0);
        let mut distance = self.state.overlay.spawn_distance_m;
        ui.label("Spawn distance");
        if ui
            .add(egui::Slider::new(&mut distance, 0.25..=5.0).suffix(" m"))
            .changed()
        {
            self.send_command(UiCommand::SetOverlaySpawnDistance(distance));
        }

        let mut mode = self.state.overlay.spawn_mode;
        let before = mode;
        ui.horizontal(|ui| {
            ui.label("Spawn mode");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt("overlay_spawn_mode")
                    .selected_text(overlay_spawn_mode_name(mode))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut mode, OverlaySpawnMode::InFront, "In front");
                        ui.selectable_value(
                            &mut mode,
                            OverlaySpawnMode::RestoreLastPosition,
                            "Restore last position",
                        );
                    });
            });
        });
        if mode != before {
            self.send_command(UiCommand::SetOverlaySpawnMode(mode));
        }

        let mut laser_enabled = self.state.overlay.laser_enabled;
        if ui.checkbox(&mut laser_enabled, "Show controller laser guide").changed() {
            self.send_command(UiCommand::SetOverlayLaserEnabled(laser_enabled));
        }
        ui.label(
            RichText::new("The surface cursor stays visible when the laser guide is off.")
                .color(MUTED)
                .small(),
        );

        ui.label(
            RichText::new("Placement settings are persisted by the daemon; runtime visibility stays session state.")
                .color(MUTED)
                .small(),
        );
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Advanced settings");
        ui.label(RichText::new("Full Nightshift configuration lives here. The VR overlay keeps only quick, headset-friendly controls.").color(MUTED));
        ui.add_space(16.0);

        setting_card(ui, "Sleep", |ui| {
            let mut detection = self.state.sleep.detection_enabled;
            if ui.checkbox(&mut detection, "Sleep detection").changed() {
                self.send_command(UiCommand::SetSleepDetection(detection));
            }
            let mut target = self.state.brightness.sleep_target;
            ui.label("Sleep brightness target");
            if ui
                .add(
                    egui::Slider::new(&mut target, 0.02..=1.0)
                        .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
                )
                .changed()
            {
                self.send_command(UiCommand::SetSleepBrightness(target));
            }
            let mut fade = self.state.settings.fade_duration_seconds;
            ui.label("Fade duration");
            if ui.add(egui::Slider::new(&mut fade, 0.0..=120.0).suffix(" s")).changed() {
                self.send_command(UiCommand::SetFadeDuration(fade));
            }
        });

        ui.add_space(12.0);
        setting_card(ui, "Heart rate", |ui| self.heart_rate_settings(ui));

        ui.add_space(12.0);
        setting_card(ui, "VRChat", |ui| {
            let runtime_name = if self.state.vrchat.connected {
                self.state.vrchat.display_name.as_deref()
            } else {
                None
            };
            self.vrchat_auth.render(ui, runtime_name);

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            let mut status_automation = self.state.settings.vrchat_status_automation_enabled;
            if ui
                .checkbox(&mut status_automation, "Sleep/wake status automation")
                .changed()
            {
                self.send_command(UiCommand::SetVrchatStatusAutomation(status_automation));
            }
            ui.label(
                RichText::new(
                    "Off by default. When enabled, Nightshift changes only your configured VRChat presence on sleep/wake edges.",
                )
                .color(MUTED)
                .small(),
            );

            let mut sleep_status = self.state.settings.sleep_vrchat_status;
            if vrchat_status_picker(ui, "Sleep status", "vrchat_sleep_status", &mut sleep_status) {
                self.send_command(UiCommand::SetVrchatSleepStatus(sleep_status));
            }

            let mut wake_status = self.state.settings.wake_vrchat_status;
            if vrchat_status_picker(ui, "Wake status", "vrchat_wake_status", &mut wake_status) {
                self.send_command(UiCommand::SetVrchatWakeStatus(wake_status));
            }
        });

        ui.add_space(12.0);
        setting_card(ui, "Automation", |ui| {
            let mut osc = self.state.osc.enabled;
            if ui.checkbox(&mut osc, "OSC automation").changed() {
                self.send_command(UiCommand::SetOscAutomation(osc));
            }

            let mut gogo = self.state.settings.gogo_automation_enabled;
            let gogo_response = ui.add_enabled(
                osc,
                egui::Checkbox::new(&mut gogo, "GoGo Loco automation"),
            );
            if gogo_response.changed() {
                self.send_command(UiCommand::SetGogoAutomation(gogo));
            }
            ui.label(
                RichText::new(if osc {
                    "Off by default. Allows Nightshift to run configured GoGo pose/feet-lock sequences."
                } else {
                    "Enable OSC automation first. GoGo automation remains off by default."
                })
                .color(MUTED)
                .small(),
            );
        });

        ui.add_space(12.0);
        setting_card(ui, "Overlay", |ui| self.overlay_settings(ui));

        ui.add_space(12.0);
        setting_card(ui, "Invite automation & access lists", |ui| {
            let commands = self.invite_settings.render(ui, &self.state.invites);
            for command in commands {
                self.send_command(command);
            }
        });

        ui.add_space(12.0);
        setting_card(ui, "Diagnostics", |ui| {
            status_row(ui, "Daemon IPC", if self.daemon_connected { "Connected" } else { "Disconnected" });
            status_row(ui, "Overlay runtime", if self.state.overlay.runtime_connected { "Connected" } else { "Disconnected" });
            status_row(ui, "OSC runtime", if self.state.osc.connected { "Connected" } else { "Waiting" });
            status_row(ui, "Heart-rate provider", hr_provider_status_name(self.state.heart_rate.provider_status));
            status_row(ui, "VRChat", if self.state.vrchat.connected { "Connected" } else { "Offline" });
            ui.add_space(6.0);
            ui.label(RichText::new("Use this desktop page for device selection, thresholds, placement, laser behavior, automations, access lists, and runtime health.").color(MUTED).small());
        });
    }
}

impl eframe::App for NightshiftApp {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_updates();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.visuals_mut().dark_mode = true;
        ui.visuals_mut().panel_fill = BG;
        ui.visuals_mut().override_text_color = Some(Color32::from_rgb(238, 238, 242));
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);

        egui::Frame::central_panel(ui.style())
            .fill(BG)
            .inner_margin(24)
            .show(ui, |ui| {
                self.nav(ui);
                egui::ScrollArea::vertical().show(ui, |ui| match self.page {
                    Page::Home => self.home(ui),
                    Page::Settings => self.settings(ui),
                });
            });
    }
}

fn status_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).strong());
        });
    });
}

fn small_card(ui: &mut egui::Ui, title: &str, value: &str) {
    egui::Frame::new().fill(PANEL_ALT).corner_radius(14).inner_margin(16).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(RichText::new(title).color(MUTED));
        ui.add_space(6.0);
        ui.label(RichText::new(value).strong().size(18.0));
    });
}

fn setting_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(PANEL).corner_radius(16).inner_margin(20).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(RichText::new(title).strong().size(18.0));
        ui.add_space(10.0);
        add_contents(ui);
    });
}

fn vrchat_status_picker(
    ui: &mut egui::Ui,
    label: &str,
    id: &str,
    status: &mut VrchatStatus,
) -> bool {
    let before = *status;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::ComboBox::from_id_salt(id)
                .selected_text(vrchat_status_name(*status))
                .show_ui(ui, |ui| {
                    ui.selectable_value(status, VrchatStatus::Active, "Active");
                    ui.selectable_value(status, VrchatStatus::JoinMe, "Join Me");
                    ui.selectable_value(status, VrchatStatus::AskMe, "Ask Me");
                    ui.selectable_value(status, VrchatStatus::Busy, "Busy");
                    ui.selectable_value(status, VrchatStatus::Offline, "Offline");
                });
        });
    });
    before != *status
}

fn hr_provider_status_name(status: HrProviderStatus) -> &'static str {
    match status {
        HrProviderStatus::Disabled => "Disabled",
        HrProviderStatus::Searching => "Searching",
        HrProviderStatus::Connected => "Connected",
        HrProviderStatus::Error => "Error",
        HrProviderStatus::Unknown => "Unknown",
    }
}

fn overlay_spawn_mode_name(mode: OverlaySpawnMode) -> &'static str {
    match mode {
        OverlaySpawnMode::InFront => "In front",
        OverlaySpawnMode::RestoreLastPosition => "Restore last position",
    }
}

fn invite_profile_name(profile: InviteProfile) -> &'static str {
    match profile {
        InviteProfile::Disabled => "Disabled",
        InviteProfile::Friends => "Friends",
        InviteProfile::Favorites => "Favorites",
        InviteProfile::Whitelist => "Whitelist",
        InviteProfile::Custom => "Custom",
    }
}

fn vrchat_status_name(status: VrchatStatus) -> &'static str {
    match status {
        VrchatStatus::Active => "Active",
        VrchatStatus::JoinMe => "Join Me",
        VrchatStatus::AskMe => "Ask Me",
        VrchatStatus::Busy => "Busy",
        VrchatStatus::Offline => "Offline",
    }
}

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
