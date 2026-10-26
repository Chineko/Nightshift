use std::{sync::mpsc, thread};

use eframe::egui::{self, Color32, RichText};
use nightshift_vrchat_auth::{AuthStep, PendingAuth, VrchatAuthService};

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

pub struct VrchatAuthUi {
    bridge: AuthBridge,
    username: String,
    password: String,
    code: String,
    pending: Option<PendingAuth>,
    authenticated_name: Option<String>,
    busy: bool,
    message: Option<(String, bool)>,
}

impl VrchatAuthUi {
    pub fn new(ctx: egui::Context) -> Self {
        Self {
            bridge: AuthBridge::spawn(ctx),
            username: String::new(),
            password: String::new(),
            code: String::new(),
            pending: None,
            authenticated_name: None,
            busy: false,
            message: None,
        }
    }

    pub fn drain_updates(&mut self) {
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
                    self.authenticated_name = Some(display_name.clone());
                    self.password.clear();
                    self.code.clear();
                    self.message = Some((
                        format!("Connected as {display_name}. Session saved securely."),
                        true,
                    ));
                }
                AuthUpdate::SessionCleared => {
                    self.pending = None;
                    self.authenticated_name = None;
                    self.password.clear();
                    self.code.clear();
                    self.message = Some(("Stored VRChat session removed.".into(), true));
                }
                AuthUpdate::Error(message) => self.message = Some((message, false)),
            }
        }
    }

    pub fn render(&mut self, ui: &mut egui::Ui, runtime_name: Option<&str>) {
        let connected_name = runtime_name.or(self.authenticated_name.as_deref());
        if let Some(name) = connected_name {
            ui.label(RichText::new(format!("Connected as {name}")).strong());
        } else {
            ui.label(RichText::new("VRChat runtime not connected").color(MUTED));
        }
        ui.label(
            RichText::new(
                "Your password is used only for login. Nightshift stores the resulting session in your desktop keyring.",
            )
            .color(MUTED)
            .small(),
        );
        ui.add_space(8.0);

        if let Some(pending) = &self.pending {
            ui.label(RichText::new(pending.method_name()).strong());
            ui.add(
                egui::TextEdit::singleline(&mut self.code)
                    .password(true)
                    .hint_text("Enter verification code"),
            );
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !self.busy && !self.code.trim().is_empty(),
                        egui::Button::new("Verify").fill(PURPLE),
                    )
                    .clicked()
                {
                    self.verify();
                }
                if ui.add_enabled(!self.busy, egui::Button::new("Start over")).clicked() {
                    self.pending = None;
                    self.code.clear();
                    self.message = None;
                }
            });
        } else if connected_name.is_none() {
            ui.label("Username or email");
            ui.add(egui::TextEdit::singleline(&mut self.username).hint_text("VRChat account"));
            ui.label("Password");
            let password_response = ui.add(
                egui::TextEdit::singleline(&mut self.password)
                    .password(true)
                    .hint_text("Password"),
            );
            let can_login = !self.busy
                && !self.username.trim().is_empty()
                && !self.password.is_empty();
            if ui
                .add_enabled(can_login, egui::Button::new("Connect").fill(PURPLE))
                .clicked()
                || (can_login
                    && password_response.lost_focus()
                    && ui.input(|input| input.key_pressed(egui::Key::Enter)))
            {
                self.login();
            }
        } else {
            ui.label(RichText::new("VRChat session is stored and ready for the runtime worker.").color(MUTED));
        }

        if self.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Contacting VRChat...");
            });
        }

        if let Some((message, success)) = &self.message {
            ui.label(RichText::new(message).color(if *success { PURPLE } else { Color32::RED }));
        }

        ui.add_space(8.0);
        if ui
            .add_enabled(!self.busy, egui::Button::new("Forget stored session"))
            .clicked()
        {
            self.message = None;
            self.bridge.send(AuthCommand::ClearSession);
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
