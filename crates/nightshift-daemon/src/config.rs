use std::{
    env,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use nightshift_protocol::{
    AppState, InviteProfile, InviteRuleSettings, OverlayPose, OverlaySpawnMode, VrchatStatus,
};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedConfig {
    pub sleep_brightness: f32,
    pub fade_duration_seconds: f32,
    pub hr_monitoring: bool,
    pub hr_device_filter: Option<String>,
    pub hr_threshold_bpm: u16,
    pub hr_calm_period_seconds: u64,
    pub hr_cooldown_seconds: u64,
    pub sleep_detection: bool,
    pub osc_automation: bool,
    pub gogo_automation: bool,
    pub vrchat_status_automation: bool,
    pub sleep_vrchat_status: VrchatStatus,
    pub wake_vrchat_status: VrchatStatus,
    pub invite_automation: bool,
    pub invite_profile: InviteProfile,
    pub invite_rules: InviteRuleSettings,
    pub overlay_spawn_distance_m: f32,
    pub overlay_spawn_mode: OverlaySpawnMode,
    pub overlay_laser_enabled: bool,
    pub overlay_remembered_pose: Option<OverlayPose>,
}

impl Default for PersistedConfig {
    fn default() -> Self {
        Self::from_state(&AppState::default())
    }
}

pub fn sanitize_overlay_pose(pose: OverlayPose) -> Option<OverlayPose> {
    if !pose.position.iter().all(|component| component.is_finite())
        || !pose.orientation_xyzw.iter().all(|component| component.is_finite())
    {
        return None;
    }

    let norm_sq = pose
        .orientation_xyzw
        .iter()
        .map(|component| component * component)
        .sum::<f32>();
    if !norm_sq.is_finite() || norm_sq <= f32::EPSILON {
        return None;
    }
    let inverse_norm = norm_sq.sqrt().recip();
    let orientation_xyzw = pose.orientation_xyzw.map(|component| component * inverse_norm);
    if !orientation_xyzw.iter().all(|component| component.is_finite()) {
        return None;
    }

    Some(OverlayPose {
        position: pose.position,
        orientation_xyzw,
    })
}

impl PersistedConfig {
    pub fn from_state(state: &AppState) -> Self {
        Self {
            sleep_brightness: state.brightness.sleep_target,
            fade_duration_seconds: state.settings.fade_duration_seconds,
            hr_monitoring: state.heart_rate.enabled,
            hr_device_filter: state.heart_rate.device_filter.clone(),
            hr_threshold_bpm: state.heart_rate.threshold_bpm,
            hr_calm_period_seconds: state.heart_rate.calm_period_seconds,
            hr_cooldown_seconds: state.heart_rate.cooldown_seconds,
            sleep_detection: state.sleep.detection_enabled,
            osc_automation: state.osc.enabled,
            gogo_automation: state.settings.gogo_automation_enabled,
            vrchat_status_automation: state.settings.vrchat_status_automation_enabled,
            sleep_vrchat_status: state.settings.sleep_vrchat_status,
            wake_vrchat_status: state.settings.wake_vrchat_status,
            invite_automation: state.invites.enabled,
            invite_profile: state.invites.profile,
            invite_rules: state.invites.rules.clone(),
            overlay_spawn_distance_m: state.overlay.spawn_distance_m,
            overlay_spawn_mode: state.overlay.spawn_mode,
            overlay_laser_enabled: state.overlay.laser_enabled,
            overlay_remembered_pose: state.overlay.remembered_pose.and_then(sanitize_overlay_pose),
        }
    }

    pub fn apply_to_state(&self, state: &mut AppState) {
        state.brightness.sleep_target = self.sleep_brightness.clamp(0.02, 1.0);
        state.settings.fade_duration_seconds = self.fade_duration_seconds.clamp(0.0, 120.0);
        state.heart_rate.enabled = self.hr_monitoring;
        state.heart_rate.device_filter = self
            .hr_device_filter
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        state.heart_rate.threshold_bpm = self.hr_threshold_bpm.clamp(20, 240);
        state.heart_rate.calm_period_seconds = self.hr_calm_period_seconds.clamp(10, 3_600);
        state.heart_rate.cooldown_seconds = self.hr_cooldown_seconds.min(24 * 60 * 60);
        state.sleep.detection_enabled = self.sleep_detection;
        state.osc.enabled = self.osc_automation;
        state.settings.gogo_automation_enabled = self.gogo_automation;
        state.settings.vrchat_status_automation_enabled = self.vrchat_status_automation;
        state.settings.sleep_vrchat_status = self.sleep_vrchat_status;
        state.settings.wake_vrchat_status = self.wake_vrchat_status;
        state.invites.enabled = self.invite_automation;
        state.invites.profile = self.invite_profile;
        state.invites.rules = self.invite_rules.clone();
        state.overlay.spawn_distance_m = self.overlay_spawn_distance_m.clamp(0.25, 5.0);
        state.overlay.spawn_mode = self.overlay_spawn_mode;
        state.overlay.laser_enabled = self.overlay_laser_enabled;
        state.overlay.remembered_pose = self.overlay_remembered_pose.and_then(sanitize_overlay_pose);
    }

    pub async fn load() -> Result<Self> {
        let path = config_path()?;
        if !tokio::fs::try_exists(&path).await? {
            return Ok(Self::default());
        }

        let bytes = tokio::fs::read(&path)
            .await
            .with_context(|| format!("reading {}", path.display()))?;
        match serde_json::from_slice(&bytes) {
            Ok(config) => Ok(config),
            Err(error) => {
                let backup_path = path.with_extension("json.bak");
                write_private_file(&backup_path, &bytes)
                    .await
                    .with_context(|| {
                        format!(
                            "backing up unparseable config {} to {}",
                            path.display(),
                            backup_path.display()
                        )
                    })?;
                Err(error).with_context(|| {
                    format!(
                        "parsing {}; corrupt bytes preserved at {}",
                        path.display(),
                        backup_path.display()
                    )
                })
            }
        }
    }

    pub async fn save(&self) -> Result<()> {
        let path = config_path()?;
        let parent = path.parent().context("config path has no parent")?;
        tokio::fs::create_dir_all(parent).await?;
        tokio::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .await
            .with_context(|| format!("securing config directory {}", parent.display()))?;

        let tmp_path = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self)?;
        write_private_file(&tmp_path, &bytes).await?;
        tokio::fs::rename(&tmp_path, &path)
            .await
            .with_context(|| format!("replacing {}", path.display()))?;
        tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .await
            .with_context(|| format!("securing {}", path.display()))?;
        Ok(())
    }
}

async fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true).mode(0o600);
    let mut file = options
        .open(path)
        .await
        .with_context(|| format!("opening {}", path.display()))?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .await
        .with_context(|| format!("securing {}", path.display()))?;
    file.write_all(bytes)
        .await
        .with_context(|| format!("writing {}", path.display()))?;
    file.flush()
        .await
        .with_context(|| format!("flushing {}", path.display()))?;
    file.sync_all()
        .await
        .with_context(|| format!("syncing {}", path.display()))?;
    Ok(())
}

fn config_path() -> Result<PathBuf> {
    if let Some(dir) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(dir).join("nightshift").join("config.json"));
    }

    let home = env::var_os("HOME").context("neither XDG_CONFIG_HOME nor HOME is set")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("nightshift")
        .join("config.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "nightshift-config-test-{}-{nonce}-{name}",
            std::process::id()
        ))
    }

    #[test]
    fn overlay_settings_round_trip_through_persisted_config() {
        let mut state = AppState::default();
        state.overlay.spawn_distance_m = 2.25;
        state.overlay.spawn_mode = OverlaySpawnMode::RestoreLastPosition;
        state.overlay.laser_enabled = false;
        state.overlay.remembered_pose = Some(OverlayPose {
            position: [1.0, 2.0, -3.0],
            orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
        });

        let config = PersistedConfig::from_state(&state);
        let mut restored = AppState::default();
        config.apply_to_state(&mut restored);

        assert_eq!(restored.overlay.spawn_distance_m, 2.25);
        assert_eq!(
            restored.overlay.spawn_mode,
            OverlaySpawnMode::RestoreLastPosition
        );
        assert!(!restored.overlay.laser_enabled);
        assert_eq!(restored.overlay.remembered_pose, state.overlay.remembered_pose);
    }

    #[test]
    fn overlay_spawn_distance_is_sanitized_when_loaded() {
        let mut config = PersistedConfig::default();
        config.overlay_spawn_distance_m = 99.0;
        let mut state = AppState::default();
        config.apply_to_state(&mut state);
        assert_eq!(state.overlay.spawn_distance_m, 5.0);

        config.overlay_spawn_distance_m = 0.0;
        config.apply_to_state(&mut state);
        assert_eq!(state.overlay.spawn_distance_m, 0.25);
    }

    #[test]
    fn overlay_pose_is_normalized_and_invalid_values_are_rejected() {
        let pose = OverlayPose {
            position: [1.0, 2.0, 3.0],
            orientation_xyzw: [0.0, 0.0, 0.0, 2.0],
        };
        assert_eq!(
            sanitize_overlay_pose(pose).unwrap().orientation_xyzw,
            [0.0, 0.0, 0.0, 1.0]
        );

        assert!(sanitize_overlay_pose(OverlayPose {
            position: [f32::NAN, 0.0, 0.0],
            orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
        })
        .is_none());
        assert!(sanitize_overlay_pose(OverlayPose {
            position: [0.0; 3],
            orientation_xyzw: [0.0; 4],
        })
        .is_none());
    }

    #[test]
    fn hr_settings_round_trip_through_persisted_config() {
        let mut state = AppState::default();
        state.heart_rate.enabled = true;
        state.heart_rate.device_filter = Some("Polar H10".into());
        state.heart_rate.threshold_bpm = 64;
        state.heart_rate.calm_period_seconds = 180;
        state.heart_rate.cooldown_seconds = 600;

        let config = PersistedConfig::from_state(&state);
        let mut restored = AppState::default();
        config.apply_to_state(&mut restored);

        assert!(restored.heart_rate.enabled);
        assert_eq!(restored.heart_rate.device_filter.as_deref(), Some("Polar H10"));
        assert_eq!(restored.heart_rate.threshold_bpm, 64);
        assert_eq!(restored.heart_rate.calm_period_seconds, 180);
        assert_eq!(restored.heart_rate.cooldown_seconds, 600);
    }

    #[test]
    fn hr_settings_are_sanitized_when_loaded() {
        let mut config = PersistedConfig::default();
        config.hr_device_filter = Some("   ".into());
        config.hr_threshold_bpm = 1;
        config.hr_calm_period_seconds = 1;
        config.hr_cooldown_seconds = u64::MAX;

        let mut state = AppState::default();
        config.apply_to_state(&mut state);

        assert_eq!(state.heart_rate.device_filter, None);
        assert_eq!(state.heart_rate.threshold_bpm, 20);
        assert_eq!(state.heart_rate.calm_period_seconds, 10);
        assert_eq!(state.heart_rate.cooldown_seconds, 24 * 60 * 60);
    }

    #[test]
    fn hr_device_filter_is_trimmed_when_loaded() {
        let mut config = PersistedConfig::default();
        config.hr_device_filter = Some("  AA:BB:CC:DD:EE:FF  ".into());

        let mut state = AppState::default();
        config.apply_to_state(&mut state);

        assert_eq!(
            state.heart_rate.device_filter.as_deref(),
            Some("AA:BB:CC:DD:EE:FF")
        );
    }

    #[tokio::test]
    async fn private_file_writer_enforces_owner_only_permissions() {
        let path = unique_temp_path("permissions");
        write_private_file(&path, b"secret settings").await.unwrap();
        let metadata = tokio::fs::metadata(&path).await.unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"secret settings");
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn corrupt_config_bytes_can_be_preserved_privately() {
        let path = unique_temp_path("backup.json.bak");
        let bytes = b"{ definitely not valid json";
        write_private_file(&path, bytes).await.unwrap();
        assert_eq!(tokio::fs::read(&path).await.unwrap(), bytes);
        let metadata = tokio::fs::metadata(&path).await.unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        let _ = tokio::fs::remove_file(path).await;
    }
}
