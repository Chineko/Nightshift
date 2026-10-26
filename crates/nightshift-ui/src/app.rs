use super::*;
use crate::{
    bridge::{Bridge, UiUpdate},
    widgets::{overlay_slider, pill_toggle},
};
use nightshift_ui::notifications::{toast_for_event, Toast, ToastQueue, ToastSeverity};

const BG: Color32 = Color32::from_rgb(12, 13, 17);
const PANEL: Color32 = Color32::from_rgb(23, 24, 30);
const PANEL_ALT: Color32 = Color32::from_rgb(30, 31, 38);
const BORDER: Color32 = Color32::from_rgb(48, 52, 66);
const MUTED: Color32 = Color32::from_rgb(157, 159, 170);
const PURPLE: Color32 = Color32::from_rgb(183, 141, 255);
const PURPLE_DARK: Color32 = Color32::from_rgb(62, 50, 83);
const AMBER: Color32 = Color32::from_rgb(245, 173, 70);
const GREEN: Color32 = Color32::from_rgb(102, 220, 145);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Settings,
}

pub(crate) struct NightshiftApp {
    page: Page,
    state: AppState,
    daemon_connected: bool,
    bridge: Bridge,
    vrchat_auth: VrchatAuthUi,
    invite_settings: InviteSettingsUi,
    brightness_send_at: Instant,
    hr_device_filter_draft: String,
    toasts: ToastQueue,
    toast_tick_at: Instant,
}

impl NightshiftApp {
    pub(crate) fn new(ctx: egui::Context) -> Self {
        Self {
            page: Page::Home,
            state: AppState::default(),
            daemon_connected: false,
            bridge: Bridge::spawn(ctx.clone()),
            vrchat_auth: VrchatAuthUi::new(ctx),
            invite_settings: InviteSettingsUi::default(),
            brightness_send_at: Instant::now() - Duration::from_secs(1),
            hr_device_filter_draft: String::new(),
            toasts: ToastQueue::default(),
            toast_tick_at: Instant::now(),
        }
    }

    fn send_command(&self, command: UiCommand) {
        if self.daemon_connected {
            self.bridge.send(command);
        }
    }

    fn sleeping(&self) -> bool {
        matches!(
            self.state.sleep.phase,
            SleepPhase::EnteringSleep | SleepPhase::Sleeping
        )
    }

    fn push_toast(&mut self, toast: Toast) {
        self.toasts.push(toast, self.sleeping());
    }

