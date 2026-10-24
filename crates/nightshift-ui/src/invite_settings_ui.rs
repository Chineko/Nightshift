use std::collections::HashSet;

use eframe::egui::{self, Color32, RichText};
use nightshift_protocol::{InviteActionKind, InviteProfile, InviteRuleSettings, InviteState, UiCommand};

const ERROR_RED: Color32 = Color32::from_rgb(235, 112, 112);

pub struct InviteSettingsUi {
    draft_rules: InviteRuleSettings,
    canonical_rules: InviteRuleSettings,
    whitelist_edit: String,
    blacklist_edit: String,
    initialized: bool,
}

impl Default for InviteSettingsUi {
    fn default() -> Self {
        Self {
            draft_rules: InviteRuleSettings::default(),
            canonical_rules: InviteRuleSettings::default(),
            whitelist_edit: String::new(),
            blacklist_edit: String::new(),
            initialized: false,
        }
    }
}

impl InviteSettingsUi {
    pub fn render(&mut self, ui: &mut egui::Ui, state: &InviteState) -> Vec<UiCommand> {
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
                ui.label(
                    RichText::new("No invite notifications are acted on with this profile.")
                        .weak()
                        .small(),
                );
            }
            InviteProfile::Friends => {
                ui.label(
                    RichText::new("Invite requests from VRChat friends are accepted; others are ignored.")
                        .weak()
                        .small(),
                );
            }
            InviteProfile::Favorites => {
                ui.label(
                    RichText::new("Invite requests from favorite friends are accepted; others are ignored.")
                        .weak()
                        .small(),
                );
            }
            InviteProfile::Whitelist => {
                self.whitelist_editor(ui, &mut commands);
            }
            InviteProfile::Custom => {
                self.custom_editor(ui, &mut commands);
            }
        }

        ui.add_space(12.0);
        ui.separator();
        self.blacklist_editor(ui, &mut commands);

        render_feedback(ui, state);
        commands
    }

    fn sync_from_state(&mut self, state: &InviteState) {
        if !self.initialized || state.rules != self.canonical_rules {
            self.draft_rules = state.rules.clone();
            self.canonical_rules = state.rules.clone();
            self.whitelist_edit = state.rules.whitelist.join("\n");
            self.blacklist_edit = state.rules.blacklist.join("\n");
            self.initialized = true;
        }
    }

    fn whitelist_editor(&mut self, ui: &mut egui::Ui, commands: &mut Vec<UiCommand>) {
        ui.add_space(8.0);
        ui.label("Whitelisted VRChat user IDs");
        let response = ui.add(
            egui::TextEdit::multiline(&mut self.whitelist_edit)
                .desired_rows(4)
                .hint_text("usr_xxxxxxxx\nusr_yyyyyyyy"),
        );
        ui.label(
            RichText::new("One VRChat user ID per line. Commas are also accepted.")
                .weak()
                .small(),
        );
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
        changed |= ui
            .checkbox(&mut self.draft_rules.accept_friends, "Sender is a friend")
            .changed();
        changed |= ui
            .checkbox(
                &mut self.draft_rules.accept_favorites,
                "Sender is a favorite friend",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.draft_rules.accept_whitelist,
                "Sender is on the whitelist",
            )
            .changed();

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
                        ui.selectable_value(
                            &mut self.draft_rules.fallback,
                            InviteActionKind::Ignored,
                            "Ignore",
                        );
                        ui.selectable_value(
                            &mut self.draft_rules.fallback,
                            InviteActionKind::Declined,
                            "Decline",
                        );
                    });
            });
        });
        changed |= before != self.draft_rules.fallback;

        if changed {
            commands.push(UiCommand::SetInviteRules(self.draft_rules.clone()));
        }
        ui.label(
            RichText::new(
                "Custom fallback is intentionally limited to Ignore or Decline; there is no implicit accept-everyone mode.",
            )
            .weak()
            .small(),
        );
    }
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
        ui.label(
            RichText::new(format!(
                "Last action: {} — {who}",
                action_name(action.kind)
            ))
            .weak()
            .small(),
        );
    }

    if let Some(error) = &state.last_error {
        ui.label(
            RichText::new(format!("Last failure: {error}"))
                .color(ERROR_RED)
                .small(),
        );
    }
}

fn parse_user_list(value: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for id in value
        .split(['\n', ','])
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
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
        assert_eq!(action_name(InviteActionKind::Asked), "Asked");
    }
}
