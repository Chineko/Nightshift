use std::{sync::mpsc, thread};

use eframe::egui::{self, Color32, RichText};
use nightshift_vrchat_auth::{AuthStep, PendingAuth, VrchatAuthService};

const BG: Color32 = Color32::from_rgb(12, 13, 17);
const PANEL: Color32 = Color32::from_rgb(23, 24, 30);
const MUTED: Color32 = Color32::from_rgb(157, 159, 170);
const PURPLE: Color32 = Color32::from_rgb(183, 141, 255);

enum AuthCommand {
    Login { username: String, password: String },
    Verify { pending: PendingAuth, code: String },
    ClearSession,
}

#[derive(Debug)]
enum AuthUpdate {
    Busy(bool),
    TwoFactorRequired(PendingAuth),
    Authenticated(String),
    SessionCleared,
    Error(String),
}

struct AuthBridge {
    commands: mpsc::Sender<AuthCommand>,
    updates: mpsc::Receiver<AuthUpdate>,
}

impl AuthBridge {
    fn spawn(ctx: egui::Context) -> Self {
        let (command_tx, command_rx) = mpsc::channel::<AuthCommand>();
        let (update_tx, update_rx) = mpsc::channel::<AuthUpdate>();

        thread::spawn(move || {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = update_tx.send(AuthUpdate::Error(format!(
                        "Failed to start authentication runtime: {error}"
                    )));
                    ctx.request_repaint();
                    return;
                }
            };
            let service = match VrchatAuthService::new() {
                Ok(service) => service,
                Err(error) => {
                    let _ = update_tx.send(AuthUpdate::Error(format!(
                        "Failed to initialize VRChat authentication: {error}"
                    )));
                    ctx.request_repaint();
                    return;
                }
            };

            while let Ok(command) = command_rx.recv() {
                let _ = update_tx.send(AuthUpdate::Busy(true));
                ctx.request_repaint();

                let update = match command {
                    AuthCommand::Login { username, password } => {
                        match runtime.block_on(service.begin_login(&username, &password)) {
                            Ok(AuthStep::Authenticated(user)) => {
                                AuthUpdate::Authenticated(user.display_name)
                            }
                            Ok(AuthStep::TwoFactorRequired(pending)) => {
                                AuthUpdate::TwoFactorRequired(pending)
                            }
                            Err(error) => AuthUpdate::Error(error.to_string()),
                        }
                    }
                    AuthCommand::Verify { pending, code } => {
                        match runtime.block_on(service.verify_two_factor(&pending, &code)) {
                            Ok(user) => AuthUpdate::Authenticated(user.display_name),
                            Err(error) => {
                                // Keep the pending session in the UI so a mistyped code is retryable.
                                let _ = update_tx.send(AuthUpdate::TwoFactorRequired(pending));
                                AuthUpdate::Error(error.to_string())
                            }
                        }
                    }
                    AuthCommand::ClearSession => match runtime.block_on(service.clear_session()) {
                        Ok(()) => AuthUpdate::SessionCleared,
                        Err(error) => AuthUpdate::Error(error.to_string()),
                    },
                };

                let _ = update_tx.send(update);
                let _ = update_tx.send(AuthUpdate::Busy(false));
                ctx.request_repaint();
            }
        });

        Self {
            commands: command_tx,
            updates: update_rx,
        }
    }

    fn send(&self, command: AuthCommand) {
        let _ = self.commands.send(command);
    }
}

struct LoginApp {
    bridge: AuthBridge,
    username: String,
    password: String,
    code: String,
    pending: Option<PendingAuth>,
    busy: bool,
    message: Option<(String, bool)>,
}

impl LoginApp {
    fn new(ctx: egui::Context) -> Self {
        Self {
            bridge: AuthBridge::spawn(ctx),
            username: String::new(),
            password: String::new(),
            code: String::new(),
            pending: None,
            busy: false,
            message: None,
        }
    }

