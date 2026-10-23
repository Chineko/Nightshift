use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

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
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState { Connecting, #[default] Connected, Degraded, Disconnected }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SleepPhase { #[default] Awake, EnteringSleep, Sleeping, Waking }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SleepState {
    pub phase: SleepPhase,
    pub detection_enabled: bool,
    pub detected_by_hr: bool,
}
impl Default for SleepState {
    fn default() -> Self { Self { phase: SleepPhase::Awake, detection_enabled: false, detected_by_hr: false } }
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeartRateState {
    pub enabled: bool,
    pub connected: bool,
    pub bpm: Option<u16>,
    pub status: HrStatus,
}
impl Default for HeartRateState {
    fn default() -> Self { Self { enabled: false, connected: false, bpm: None, status: HrStatus::Unknown } }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OscState { pub enabled: bool, pub connected: bool }

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteActionKind { Accepted, Declined, Ignored, Asked }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteAction { pub kind: InviteActionKind, pub display_name: Option<String> }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteState {
    pub enabled: bool,
    pub profile: InviteProfile,
    pub last_action: Option<InviteAction>,
}
impl Default for InviteState {
    fn default() -> Self { Self { enabled: false, profile: InviteProfile::Disabled, last_action: None } }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlaySpawnMode { #[default] InFront, RestoreLastPosition }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayState {
    pub visible: bool,
    pub spawn_distance_m: f32,
    pub spawn_mode: OverlaySpawnMode,
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
}
impl Default for SettingsState {
    fn default() -> Self {
        Self { fade_duration_seconds: 3.0, sleep_vrchat_status: VrchatStatus::AskMe, wake_vrchat_status: VrchatStatus::Active }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello { protocol: u32, client: String, client_version: String },
    GetState,
    Command { command: UiCommand },
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
pub enum Capability { Sleep, Brightness, HeartRate, Osc, VrchatStatus, InviteRules, Overlay }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum UiCommand {
    EnterSleep,
    Wake,
    SetBrightness(f32),
    SetSleepBrightness(f32),
    SetFadeDuration(f32),
    SetHrMonitoring(bool),
    SetSleepDetection(bool),
    SetOscAutomation(bool),
    SetVrchatSleepStatus(VrchatStatus),
    SetVrchatWakeStatus(VrchatStatus),
    SetInviteAutomation(bool),
    SetInviteProfile(InviteProfile),
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