    fn push_event_toast(&mut self, event: &StateEvent) {
        if let Some(toast) = toast_for_event(&self.state, event) {
            let sleeping = if matches!(event, StateEvent::SleepChanged(_)) {
                false
            } else {
                self.sleeping()
            };
            self.toasts.push(toast, sleeping);
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
                UiUpdate::Connected(connected) => {
                    if self.daemon_connected && !connected {
                        self.push_toast(Toast::error(
                            "Nightshift disconnected",
                            "Desktop controls are waiting for the daemon to reconnect.",
                        ));
                    }
                    self.daemon_connected = connected;
                }
                UiUpdate::Message(ServerMessage::FullState { state }) => self.set_full_state(state),
                UiUpdate::Message(ServerMessage::Event { event }) => {
                    self.push_event_toast(&event);
                    self.apply_event(event);
                }
                UiUpdate::Message(ServerMessage::Hello { .. }) => {}
                UiUpdate::Message(ServerMessage::Error { message }) => {
                    self.push_toast(Toast::error("Nightshift", message));
                }
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

    fn tick_toasts(&mut self) {
        let now = Instant::now();
        self.toasts
            .tick(now.saturating_duration_since(self.toast_tick_at));
        self.toast_tick_at = now;
    }

    fn render_toasts(&mut self, ctx: &egui::Context) {
        if self.toasts.is_empty() {
            return;
        }

        let mut dismiss = None;
        egui::Area::new(egui::Id::unique("nightshift_toasts"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-18.0, 18.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_width(340.0);
                ui.vertical(|ui| {
                    for (index, toast) in self.toasts.visible().enumerate() {
                        let accent = match toast.severity {
                            ToastSeverity::Info => PURPLE,
                            ToastSeverity::Warning => AMBER,
                            ToastSeverity::Error => Color32::from_rgb(238, 106, 106),
                        };

                        egui::Frame::new()
                            .fill(PANEL)
                            .stroke(Stroke::new(1.0, accent))
                            .corner_radius(14)
                            .inner_margin(14)
                            .show(ui, |ui| {
                                ui.set_min_width(310.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(&toast.title)
                                            .strong()
                                            .size(16.0)
                                            .color(accent),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.small_button("×").clicked() {
                                                dismiss = Some(index);
                                            }
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(&toast.message)
                                        .size(15.0)
                                        .color(MUTED),
                                );
                            });
                    }
                });
            });

        if let Some(index) = dismiss {
            self.toasts.dismiss(index);
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
        egui::Frame::new()
            .fill(PANEL)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(18)
            .inner_margin(egui::Margin::symmetric(18, 12))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("☾").size(23.0).color(PURPLE));
                    ui.label(RichText::new("NIGHTSHIFT").strong().size(22.0));
                    ui.add_space(20.0);
                    if nav_button(ui, "HOME", self.page == Page::Home).clicked() {
                        self.page = Page::Home;
                    }
                    if nav_button(ui, "SETTINGS", self.page == Page::Settings).clicked() {
                        self.page = Page::Settings;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let status = if self.daemon_connected {
                            "LIVE"
                        } else {
                            "OFFLINE"
                        };
                        let color = if self.daemon_connected { GREEN } else { MUTED };
                        ui.label(
                            RichText::new(format!("●  {status}"))
                                .strong()
                                .size(14.0)
                                .color(color),
                        );
                    });
                });
            });
    }

    fn sleep_card(&mut self, ui: &mut egui::Ui) {
        let sleeping = matches!(
            self.state.sleep.phase,
            SleepPhase::Sleeping | SleepPhase::EnteringSleep
        );
        let accent = if sleeping { PURPLE } else { AMBER };
        let title = match self.state.sleep.phase {
            SleepPhase::Awake => "Awake",
            SleepPhase::EnteringSleep => "Entering sleep mode…",
            SleepPhase::Sleeping => "Sleep mode active",
            SleepPhase::Waking => "Waking up…",
        };

        overlay_card(ui, |ui| {
            ui.label(
                RichText::new(if sleeping {
                    "☾  SLEEP MODE"
                } else {
                    "☀  SLEEP MODE"
                })
                .color(accent)
                .strong()
                .size(16.0),
            );
            ui.add_space(8.0);
            ui.label(RichText::new(title).size(30.0).strong());
            ui.label(
                RichText::new(if sleeping {
                    "Your display is dimmed and sleep automation is active."
                } else {
                    "Dimming, status and invite automation are ready."
                })
                .size(16.0)
                .color(MUTED),
            );
            ui.add_space(16.0);

            let button_text = if sleeping {
                "WAKE UP"
            } else {
                "ENTER SLEEP MODE"
            };
            if ui
                .add_sized(
                    [ui.available_width(), 48.0],
                    egui::Button::new(RichText::new(button_text).strong().size(16.0))
                        .fill(PURPLE)
                        .corner_radius(16),
                )
                .clicked()
            {
                self.send_command(if sleeping {
                    UiCommand::Wake
                } else {
                    UiCommand::EnterSleep
                });
            }

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(6.0);
            status_row(
                ui,
                "Current brightness",
                &format!(
                    "{}%",
                    (self.state.brightness.current * 100.0).round() as u32
                ),
            );
            status_row(
                ui,
                "Sleep target",
                &format!(
                    "{}%",
                    (self.state.brightness.sleep_target * 100.0).round() as u32
                ),
            );
            status_row(
                ui,
                "Invite policy",
                invite_profile_name(self.state.invites.profile),
            );
            status_row(ui, "Automation", if sleeping { "Active" } else { "Ready" });
        });
    }

    fn brightness_card(&mut self, ui: &mut egui::Ui) {
        overlay_card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("BRIGHTNESS").strong().size(17.0));
                    ui.label(
                        RichText::new("OVERLAY DISPLAY LEVEL")
                            .size(13.5)
                            .color(MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{}%",
                            (self.state.brightness.current * 100.0).round() as u32
                        ))
                        .size(25.0)
                        .strong(),
                    );
                });
            });
            ui.add_space(12.0);
            let mut value = self.state.brightness.current;
            let response = overlay_slider(ui, &mut value, 0.02..=1.0);

            if response.changed() {
                self.state.brightness.current = value;
                if !response.drag_stopped()
                    && self.brightness_send_at.elapsed() >= Duration::from_millis(24)
                {
                    self.send_command(UiCommand::SetBrightness(value));
                    self.brightness_send_at = Instant::now();
                }
            }
            if response.drag_stopped() {
                self.send_command(UiCommand::SetBrightness(self.state.brightness.current));
                self.brightness_send_at = Instant::now();
            }

            ui.add_space(4.0);
            let sync = if self.state.brightness.manual_override {
                "MANUAL OVERRIDE"
            } else if self.state.brightness.transitioning {
                "FADING"
            } else {
                "SYNCED"
            };
            ui.label(
                RichText::new(sync)
                    .size(13.0)
                    .strong()
                    .color(if self.state.brightness.transitioning {
                        AMBER
                    } else {
                        GREEN
                    }),
            );
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
            small_card(&mut columns[0], "♥  HEART RATE", &heart_rate);
            small_card(
                &mut columns[1],
                "≋  OSC AUTOMATION",
                if self.state.osc.enabled {
                    "Running"
                } else {
                    "Off"
                },
            );
        });
        ui.add_space(10.0);
        ui.columns(2, |columns| {
            small_card(
                &mut columns[0],
                "●  VRCHAT STATUS",
                vrchat_status_name(self.state.vrchat.status),
            );
            small_card(
                &mut columns[1],
                "◎  INVITE RULES",
                invite_profile_name(self.state.invites.profile),
            );
        });
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("VR OVERLAY").size(14.0).strong().color(PURPLE));
        ui.label(RichText::new(self.greeting()).size(32.0).strong());
        ui.label(
            RichText::new("Settle in. Nightshift is ready when you are.")
                .size(16.0)
                .color(MUTED),
        );
        ui.add_space(20.0);
        self.sleep_card(ui);
        ui.add_space(14.0);
        self.brightness_card(ui);
        ui.add_space(18.0);
        ui.label(RichText::new("AT A GLANCE").strong().size(18.0));
        ui.add_space(10.0);
        self.glance_cards(ui);
    }

    fn heart_rate_settings(&mut self, ui: &mut egui::Ui) {
        let mut monitoring = self.state.heart_rate.enabled;
        if pill_toggle(ui, "Heart-rate monitoring", &mut monitoring, true).changed() {
            self.send_command(UiCommand::SetHrMonitoring(monitoring));
        }

        status_row(
            ui,
            "Provider",
            hr_provider_status_name(self.state.heart_rate.provider_status),
        );
        status_row(
            ui,
            "Connected device",
            self.state
                .heart_rate
                .connected_device
                .as_deref()
                .unwrap_or("—"),
        );
        if let Some(error) = self.state.heart_rate.last_error.as_deref() {
            ui.label(RichText::new(error).color(AMBER).size(14.0));
        }

        ui.add_space(10.0);
        ui.label(RichText::new("Discovered devices").strong());
        let devices = self.state.heart_rate.available_devices.clone();
        if devices.is_empty() {
            let message = if self.state.heart_rate.provider_status == HrProviderStatus::Searching {
                "Scanning for BLE heart-rate devices…"
            } else {
                "No devices from the latest scan."
            };
            ui.label(RichText::new(message).color(MUTED).size(14.0));
        } else {
            for device in devices {
                let label = match device.name.as_deref() {
                    Some(name) => format!("{name}  ·  {}", device.address),
                    None => device.address.clone(),
                };
                let selected =
                    self.state.heart_rate.device_filter.as_deref() == Some(device.id.as_str());
                if ui.selectable_label(selected, label).clicked() {
                    self.hr_device_filter_draft = device.id.clone();
                    self.send_command(UiCommand::SetHrDeviceFilter(Some(device.id)));
                }
            }
        }

        ui.add_space(10.0);
        ui.label(RichText::new("Preferred BLE device").strong());
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.hr_device_filter_draft)
                    .hint_text("Auto-select, name, address, or provider ID"),
            );
            let canonical = self.state.heart_rate.device_filter.as_deref().unwrap_or("");
            let draft = self.hr_device_filter_draft.trim();
            let changed = draft != canonical;
            if ui
                .add_enabled(changed, egui::Button::new("Apply"))
                .clicked()
                || (response.lost_focus()
                    && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    && changed)
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
            RichText::new(
                "Choose a discovered device or use a name, Bluetooth address, or provider ID. Empty means auto-select.",
            )
            .color(MUTED)
            .size(14.0),
        );

        ui.add_space(12.0);
        let mut threshold = self.state.heart_rate.threshold_bpm as f32;
        slider_label(ui, "Sleep threshold", format!("{:.0} BPM", threshold));
        if overlay_slider(ui, &mut threshold, 20.0..=240.0).changed() {
            self.send_command(UiCommand::SetHrThreshold(threshold.round() as u16));
        }

        let mut calm = self.state.heart_rate.calm_period_seconds as f32;
        slider_label(ui, "Calm period below threshold", format!("{:.0} s", calm));
        if overlay_slider(ui, &mut calm, 10.0..=3_600.0).changed() {
            self.send_command(UiCommand::SetHrCalmPeriod(calm.round() as u64));
        }

        let mut cooldown = self.state.heart_rate.cooldown_seconds as f32;
        slider_label(ui, "Manual-wake cooldown", format!("{:.0} s", cooldown));
        if overlay_slider(ui, &mut cooldown, 0.0..=(24 * 60 * 60) as f32).changed() {
            self.send_command(UiCommand::SetHrCooldown(cooldown.round() as u64));
        }
        ui.label(
            RichText::new(
                "Changing HR tuning resets calm/retrigger progress but preserves the manual-wake safety cooldown.",
            )
            .color(MUTED)
            .size(14.0),
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

        ui.add_space(10.0);
        let visibility_label = if self.state.overlay.visible {
            "Hide overlay"
        } else {
            "Show overlay"
        };
        if ui
            .add_sized(
                [160.0, 38.0],
                egui::Button::new(visibility_label).corner_radius(12),
            )
            .clicked()
        {
            self.send_command(if self.state.overlay.visible {
                UiCommand::HideOverlay
            } else {
                UiCommand::ShowOverlay
            });
        }

        ui.add_space(10.0);
        let mut distance = self.state.overlay.spawn_distance_m;
        slider_label(ui, "Spawn distance", format!("{distance:.2} m"));
        if overlay_slider(ui, &mut distance, 0.25..=5.0).changed() {
            self.send_command(UiCommand::SetOverlaySpawnDistance(distance));
        }

        let mut mode = self.state.overlay.spawn_mode;
        let before = mode;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Spawn mode").size(16.0));
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
        if pill_toggle(
            ui,
            "Show controller laser guide",
            &mut laser_enabled,
            true,
        )
        .changed()
        {
            self.send_command(UiCommand::SetOverlayLaserEnabled(laser_enabled));
        }
        ui.label(
            RichText::new("The surface cursor stays visible when the laser guide is off.")
                .color(MUTED)
                .size(14.0),
        );
        ui.label(
            RichText::new(
                "Placement settings are persisted by the daemon; runtime visibility stays session state.",
            )
            .color(MUTED)
            .size(14.0),
        );
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("DESKTOP CONTROL CENTER").size(14.0).strong().color(PURPLE));
        ui.label(RichText::new("Settings").size(32.0).strong());
        ui.label(
            RichText::new(
                "The same Nightshift controls as the VR panel, with the deeper tuning kept here.",
            )
            .size(16.0)
            .color(MUTED),
        );
        ui.add_space(18.0);

        setting_card(ui, "SLEEP", |ui| {
            let mut detection = self.state.sleep.detection_enabled;
            if pill_toggle(ui, "Sleep detection", &mut detection, true).changed() {
                self.send_command(UiCommand::SetSleepDetection(detection));
            }

            let mut target = self.state.brightness.sleep_target;
            slider_label(
                ui,
                "Sleep brightness target",
                format!("{:.0}%", target * 100.0),
            );
            if overlay_slider(ui, &mut target, 0.02..=1.0).changed() {
                self.send_command(UiCommand::SetSleepBrightness(target));
            }

            let mut fade = self.state.settings.fade_duration_seconds;
            slider_label(ui, "Fade duration", format!("{fade:.1} s"));
            if overlay_slider(ui, &mut fade, 0.0..=120.0).changed() {
                self.send_command(UiCommand::SetFadeDuration(fade));
            }
        });

        ui.add_space(14.0);
        setting_card(ui, "HEART RATE", |ui| self.heart_rate_settings(ui));

        ui.add_space(14.0);
        setting_card(ui, "VRCHAT", |ui| {
            let runtime_name = if self.state.vrchat.connected {
                self.state.vrchat.display_name.as_deref()
            } else {
                None
            };
            self.vrchat_auth.render(ui, runtime_name);

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(10.0);

            let mut status_automation = self.state.settings.vrchat_status_automation_enabled;
            if pill_toggle(
                ui,
                "Sleep/wake status automation",
                &mut status_automation,
                true,
            )
            .changed()
            {
                self.send_command(UiCommand::SetVrchatStatusAutomation(status_automation));
            }
            ui.label(
                RichText::new(
                    "Off by default. When enabled, Nightshift changes only your configured VRChat presence on sleep/wake edges.",
                )
                .color(MUTED)
                .size(14.0),
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

        ui.add_space(14.0);
        setting_card(ui, "AUTOMATION", |ui| {
            let mut osc = self.state.osc.enabled;
            if pill_toggle(ui, "OSC automation", &mut osc, true).changed() {
                self.send_command(UiCommand::SetOscAutomation(osc));
            }

            let mut gogo = self.state.settings.gogo_automation_enabled;
            if pill_toggle(ui, "GoGo Loco automation", &mut gogo, osc).changed() {
                self.send_command(UiCommand::SetGogoAutomation(gogo));
            }
            ui.label(
                RichText::new(if osc {
                    "Off by default. Allows Nightshift to run configured GoGo pose/feet-lock sequences."
                } else {
                    "Enable OSC automation first. GoGo automation remains off by default."
                })
                .color(MUTED)
                .size(14.0),
            );
        });

        ui.add_space(14.0);
        setting_card(ui, "OVERLAY", |ui| self.overlay_settings(ui));

        ui.add_space(14.0);
        setting_card(ui, "INVITE AUTOMATION & ACCESS LISTS", |ui| {
            let commands = self.invite_settings.render(ui, &self.state.invites);
            for command in commands {
                self.send_command(command);
            }
        });

        ui.add_space(14.0);
        setting_card(ui, "DIAGNOSTICS", |ui| {
            status_row(
                ui,
                "Daemon IPC",
                if self.daemon_connected {
                    "Connected"
                } else {
                    "Disconnected"
                },
            );
            status_row(
                ui,
                "Overlay runtime",
                if self.state.overlay.runtime_connected {
                    "Connected"
                } else {
                    "Disconnected"
                },
            );
            status_row(
                ui,
                "OSC runtime",
                if self.state.osc.connected {
                    "Connected"
                } else {
                    "Waiting"
                },
            );
            status_row(
                ui,
                "Heart-rate provider",
                hr_provider_status_name(self.state.heart_rate.provider_status),
            );
            status_row(
                ui,
                "VRChat",
                if self.state.vrchat.connected {
                    "Connected"
                } else {
                    "Offline"
                },
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Device selection, thresholds, placement, automations, access lists, and runtime health all live here.",
                )
                .color(MUTED)
                .size(14.0),
            );
        });
    }
}