    fn drain_updates(&mut self) {
        while let Ok(update) = self.bridge.updates.try_recv() {
            match update {
                AuthUpdate::Busy(busy) => self.busy = busy,
                AuthUpdate::TwoFactorRequired(pending) => {
                    self.pending = Some(pending);
                    self.code.clear();
                    self.message = Some(("VRChat requires two-factor authentication.".into(), true));
                }
                AuthUpdate::Authenticated(display_name) => {
                    self.pending = None;
                    self.password.clear();
                    self.code.clear();
                    self.message = Some((
                        format!("Connected as {display_name}. Session saved securely."),
                        true,
                    ));
                }
                AuthUpdate::SessionCleared => {
                    self.pending = None;
                    self.password.clear();
                    self.code.clear();
                    self.message = Some(("Stored VRChat session removed.".into(), true));
                }
                AuthUpdate::Error(message) => self.message = Some((message, false)),
            }
        }
    }

    fn login(&mut self) {
        let username = self.username.trim().to_owned();
        if username.is_empty() || self.password.is_empty() || self.busy {
            return;
        }
        let password = std::mem::take(&mut self.password);
        self.message = None;
        self.bridge.send(AuthCommand::Login { username, password });
    }

    fn verify(&mut self) {
        let Some(pending) = self.pending.clone() else {
            return;
        };
        let code = std::mem::take(&mut self.code);
        if code.trim().is_empty() || self.busy {
            return;
        }
        self.message = None;
        self.bridge.send(AuthCommand::Verify { pending, code });
    }
}

impl eframe::App for LoginApp {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_updates();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.visuals_mut().dark_mode = true;
        ui.visuals_mut().panel_fill = BG;
        ui.visuals_mut().override_text_color = Some(Color32::from_rgb(238, 238, 242));

        egui::Frame::central_panel(ui.style())
            .fill(BG)
            .inner_margin(24)
            .show(ui, |ui| {
                ui.heading(RichText::new("Connect VRChat").size(26.0));
                ui.label(
                    RichText::new(
                        "Credentials are used only to establish a VRChat session. Nightshift stores only the resulting session in your desktop keyring.",
                    )
                    .color(MUTED),
                );
                ui.add_space(18.0);

                egui::Frame::new()
                    .fill(PANEL)
                    .corner_radius(16)
                    .inner_margin(20)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());

                        if let Some(pending) = &self.pending {
                            ui.label(RichText::new(pending.method_name()).strong());
                            ui.add(
                                egui::TextEdit::singleline(&mut self.code)
                                    .password(true)
                                    .hint_text("Enter verification code"),
                            );
                            ui.add_space(8.0);
                            if ui
                                .add_enabled(
                                    !self.busy && !self.code.trim().is_empty(),
                                    egui::Button::new("Verify").fill(PURPLE),
                                )
                                .clicked()
                            {
                                self.verify();
                            }
                            if ui.button("Start over").clicked() && !self.busy {
                                self.pending = None;
                                self.code.clear();
                                self.message = None;
                            }
                        } else {
                            ui.label("Username or email");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.username)
                                    .hint_text("VRChat account"),
                            );
                            ui.label("Password");
                            let password_response = ui.add(
                                egui::TextEdit::singleline(&mut self.password)
                                    .password(true)
                                    .hint_text("Password"),
                            );
                            ui.add_space(8.0);
                            let can_login = !self.busy
                                && !self.username.trim().is_empty()
                                && !self.password.is_empty();
                            if ui
                                .add_enabled(
                                    can_login,
                                    egui::Button::new("Connect").fill(PURPLE),
                                )
                                .clicked()
                                || (can_login
                                    && password_response.lost_focus()
                                    && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                            {
                                self.login();
                            }
                        }

                        if self.busy {
                            ui.add_space(10.0);
                            ui.spinner();
                            ui.label(RichText::new("Talking to VRChat…").color(MUTED));
                        }

                        if let Some((message, success)) = &self.message {
                            ui.add_space(12.0);
                            let color = if *success {
                                Color32::from_rgb(102, 220, 145)
                            } else {
                                Color32::from_rgb(235, 112, 112)
                            };
                            ui.label(RichText::new(message).color(color));
                        }

                        ui.add_space(16.0);
                        ui.separator();
                        if ui
                            .add_enabled(!self.busy, egui::Button::new("Forget stored session"))
                            .clicked()
                        {
                            self.bridge.send(AuthCommand::ClearSession);
                        }
                    });
            });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_title("Nightshift — VRChat Login")
            .with_inner_size([520.0, 500.0])
            .with_min_inner_size([420.0, 420.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Nightshift VRChat Login",
        options,
        Box::new(|cc| Ok(Box::new(LoginApp::new(cc.egui_ctx.clone())))),
    )
}
