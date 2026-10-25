use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use eframe::egui::{self, Color32, RichText, TextureHandle};
use nightshift_protocol::{InviteActionKind, InviteProfile, InviteRuleSettings, InviteState, UiCommand};
use nightshift_vrchat_auth::{FriendProfile, VrchatAuthService};

const ERROR_RED: Color32 = Color32::from_rgb(235, 112, 112);
const PURPLE: Color32 = Color32::from_rgb(183, 141, 255);
const MUTED: Color32 = Color32::from_rgb(157, 159, 170);
const FRIEND_RESULT_LIMIT: usize = 40;
const FRIEND_THUMBNAIL_SIZE: u32 = 128;
const MAX_FRIEND_TEXTURES: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SocialListTarget {
    Whitelist,
    Blacklist,
}

enum FriendCommand {
    Refresh,
    LoadImage { friend_id: String, url: String },
}

enum FriendUpdate {
    Busy(bool),
    Profiles {
        profiles: Vec<FriendProfile>,
        stale_age: Option<Duration>,
    },
    Image {
        friend_id: String,
        url: String,
        bytes: Arc<[u8]>,
    },
    Error(String),
}

struct FriendBridge {
    commands: mpsc::Sender<FriendCommand>,
    updates: mpsc::Receiver<FriendUpdate>,
}

impl FriendBridge {
    fn spawn(ctx: egui::Context) -> Self {
        let (command_tx, command_rx) = mpsc::channel::<FriendCommand>();
        let (update_tx, update_rx) = mpsc::channel::<FriendUpdate>();

        thread::spawn(move || {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = update_tx.send(FriendUpdate::Error(format!(
                        "Failed to start VRChat friend runtime: {error}"
                    )));
                    ctx.request_repaint();
                    return;
                }
            };
            let service = match VrchatAuthService::new() {
                Ok(service) => service,
                Err(error) => {
                    let _ = update_tx.send(FriendUpdate::Error(format!(
                        "Failed to initialize VRChat friend service: {error}"
                    )));
                    ctx.request_repaint();
                    return;
                }
            };

            while let Ok(command) = command_rx.recv() {
                match command {
                    FriendCommand::Refresh => {
                        let _ = update_tx.send(FriendUpdate::Busy(true));
                        if let Ok(Some((profiles, age))) = runtime.block_on(service.cached_friends()) {
                            let _ = update_tx.send(FriendUpdate::Profiles {
                                profiles,
                                stale_age: Some(age),
                            });
                        }
                        let update = match runtime.block_on(service.friends()) {
                            Ok(profiles) => FriendUpdate::Profiles {
                                profiles,
                                stale_age: None,
                            },
                            Err(error) => FriendUpdate::Error(error.to_string()),
                        };
                        let _ = update_tx.send(update);
                        let _ = update_tx.send(FriendUpdate::Busy(false));
                    }
                    FriendCommand::LoadImage { friend_id, url } => {
                        match runtime.block_on(service.friend_image_bytes(&url)) {
                            Ok(bytes) => {
                                let _ = update_tx.send(FriendUpdate::Image {
                                    friend_id,
                                    url,
                                    bytes,
                                });
                            }
                            Err(_) => {
                                // Image failures are intentionally silent; an old thumbnail or initials keep watch.
                            }
                        }
                    }
                }
                ctx.request_repaint();
            }
        });

        Self {
            commands: command_tx,
            updates: update_rx,
        }
    }

    fn send(&self, command: FriendCommand) {
        let _ = self.commands.send(command);
    }
}

pub struct InviteSettingsUi {
    draft_rules: InviteRuleSettings,
    canonical_rules: InviteRuleSettings,
    whitelist_edit: String,
    blacklist_edit: String,
    initialized: bool,
    friend_bridge: Option<FriendBridge>,
    friend_profiles: Vec<FriendProfile>,
    friend_query: String,
    friend_busy: bool,
    friend_error: Option<String>,
    friend_stale_age: Option<Duration>,
    friend_refresh_started: bool,
    social_target: SocialListTarget,
    requested_images: HashSet<String>,
    friend_textures: HashMap<String, TextureHandle>,
    friend_texture_sources: HashMap<String, String>,
    friend_texture_order: VecDeque<String>,
}

