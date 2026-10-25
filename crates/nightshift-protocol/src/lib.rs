use serde::{Deserialize, Deserializer, Serialize};

pub const PROTOCOL_VERSION: u32 = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppState {
    pub connection: ConnectionState,
    pub sleep: SleepState,
    pub brightness: BrightnessState,
    pub heart_rate: HeartRateState,
    pub osc: OscState,
    pub vrchat: VrchatState,
    pub invites: InviteState,
    pub overlay: OverlayState,
    pub settings: SettingsState,
    #[serde(default)]
    pub recent_events: Vec<RecentEvent>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            connection: ConnectionState::Connected,
            sleep: SleepState::default(),
            brightness: BrightnessState::default(),
            heart_rate: HeartRateState::default(),
            osc: OscState::default(),
            vrchat: VrchatState::default(),
            invites: InviteState::default(),
            overlay: OverlayState::default(),
            settings: SettingsState::default(),
            recent_events: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeHealthState {
    pub overlay_connected: bool,
    pub osc_connected: bool,
    pub heart_rate_connected: bool,
    pub vrchat_connected: bool,
    pub heart_rate_provider_status: HrProviderStatus,
}

impl AppState {
    pub fn runtime_health(&self) -> RuntimeHealthState {
        // One health view, zero extra ledgers. The goblins may observe state; they may not fork it.
        RuntimeHealthState {
            overlay_connected: self.overlay.runtime_connected,
            osc_connected: self.osc.connected,
            heart_rate_connected: self.heart_rate.connected,
            vrchat_connected: self.vrchat.connected,
            heart_rate_provider_status: self.heart_rate.provider_status,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecentEventKind {
    #[default]
    Runtime,
    Sleep,
    Vrchat,
    Invite,
    HeartRate,
    Osc,
    Overlay,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentEvent {
    pub uptime_ms: u64,
    pub kind: RecentEventKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState { Connecting, #[default] Connected, Degraded, Disconnected }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SleepPhase { #[default] Awake, EnteringSleep, Sleeping, Waking }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SleepingPose { #[default] Unknown, Front, Back, Left, Right }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SleepState {
    pub phase: SleepPhase,
    pub detection_enabled: bool,
    pub detected_by_hr: bool,
    #[serde(default)]
    pub pose: SleepingPose,
}
impl Default for SleepState {
    fn default() -> Self {
        Self {
            phase: SleepPhase::Awake,
            detection_enabled: false,
            detected_by_hr: false,
            pose: SleepingPose::Unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrightnessState {
    pub current: f32,
    pub target: f32,
    pub sleep_target: f32,
    pub transitioning: bool,
    pub manual_override: bool,
}
impl Default for BrightnessState {
    fn default() -> Self { Self { current: 1.0, target: 1.0, sleep_target: 0.06, transitioning: false, manual_override: false } }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HrStatus { Normal, Resting, SleepCandidate, #[default] Unknown }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HrProviderStatus { Disabled, Searching, Connected, Error, #[default] Unknown }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HrDeviceInfo {
    pub id: String,
    pub address: String,
    pub name: Option<String>,
}

fn default_hr_threshold_bpm() -> u16 { 60 }
fn default_hr_calm_period_seconds() -> u64 { 120 }
fn default_hr_cooldown_seconds() -> u64 { 15 * 60 }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeartRateState {
    pub enabled: bool,
    pub connected: bool,
    pub bpm: Option<u16>,
    pub status: HrStatus,
    #[serde(default)]
    pub device_filter: Option<String>,
    #[serde(default)]
    pub connected_device: Option<String>,
    #[serde(default)]
    pub available_devices: Vec<HrDeviceInfo>,
    #[serde(default)]
    pub provider_status: HrProviderStatus,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default = "default_hr_threshold_bpm")]
    pub threshold_bpm: u16,
    #[serde(default = "default_hr_calm_period_seconds")]
    pub calm_period_seconds: u64,
    #[serde(default = "default_hr_cooldown_seconds")]
    pub cooldown_seconds: u64,
}
impl Default for HeartRateState {
    fn default() -> Self {
        Self {
            enabled: false,
            connected: false,
            bpm: None,
            status: HrStatus::Unknown,
            device_filter: None,
            connected_device: None,
            available_devices: Vec::new(),
            provider_status: HrProviderStatus::Disabled,
            last_error: None,
            threshold_bpm: default_hr_threshold_bpm(),
            calm_period_seconds: default_hr_calm_period_seconds(),
            cooldown_seconds: default_hr_cooldown_seconds(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OscState {
    pub enabled: bool,
    pub connected: bool,
    #[serde(default)]
    pub avatar_id: Option<String>,
    #[serde(default)]
    pub avatar_parameter_count: u32,
    #[serde(default)]
    pub gogo_detected: bool,
}
impl Default for OscState {
    fn default() -> Self {
        Self {
            enabled: false,
            connected: false,
            avatar_id: None,
            avatar_parameter_count: 0,
            gogo_detected: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VrchatStatus { #[default] Active, JoinMe, AskMe, Busy, Offline }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VrchatState {
    pub connected: bool,
    pub display_name: Option<String>,
    pub status: VrchatStatus,
}
impl Default for VrchatState {
    fn default() -> Self { Self { connected: false, display_name: None, status: VrchatStatus::Offline } }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteProfile { #[default] Disabled, Friends, Favorites, Whitelist, Custom }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteActionKind { Accepted, Declined, #[default] Ignored, Asked }

fn safe_invite_fallback(value: InviteActionKind) -> InviteActionKind {
    match value {
        InviteActionKind::Declined => InviteActionKind::Declined,
        _ => InviteActionKind::Ignored,
    }
}

fn deserialize_safe_invite_fallback<'de, D>(deserializer: D) -> Result<InviteActionKind, D::Error>
where
    D: Deserializer<'de>,
{
    InviteActionKind::deserialize(deserializer).map(safe_invite_fallback)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct InviteRuleSettings {
    pub whitelist: Vec<String>,
    pub blacklist: Vec<String>,
    pub accept_friends: bool,
    pub accept_favorites: bool,
    pub accept_whitelist: bool,
    #[serde(default, deserialize_with = "deserialize_safe_invite_fallback")]
    pub fallback: InviteActionKind,
}

impl Default for InviteRuleSettings {
    fn default() -> Self {
        Self {
            whitelist: Vec::new(),
            blacklist: Vec::new(),
            accept_friends: true,
            accept_favorites: true,
            accept_whitelist: true,
            fallback: InviteActionKind::Ignored,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteAction { pub kind: InviteActionKind, pub display_name: Option<String> }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteState {
    pub enabled: bool,
    pub profile: InviteProfile,
    #[serde(default)]
    pub rules: InviteRuleSettings,
    #[serde(default)]
    pub last_action: Option<InviteAction>,
    #[serde(default)]
    pub last_error: Option<String>,
}
impl Default for InviteState {
    fn default() -> Self {
        Self {
            enabled: false,
            profile: InviteProfile::Disabled,
            rules: InviteRuleSettings::default(),
            last_action: None,
            last_error: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlaySpawnMode { #[default] InFront, RestoreLastPosition }

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayPose {
    pub position: [f32; 3],
    pub orientation_xyzw: [f32; 4],
}

fn default_overlay_laser_enabled() -> bool { true }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayState {
    pub visible: bool,
    pub spawn_distance_m: f32,
    pub spawn_mode: OverlaySpawnMode,
    #[serde(default = "default_overlay_laser_enabled")]
    pub laser_enabled: bool,
    #[serde(default)]
    pub remembered_pose: Option<OverlayPose>,
    #[serde(default)]
    pub runtime_connected: bool,
    #[serde(default)]
    pub runtime_client_version: Option<String>,
}
impl Default for OverlayState {
    fn default() -> Self {
        Self {
            visible: false,
            spawn_distance_m: 1.0,
            spawn_mode: OverlaySpawnMode::InFront,
            laser_enabled: true,
            remembered_pose: None,
            runtime_connected: false,
            runtime_client_version: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingsState {
    pub fade_duration_seconds: f32,
    pub sleep_vrchat_status: VrchatStatus,
    pub wake_vrchat_status: VrchatStatus,
    #[serde(default)]
    pub vrchat_status_automation_enabled: bool,
    #[serde(default)]
    pub gogo_automation_enabled: bool,
}
impl Default for SettingsState {
    fn default() -> Self {
        Self {
            fade_duration_seconds: 3.0,
            sleep_vrchat_status: VrchatStatus::AskMe,
            wake_vrchat_status: VrchatStatus::Active,
            vrchat_status_automation_enabled: false,
            gogo_automation_enabled: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello { protocol: u32, client: String, client_version: String },
    GetState,
    Command { command: UiCommand },
    Report { report: WorkerReport },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "state", rename_all = "snake_case")]
pub enum WorkerReport {
    OscRuntime {
        connected: bool,
        avatar_id: Option<String>,
        avatar_parameter_count: u32,
        gogo_detected: bool,
    },
    SleepingPose { pose: SleepingPose },
    OverlayRuntimePose { pose: OverlayPose },
    HeartRateRuntime { connected: bool, bpm: Option<u16> },
    HeartRateProviderRuntime {
        status: HrProviderStatus,
        connected_device: Option<String>,
        last_error: Option<String>,
    },
    HeartRateDevices { devices: Vec<HrDeviceInfo> },
    VrchatRuntime {
        connected: bool,
        display_name: Option<String>,
        status: VrchatStatus,
        #[serde(default)]
        last_invite_action: Option<InviteAction>,
        #[serde(default)]
        invite_error: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Hello { protocol: u32, server_version: String, capabilities: Vec<Capability> },
    FullState { state: AppState },
    Event { event: StateEvent },
    Error { message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability { Sleep, Brightness, HeartRate, Osc, VrchatStatus, InviteRules, Overlay, WorkerReports }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum UiCommand {
    EnterSleep,
    Wake,
    SetBrightness(f32),
    SetSleepBrightness(f32),
    SetFadeDuration(f32),
    SetHrMonitoring(bool),
    SetHrDeviceFilter(Option<String>),
    SetHrThreshold(u16),
    SetHrCalmPeriod(u64),
    SetHrCooldown(u64),
    SetSleepDetection(bool),
    SetOscAutomation(bool),
    SetGogoAutomation(bool),
    SetVrchatStatusAutomation(bool),
    SetVrchatSleepStatus(VrchatStatus),
    SetVrchatWakeStatus(VrchatStatus),
    SetInviteAutomation(bool),
    SetInviteProfile(InviteProfile),
    SetInviteRules(InviteRuleSettings),
    SetOverlaySpawnDistance(f32),
    SetOverlaySpawnMode(OverlaySpawnMode),
    SetOverlayLaserEnabled(bool),
    ShowOverlay,
    HideOverlay,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "state", rename_all = "snake_case")]
pub enum StateEvent {
    SleepChanged(SleepState),
    BrightnessChanged(BrightnessState),
    HeartRateChanged(HeartRateState),
    OscChanged(OscState),
    VrchatChanged(VrchatState),
    InvitesChanged(InviteState),
    OverlayChanged(OverlayState),
    SettingsChanged(SettingsState),
    ConnectionChanged(ConnectionState),
}
