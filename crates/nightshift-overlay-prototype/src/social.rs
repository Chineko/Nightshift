use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use image::imageops::FilterType;
use nightshift_vrchat_auth::{FriendProfile, VrchatAuthService};

use crate::{
    content::PanelContent,
    home_layout::SettingsCompositionLayout,
    interaction::{PanelAction, PanelPage},
    raster::PanelBitmap,
};

const AVATAR_SIZE: u32 = 32;
const PROFILE_REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);
const EMPTY_PROFILE_RETRY_INTERVAL: Duration = Duration::from_secs(60);
const ROW_FILL: [u8; 4] = [18, 20, 31, 255];
const ROW_HOVER_FILL: [u8; 4] = [24, 26, 39, 255];
const TEXT: [u8; 4] = [244, 244, 249, 255];

#[derive(Clone, Debug, PartialEq, Eq)]
struct SocialAvatar {
    rgba: Arc<[u8]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SocialProfile {
    display_name: String,
    avatar: Option<SocialAvatar>,
    image_url: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct SocialProfiles {
    profiles: HashMap<String, SocialProfile>,
    revision: u64,
}

impl SocialProfiles {
    fn profile(&self, user_id: &str) -> Option<&SocialProfile> {
        self.profiles.get(user_id)
    }

    fn replace_profiles(&mut self, profiles: Vec<FriendProfile>) {
        let mut next = HashMap::with_capacity(profiles.len());
        for profile in profiles {
            let image_url = profile.preferred_image_url().map(str::to_owned);
            let avatar = self
                .profiles
                .get(&profile.id)
                .filter(|current| current.image_url == image_url)
                .and_then(|current| current.avatar.clone());
            next.insert(
                profile.id,
                SocialProfile {
                    display_name: profile.display_name,
                    avatar,
                    image_url,
                },
            );
        }
        if self.profiles != next {
            self.profiles = next;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    fn set_avatar(&mut self, user_id: &str, image_url: &str, avatar: SocialAvatar) {
        let Some(profile) = self.profiles.get_mut(user_id) else {
            return;
        };
        if profile.image_url.as_deref() != Some(image_url) {
            return;
        }
        if profile.avatar.as_ref() != Some(&avatar) {
            profile.avatar = Some(avatar);
            self.revision = self.revision.wrapping_add(1);
        }
    }
}

enum SocialCommand {
    Refresh,
    LoadAvatar { user_id: String, url: String },
}

enum SocialUpdate {
    Profiles(Vec<FriendProfile>),
    Avatar {
        user_id: String,
        url: String,
        avatar: SocialAvatar,
    },
    Error(String),
}

#[derive(Debug)]
struct SocialBridge {
    commands: mpsc::Sender<SocialCommand>,
    updates: mpsc::Receiver<SocialUpdate>,
}

impl SocialBridge {
    fn spawn() -> Self {
        let (command_tx, command_rx) = mpsc::channel::<SocialCommand>();
        let (update_tx, update_rx) = mpsc::channel::<SocialUpdate>();

        thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = update_tx.send(SocialUpdate::Error(format!(
                        "failed to start VRChat social runtime: {error}"
                    )));
                    return;
                }
            };
            let service = match VrchatAuthService::new() {
                Ok(service) => service,
                Err(error) => {
                    let _ = update_tx.send(SocialUpdate::Error(format!(
                        "failed to initialize VRChat social service: {error}"
                    )));
                    return;
                }
            };

            while let Ok(command) = command_rx.recv() {
                match command {
                    SocialCommand::Refresh => {
                        let update = match runtime.block_on(service.friends()) {
                            Ok(profiles) => SocialUpdate::Profiles(profiles),
                            Err(error) => SocialUpdate::Error(error.to_string()),
                        };
                        let _ = update_tx.send(update);
                    }
                    SocialCommand::LoadAvatar { user_id, url } => {
                        let bytes = match runtime.block_on(service.friend_image_bytes(&url)) {
                            Ok(bytes) => bytes,
                            Err(_) => continue,
                        };
                        let Ok(decoded) = image::load_from_memory(&bytes) else {
                            continue;
                        };
                        let rgba = decoded
                            .resize_exact(AVATAR_SIZE, AVATAR_SIZE, FilterType::Triangle)
                            .to_rgba8()
                            .into_raw();
                        let _ = update_tx.send(SocialUpdate::Avatar {
                            user_id,
                            url,
                            avatar: SocialAvatar { rgba: rgba.into() },
                        });
                    }
                }
            }
        });

        Self {
            commands: command_tx,
            updates: update_rx,
        }
    }
}