impl Default for InviteSettingsUi {
    fn default() -> Self {
        Self {
            draft_rules: InviteRuleSettings::default(),
            canonical_rules: InviteRuleSettings::default(),
            whitelist_edit: String::new(),
            blacklist_edit: String::new(),
            initialized: false,
            friend_bridge: None,
            friend_profiles: Vec::new(),
            friend_query: String::new(),
            friend_busy: false,
            friend_error: None,
            friend_stale_age: None,
            friend_refresh_started: false,
            social_target: SocialListTarget::Whitelist,
            requested_images: HashSet::new(),
            friend_textures: HashMap::new(),
            friend_texture_sources: HashMap::new(),
            friend_texture_order: VecDeque::new(),
        }
    }
}

impl InviteSettingsUi {
    pub fn render(&mut self, ui: &mut egui::Ui, state: &InviteState) -> Vec<UiCommand> {
        self.ensure_friend_bridge(ui.ctx().clone());
        self.drain_friend_updates(ui.ctx());
        self.sync_from_state(state);
        let mut commands = Vec::new();

        let mut enabled = state.enabled;
        if ui.checkbox(&mut enabled, "Invite automation").changed() {
            commands.push(UiCommand::SetInviteAutomation(enabled));
        }
        ui.label(
            RichText::new(
                "Off by default. Nightshift only acts on invite notifications when this is enabled.",
            )
            .weak()
            .small(),
        );

        ui.add_space(8.0);
        let mut profile = state.profile;
        ui.horizontal(|ui| {
            ui.label("Profile");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt("invite_profile")
                    .selected_text(profile_name(profile))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut profile, InviteProfile::Disabled, "Disabled");
                        ui.selectable_value(&mut profile, InviteProfile::Friends, "Friends");
                        ui.selectable_value(&mut profile, InviteProfile::Favorites, "Favorites");
                        ui.selectable_value(&mut profile, InviteProfile::Whitelist, "Whitelist");
                        ui.selectable_value(&mut profile, InviteProfile::Custom, "Custom");
                    });
            });
        });
        if profile != state.profile {
            commands.push(UiCommand::SetInviteProfile(profile));
        }

        match profile {
            InviteProfile::Disabled => {
                ui.label(RichText::new("No invite notifications are acted on with this profile.").weak().small());
            }
            InviteProfile::Friends => {
                ui.label(RichText::new("Invite requests from VRChat friends are accepted; others are ignored.").weak().small());
            }
            InviteProfile::Favorites => {
                ui.label(RichText::new("Invite requests from favorite friends are accepted; others are ignored.").weak().small());
            }
            InviteProfile::Whitelist => self.whitelist_editor(ui, &mut commands),
            InviteProfile::Custom => self.custom_editor(ui, &mut commands),
        }

        ui.add_space(12.0);
        ui.separator();
        self.blacklist_editor(ui, &mut commands);

        ui.add_space(12.0);
        ui.separator();
        self.friend_picker(ui, &mut commands);

        render_feedback(ui, state);
        commands
    }

    fn ensure_friend_bridge(&mut self, ctx: egui::Context) {
        if self.friend_bridge.is_none() {
            self.friend_bridge = Some(FriendBridge::spawn(ctx));
        }
        if !self.friend_refresh_started {
            if let Some(bridge) = &self.friend_bridge {
                bridge.send(FriendCommand::Refresh);
                self.friend_refresh_started = true;
            }
        }
    }

    fn drain_friend_updates(&mut self, ctx: &egui::Context) {
        let Some(bridge) = &self.friend_bridge else { return; };
        while let Ok(update) = bridge.updates.try_recv() {
            match update {
                FriendUpdate::Busy(busy) => self.friend_busy = busy,
                FriendUpdate::Profiles {
                    profiles,
                    stale_age,
                } => {
                    self.friend_profiles = profiles;
                    self.friend_stale_age = stale_age;
                    if stale_age.is_none() {
                        self.friend_error = None;
                        self.requested_images
                            .retain(|url| self.friend_textures.contains_key(url));
                    }
                }
                FriendUpdate::Image {
                    friend_id,
                    url,
                    bytes,
                } => {
                    if let Ok(decoded) = image::load_from_memory(&bytes) {
                        let rgba = decoded
                            .resize_to_fill(
                                FRIEND_THUMBNAIL_SIZE,
                                FRIEND_THUMBNAIL_SIZE,
                                image::imageops::FilterType::Triangle,
                            )
                            .to_rgba8();
                        let color = egui::ColorImage::from_rgba_unmultiplied(
                            [FRIEND_THUMBNAIL_SIZE as usize; 2],
                            rgba.as_raw(),
                        );
                        let texture = ctx.load_texture(
                            format!("vrchat-friend:{url}"),
                            color,
                            egui::TextureOptions::LINEAR,
                        );
                        self.friend_textures.insert(url.clone(), texture);
                        self.friend_texture_sources.insert(friend_id, url.clone());
                        self.friend_texture_order.push_back(url);
                        while self.friend_texture_order.len() > MAX_FRIEND_TEXTURES {
                            if let Some(oldest) = self.friend_texture_order.pop_front() {
                                self.friend_textures.remove(&oldest);
                                self.requested_images.remove(&oldest);
                                self.friend_texture_sources
                                    .retain(|_, source| source != &oldest);
                            }
                        }
                        // Downloaded bytes and full-size decode fall out of scope here. Tiny textures stay; boxes do not.
                    }
                }
                FriendUpdate::Error(error) => self.friend_error = Some(error),
            }
        }
    }

    fn friend_picker(&mut self, ui: &mut egui::Ui, commands: &mut Vec<UiCommand>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Friend picker").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!self.friend_busy, egui::Button::new("Refresh")).clicked() {
                    self.friend_error = None;
                    if let Some(bridge) = &self.friend_bridge {
                        bridge.send(FriendCommand::Refresh);
                    }
                }
            });
        });
        ui.label(
            RichText::new("Search your VRChat friends and add them without copying user IDs. Rules still store stable usr_ IDs only.")
                .color(MUTED)
                .small(),
        );

        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.social_target, SocialListTarget::Whitelist, "Whitelist");
            ui.selectable_value(&mut self.social_target, SocialListTarget::Blacklist, "Blacklist");
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.friend_query)
                .hint_text("Search friends by display name"),
        );

        if self.friend_busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Refreshing VRChat friends…");
            });
        }
        if let Some(age) = self.friend_stale_age {
            ui.label(
                RichText::new(format!(
                    "Showing cached friend data ({} old) while refresh is pending.",
                    format_cache_age(age)
                ))
                .color(MUTED)
                .small(),
            );
        }
        if let Some(error) = &self.friend_error {
            ui.label(RichText::new(error).color(ERROR_RED).small());
        }

        let query = self.friend_query.trim().to_lowercase();
        let profiles = self
            .friend_profiles
            .iter()
            .filter(|profile| {
                query.is_empty()
                    || profile.display_name.to_lowercase().contains(&query)
                    || profile.id.to_lowercase().contains(&query)
            })
            .take(FRIEND_RESULT_LIMIT)
            .cloned()
            .collect::<Vec<_>>();

        if profiles.is_empty() && !self.friend_busy && self.friend_error.is_none() {
            ui.label(RichText::new("No matching friends.").color(MUTED).small());
            return;
        }

        egui::ScrollArea::vertical()
            .id_salt("vrchat_friend_picker")
            .max_height(260.0)
            .show(ui, |ui| {
                for profile in profiles {
                    let image_url = profile.preferred_image_url().map(str::to_owned);
                    if let Some(url) = image_url.as_deref()
                        && !self.friend_textures.contains_key(url)
                        && self.requested_images.insert(url.to_owned())
                        && let Some(bridge) = &self.friend_bridge
                    {
                        bridge.send(FriendCommand::LoadImage {
                            friend_id: profile.id.clone(),
                            url: url.to_owned(),
                        });
                    }

                    let in_whitelist = self.draft_rules.whitelist.iter().any(|id| id == &profile.id);
                    let in_blacklist = self.draft_rules.blacklist.iter().any(|id| id == &profile.id);
                    let already_targeted = match self.social_target {
                        SocialListTarget::Whitelist => in_whitelist,
                        SocialListTarget::Blacklist => in_blacklist,
                    };

                    egui::Frame::new()
                        .fill(Color32::from_rgb(30, 31, 38))
                        .corner_radius(10)
                        .inner_margin(8)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let texture = image_url
                                    .as_deref()
                                    .and_then(|url| self.friend_textures.get(url))
                                    .or_else(|| {
                                        self.friend_texture_sources
                                            .get(&profile.id)
                                            .and_then(|url| self.friend_textures.get(url))
                                    });
                                if let Some(texture) = texture {
                                    ui.add(egui::Image::new((texture.id(), egui::vec2(34.0, 34.0))));
                                } else {
                                    let initials = friend_initials(&profile.display_name);
                                    egui::Frame::new()
                                        .fill(Color32::from_rgb(62, 50, 83))
                                        .corner_radius(17)
                                        .inner_margin(8)
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(initials).strong().color(PURPLE));
                                        });
                                }

                                ui.vertical(|ui| {
                                    ui.label(RichText::new(&profile.display_name).strong());
                                    ui.label(RichText::new(&profile.id).color(MUTED).small());
                                });

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let text = if already_targeted { "Added" } else { "Add" };
                                    if ui.add_enabled(!already_targeted, egui::Button::new(text)).clicked() {
                                        let mut rules = self.draft_rules.clone();
                                        match self.social_target {
                                            SocialListTarget::Whitelist => {
                                                if !rules.whitelist.contains(&profile.id) {
                                                    rules.whitelist.push(profile.id.clone());
                                                }
                                            }
                                            SocialListTarget::Blacklist => {
                                                if !rules.blacklist.contains(&profile.id) {
                                                    rules.blacklist.push(profile.id.clone());
                                                }
                                            }
                                        }
                                        self.apply_draft_rules(rules.clone());
                                        commands.push(UiCommand::SetInviteRules(rules));
                                    }
                                });
                            });
                        });
                    ui.add_space(5.0);
                }
            });
    }

    fn sync_from_state(&mut self, state: &InviteState) {
        if !self.initialized || state.rules != self.canonical_rules {
            self.apply_draft_rules(state.rules.clone());
            self.canonical_rules = state.rules.clone();
            self.initialized = true;
        }
    }

    fn apply_draft_rules(&mut self, rules: InviteRuleSettings) {
        self.whitelist_edit = rules.whitelist.join("\n");
        self.blacklist_edit = rules.blacklist.join("\n");
        self.draft_rules = rules;
    }

    fn whitelist_editor(&mut self, ui: &mut egui::Ui, commands: &mut Vec<UiCommand>) {
        ui.add_space(8.0);
        ui.label("Whitelisted VRChat user IDs");
        let response = ui.add(
            egui::TextEdit::multiline(&mut self.whitelist_edit)
                .desired_rows(4)
                .hint_text("usr_xxxxxxxx\nusr_yyyyyyyy"),
        );
        ui.label(RichText::new("One VRChat user ID per line. Commas are also accepted.").weak().small());
        if response.changed() {
            self.draft_rules.whitelist = parse_user_list(&self.whitelist_edit);
            commands.push(UiCommand::SetInviteRules(self.draft_rules.clone()));
        }
    }

    fn blacklist_editor(&mut self, ui: &mut egui::Ui, commands: &mut Vec<UiCommand>) {
        ui.label(RichText::new("Always decline").strong());
        ui.label("Blacklisted VRChat user IDs");
        let response = ui.add(
            egui::TextEdit::multiline(&mut self.blacklist_edit)
                .desired_rows(4)
                .hint_text("usr_blocked_one\nusr_blocked_two"),
        );
        ui.label(
            RichText::new("Blacklisted users are declined before friend, favorite, whitelist, or custom acceptance rules.")
                .weak()
                .small(),
        );
        if response.changed() {
            self.draft_rules.blacklist = parse_user_list(&self.blacklist_edit);
            commands.push(UiCommand::SetInviteRules(self.draft_rules.clone()));
        }
    }

    fn custom_editor(&mut self, ui: &mut egui::Ui, commands: &mut Vec<UiCommand>) {
        ui.add_space(8.0);
        ui.label(RichText::new("Accept when").strong());
        let mut changed = false;
        changed |= ui.checkbox(&mut self.draft_rules.accept_friends, "Sender is a friend").changed();
        changed |= ui.checkbox(&mut self.draft_rules.accept_favorites, "Sender is a favorite friend").changed();
        changed |= ui.checkbox(&mut self.draft_rules.accept_whitelist, "Sender is on the whitelist").changed();

        if self.draft_rules.accept_whitelist {
            self.whitelist_editor(ui, commands);
        }

        let before = self.draft_rules.fallback;
        ui.horizontal(|ui| {
            ui.label("Otherwise");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt("invite_custom_fallback")
                    .selected_text(fallback_name(self.draft_rules.fallback))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.draft_rules.fallback, InviteActionKind::Ignored, "Ignore");
                        ui.selectable_value(&mut self.draft_rules.fallback, InviteActionKind::Declined, "Decline");
                    });
            });
        });
        changed |= before != self.draft_rules.fallback;

        if changed {
            commands.push(UiCommand::SetInviteRules(self.draft_rules.clone()));
        }
        ui.label(
            RichText::new("Custom fallback is intentionally limited to Ignore or Decline; there is no implicit accept-everyone mode.")
                .weak()
                .small(),
        );
    }
}