impl eframe::App for NightshiftApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_updates();
        self.tick_toasts();
        if !self.toasts.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.visuals_mut().dark_mode = true;
        ui.visuals_mut().panel_fill = BG;
        ui.visuals_mut().override_text_color = Some(Color32::from_rgb(238, 238, 242));
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        let ctx = ui.ctx().clone();

        egui::Frame::central_panel(ui.style())
            .fill(BG)
            .inner_margin(28)
            .show(ui, |ui| {
                self.nav(ui);
                ui.add_space(18.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
                        Page::Home => self.home(ui),
                        Page::Settings => self.settings(ui),
                    });
            });

        self.render_toasts(&ctx);
    }
}

fn nav_button(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .strong()
                .size(14.0)
                .color(if selected { Color32::WHITE } else { MUTED }),
        )
        .fill(if selected { PURPLE_DARK } else { Color32::TRANSPARENT })
        .stroke(Stroke::new(1.0, if selected { PURPLE } else { BORDER }))
        .corner_radius(12),
    )
}

fn overlay_card(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(20)
        .inner_margin(22)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add_contents(ui);
        });
}

fn status_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(15.0).color(MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).size(15.0).strong());
        });
    });
}

fn slider_label(ui: &mut egui::Ui, label: &str, value: String) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(16.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).size(15.0).strong().color(PURPLE));
        });
    });
}

fn small_card(ui: &mut egui::Ui, title: &str, value: &str) {
    egui::Frame::new()
        .fill(PANEL_ALT)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(16)
        .inner_margin(18)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(RichText::new(title).size(13.5).strong().color(MUTED));
            ui.add_space(7.0);
            ui.label(RichText::new(value).strong().size(20.0));
        });
}

fn setting_card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(20)
        .inner_margin(22)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).strong().size(18.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("NIGHTSHIFT").size(11.5).strong().color(PURPLE));
                });
            });
            ui.add_space(12.0);
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
        ui.label(RichText::new(label).size(16.0));
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