#[derive(Debug)]
pub(super) struct SocialAvatarRuntime {
    bridge: SocialBridge,
    profiles: SocialProfiles,
    requested_images: HashSet<String>,
    last_refresh_requested: Instant,
    last_error: Option<String>,
}

impl SocialAvatarRuntime {
    pub(super) fn spawn() -> Self {
        let bridge = SocialBridge::spawn();
        let _ = bridge.commands.send(SocialCommand::Refresh);
        Self {
            bridge,
            profiles: SocialProfiles::default(),
            requested_images: HashSet::new(),
            last_refresh_requested: Instant::now(),
            last_error: None,
        }
    }

    pub(super) fn revision(&self) -> u64 {
        self.profiles.revision
    }

    pub(super) fn poll(&mut self) {
        while let Ok(update) = self.bridge.updates.try_recv() {
            match update {
                SocialUpdate::Profiles(profiles) => {
                    self.profiles.replace_profiles(profiles);
                    // A successful metadata refresh starts a new bounded request window. Avatars
                    // whose URL did not change stay decoded, while failed/moved URLs may retry.
                    self.requested_images.clear();
                    self.last_error = None;
                }
                SocialUpdate::Avatar {
                    user_id,
                    url,
                    avatar,
                } => self.profiles.set_avatar(&user_id, &url, avatar),
                SocialUpdate::Error(error) => {
                    if self.last_error.as_deref() != Some(error.as_str()) {
                        eprintln!("Nightshift VR social metadata unavailable: {error}");
                    }
                    self.last_error = Some(error);
                }
            }
        }
    }

    pub(super) fn request_visible(&mut self, content: &PanelContent) {
        if content.page != PanelPage::Settings {
            return;
        }

        let retry_after = if self.profiles.profiles.is_empty() {
            EMPTY_PROFILE_RETRY_INTERVAL
        } else {
            PROFILE_REFRESH_INTERVAL
        };
        if self.last_refresh_requested.elapsed() >= retry_after {
            let _ = self.bridge.commands.send(SocialCommand::Refresh);
            self.last_refresh_requested = Instant::now();
        }

        for user_id in content.social_entries().iter().take(5) {
            let Some(profile) = self.profiles.profile(user_id) else {
                continue;
            };
            if profile.avatar.is_some() {
                continue;
            }
            let Some(url) = profile.image_url.as_deref() else {
                continue;
            };
            if !self.requested_images.insert(url.to_owned()) {
                continue;
            }
            let _ = self.bridge.commands.send(SocialCommand::LoadAvatar {
                user_id: user_id.clone(),
                url: url.to_owned(),
            });
        }
    }

    pub(super) fn decorate(
        &self,
        bitmap: &mut PanelBitmap,
        content: &PanelContent,
        hover: Option<PanelAction>,
    ) {
        if content.page != PanelPage::Settings {
            return;
        }

        let rows = SettingsCompositionLayout::compute().social_rows;
        for (index, user_id) in content.social_entries().iter().take(5).enumerate() {
            let Some(profile) = self.profiles.profile(user_id) else {
                continue;
            };
            let row = rows[index];
            let x = row.x.round() as u32;
            let y = row.y.round() as u32;
            let row_fill = if hover == Some(PanelAction::RemoveSocial(index as u8)) {
                ROW_HOVER_FILL
            } else {
                ROW_FILL
            };

            fill_rect(bitmap, x + 50, y + 16, 120, 30, row_fill);
            let label = compact_label(&profile.display_name, 9);
            draw_text(bitmap, x + 52, y + 21, &label, 2, TEXT);

            if let Some(avatar) = profile.avatar.as_ref() {
                draw_avatar(bitmap, x + 26, y + 30, 16, avatar);
            }
        }
    }
}

impl Default for SocialAvatarRuntime {
    fn default() -> Self {
        Self::spawn()
    }
}

fn compact_label(value: &str, max_chars: usize) -> String {
    let mut label = value.trim().to_string();
    if label.chars().count() > max_chars {
        label = label
            .chars()
            .take(max_chars.saturating_sub(2))
            .collect::<String>();
        label.push_str("..");
    }
    label.to_ascii_uppercase()
}