fn friend_initials(name: &str) -> String {
    let mut words = name.split_whitespace().filter_map(|word| word.chars().next());
    let first = words.next().unwrap_or('?');
    let second = words.next();
    match second {
        Some(second) => format!("{}{}", first.to_uppercase(), second.to_uppercase()),
        None => first.to_uppercase().collect(),
    }
}

fn format_cache_age(age: Duration) -> String {
    if age.as_secs() < 60 {
        return format!("{}s", age.as_secs());
    }
    if age.as_secs() < 60 * 60 {
        return format!("{}m", age.as_secs() / 60);
    }
    format!("{}h", age.as_secs() / (60 * 60))
}

fn render_feedback(ui: &mut egui::Ui, state: &InviteState) {
    if state.last_action.is_none() && state.last_error.is_none() {
        return;
    }
    ui.add_space(10.0);
    ui.separator();
    ui.add_space(6.0);
    if let Some(action) = &state.last_action {
        let who = action.display_name.as_deref().unwrap_or("Unknown user");
        ui.label(RichText::new(format!("Last action: {} — {who}", action_name(action.kind))).weak().small());
    }
    if let Some(error) = &state.last_error {
        ui.label(RichText::new(format!("Last failure: {error}")).color(ERROR_RED).small());
    }
}

fn parse_user_list(value: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for id in value.split(['\n', ',']).map(str::trim).filter(|id| !id.is_empty()) {
        if seen.insert(id.to_owned()) {
            ids.push(id.to_owned());
        }
    }
    ids
}