fn draw_avatar(bitmap: &mut PanelBitmap, cx: u32, cy: u32, radius: u32, avatar: &SocialAvatar) {
    if avatar.rgba.len() != (AVATAR_SIZE * AVATAR_SIZE * 4) as usize || radius == 0 {
        return;
    }
    let r = radius as i32;
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let x = cx as i32 + dx;
            let y = cy as i32 + dy;
            if x < 0 || y < 0 {
                continue;
            }
            let sx = (((dx + r) as u32 * (AVATAR_SIZE - 1)) / (radius * 2)) as usize;
            let sy = (((dy + r) as u32 * (AVATAR_SIZE - 1)) / (radius * 2)) as usize;
            let offset = (sy * AVATAR_SIZE as usize + sx) * 4;
            if avatar.rgba[offset + 3] == 0 {
                continue;
            }
            put_pixel(
                bitmap,
                x as u32,
                y as u32,
                [
                    avatar.rgba[offset],
                    avatar.rgba[offset + 1],
                    avatar.rgba[offset + 2],
                    255,
                ],
            );
        }
    }
}

fn fill_rect(bitmap: &mut PanelBitmap, x: u32, y: u32, width: u32, height: u32, color: [u8; 4]) {
    let x_end = x.saturating_add(width).min(bitmap.width);
    let y_end = y.saturating_add(height).min(bitmap.height);
    for py in y..y_end {
        for px in x..x_end {
            put_pixel(bitmap, px, py, color);
        }
    }
}

fn draw_text(bitmap: &mut PanelBitmap, x: u32, y: u32, text: &str, scale: u32, color: [u8; 4]) {
    let mut cursor_x = x;
    let advance = 6 * scale;
    for character in text.chars() {
        if cursor_x.saturating_add(5 * scale) > bitmap.width {
            break;
        }
        let glyph = glyph5x7(character);
        for (row, bits) in glyph.into_iter().enumerate() {
            for column in 0..5_u32 {
                let mask = 1 << (4 - column);
                if bits & mask != 0 {
                    fill_rect(
                        bitmap,
                        cursor_x + column * scale,
                        y + row as u32 * scale,
                        scale,
                        scale,
                        color,
                    );
                }
            }
        }
        cursor_x = cursor_x.saturating_add(advance);
    }
}

fn put_pixel(bitmap: &mut PanelBitmap, x: u32, y: u32, color: [u8; 4]) {
    if x >= bitmap.width || y >= bitmap.height {
        return;
    }
    let offset = ((y * bitmap.width + x) * 4) as usize;
    bitmap.rgba[offset..offset + 4].copy_from_slice(&color);
}

fn glyph5x7(character: char) -> [u8; 7] {
    match character.to_ascii_uppercase() {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100,
        ],
        ' ' => [0; 7],
        _ => [
            0b01110, 0b10001, 0b00010, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_profile_refresh_keeps_decoded_avatar() {
        let mut profiles = SocialProfiles::default();
        profiles.replace_profiles(vec![FriendProfile {
            id: "usr_friend".into(),
            display_name: "Sleepy Friend".into(),
            icon_url: Some("https://example.com/icon.png".into()),
            profile_pic_override: None,
        }]);
        let avatar = SocialAvatar {
            rgba: vec![255_u8; (AVATAR_SIZE * AVATAR_SIZE * 4) as usize].into(),
        };
        profiles.set_avatar("usr_friend", "https://example.com/icon.png", avatar.clone());
        let revision = profiles.revision;

        profiles.replace_profiles(vec![FriendProfile {
            id: "usr_friend".into(),
            display_name: "Sleepy Friend".into(),
            icon_url: Some("https://example.com/icon.png".into()),
            profile_pic_override: None,
        }]);

        assert_eq!(profiles.revision, revision);
        assert_eq!(
            profiles
                .profile("usr_friend")
                .and_then(|profile| profile.avatar.as_ref()),
            Some(&avatar)
        );
    }

    #[test]
    fn stale_avatar_response_does_not_replace_new_profile_image() {
        let mut profiles = SocialProfiles::default();
        profiles.replace_profiles(vec![FriendProfile {
            id: "usr_friend".into(),
            display_name: "Friend".into(),
            icon_url: Some("https://example.com/new.png".into()),
            profile_pic_override: None,
        }]);
        let revision = profiles.revision;
        profiles.set_avatar(
            "usr_friend",
            "https://example.com/old.png",
            SocialAvatar {
                rgba: vec![0_u8; (AVATAR_SIZE * AVATAR_SIZE * 4) as usize].into(),
            },
        );
        assert_eq!(profiles.revision, revision);
        assert!(profiles.profile("usr_friend").unwrap().avatar.is_none());
    }

    #[test]
    fn labels_use_friend_display_names_instead_of_ids() {
        assert_eq!(compact_label("Sleepy Friend", 9), "SLEEPY ..");
    }
}