fn profile_name(profile: InviteProfile) -> &'static str {
    match profile {
        InviteProfile::Disabled => "Disabled",
        InviteProfile::Friends => "Friends",
        InviteProfile::Favorites => "Favorites",
        InviteProfile::Whitelist => "Whitelist",
        InviteProfile::Custom => "Custom",
    }
}

fn action_name(action: InviteActionKind) -> &'static str {
    match action {
        InviteActionKind::Accepted => "Accepted",
        InviteActionKind::Declined => "Declined",
        InviteActionKind::Ignored => "Ignored",
        InviteActionKind::Asked => "Asked",
    }
}

fn fallback_name(action: InviteActionKind) -> &'static str {
    match action {
        InviteActionKind::Accepted => "Accept",
        InviteActionKind::Declined => "Decline",
        InviteActionKind::Ignored => "Ignore",
        InviteActionKind::Asked => "Ask",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_list_parser_trims_deduplicates_and_accepts_commas() {
        assert_eq!(
            parse_user_list(" usr_one\nusr_two, usr_one\n\nusr_three "),
            vec!["usr_one", "usr_two", "usr_three"]
        );
    }

    #[test]
    fn feedback_action_names_are_user_facing() {
        assert_eq!(action_name(InviteActionKind::Accepted), "Accepted");
        assert_eq!(action_name(InviteActionKind::Declined), "Declined");
        assert_eq!(action_name(InviteActionKind::Ignored), "Ignored");
    }

    #[test]
    fn initials_are_generated_without_fake_avatar_data() {
        assert_eq!(friend_initials("Sleepy Friend"), "SF");
        assert_eq!(friend_initials("Chineko"), "C");
        assert_eq!(friend_initials(""), "?");
    }

    #[test]
    fn friend_thumbnail_budget_stays_small() {
        assert_eq!(FRIEND_THUMBNAIL_SIZE, 128);
        assert_eq!(MAX_FRIEND_TEXTURES, 128);
        assert_eq!(FRIEND_THUMBNAIL_SIZE as usize * FRIEND_THUMBNAIL_SIZE as usize * 4 * MAX_FRIEND_TEXTURES, 8 * 1024 * 1024);
    }

    #[test]
    fn cached_friend_age_is_compact() {
        assert_eq!(format_cache_age(Duration::from_secs(45)), "45s");
        assert_eq!(format_cache_age(Duration::from_secs(90)), "1m");
        assert_eq!(format_cache_age(Duration::from_secs(7_200)), "2h");
    }
}
