use std::{
    env,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use nightshift_protocol::{AppState, OverlaySpawnMode, SleepPhase};

const MAX_MANAGED_REPORTS: usize = 10;
const REPORT_PREFIX: &str = "nightshift-debug-";
const REPORT_SUFFIX: &str = ".txt";

pub(crate) fn write_debug_report(
    state: &AppState,
    requested_path: Option<PathBuf>,
) -> Result<PathBuf> {
    let managed = requested_path.is_none();
    let path = match requested_path {
        Some(path) => path,
        None => default_report_path()?,
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating diagnostics directory {}", parent.display()))?;
    }

    let report = build_report(state)?;
    fs::write(&path, report)
        .with_context(|| format!("writing Nightshift debug report to {}", path.display()))?;

    if managed && let Some(parent) = path.parent() {
        // Diagnostics are useful. A diagnostics landfill is less useful. Hydra gets ten boxes.
        let _ = prune_managed_reports(parent, MAX_MANAGED_REPORTS);
    }

    Ok(path)
}

fn default_report_path() -> Result<PathBuf> {
    let timestamp = unix_timestamp()?;
    Ok(state_home()?
        .join("nightshift")
        .join("diagnostics")
        .join(format!("{REPORT_PREFIX}{timestamp}{REPORT_SUFFIX}")))
}

fn state_home() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path));
    }
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".local").join("state"))
}

fn unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_secs())
}

fn build_report(state: &AppState) -> Result<String> {
    let mut report = String::new();
    let timestamp = unix_timestamp()?;

    writeln!(report, "Nightshift debug report")?;
    writeln!(report, "version: {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(report, "unix_time: {timestamp}")?;
    writeln!(report)?;

    write_state_summary(&mut report, state)?;
    write_system_summary(&mut report)?;
    write_process_summary(&mut report)?;

    writeln!(report, "privacy")?;
    writeln!(report, "  credentials/session cookies: not collected")?;
    writeln!(report, "  process command lines/env: not collected")?;
    writeln!(report, "  friend/profile identities: not collected")?;

    Ok(report)
}

fn write_state_summary(report: &mut String, state: &AppState) -> Result<()> {
    writeln!(report, "nightshift_state")?;
    writeln!(report, "  sleep_phase: {}", phase_name(state.sleep.phase))?;
    writeln!(
        report,
        "  brightness_current_percent: {:.1}",
        state.brightness.current * 100.0
    )?;
    writeln!(
        report,
        "  brightness_target_percent: {:.1}",
        state.brightness.target * 100.0
    )?;
    writeln!(
        report,
        "  sleep_brightness_percent: {:.1}",
        state.brightness.sleep_target * 100.0
    )?;
    writeln!(
        report,
        "  fade_seconds: {:.2}",
        state.settings.fade_duration_seconds
    )?;
    writeln!(
        report,
        "  overlay_runtime_connected: {}",
        state.overlay.runtime_connected
    )?;
    writeln!(
        report,
        "  overlay_runtime_version: {}",
        state.overlay.runtime_client_version.as_deref().unwrap_or("unknown")
    )?;
    writeln!(report, "  overlay_visible: {}", state.overlay.visible)?;
    writeln!(
        report,
        "  overlay_distance_m: {:.2}",
        state.overlay.spawn_distance_m
    )?;
    writeln!(
        report,
        "  overlay_mode: {}",
        overlay_mode_name(state.overlay.spawn_mode)
    )?;
    writeln!(report, "  vrchat_connected: {}", state.vrchat.connected)?;
    writeln!(report, "  osc_enabled: {}", state.osc.enabled)?;
    writeln!(report, "  osc_connected: {}", state.osc.connected)?;
    writeln!(
        report,
        "  osc_avatar_parameter_count: {}",
        state.osc.avatar_parameter_count
    )?;
    writeln!(report, "  gogo_detected: {}", state.osc.gogo_detected)?;
    writeln!(report)?;
    Ok(())
}

fn write_system_summary(report: &mut String) -> Result<()> {
    writeln!(report, "system")?;
    write_optional_proc_line(report, "loadavg", "/proc/loadavg")?;
    write_optional_proc_line(report, "uptime_seconds", "/proc/uptime")?;

    if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
        for key in ["MemTotal", "MemAvailable", "SwapTotal", "SwapFree"] {
            if let Some(value) = proc_status_value(&meminfo, key) {
                writeln!(report, "  {}: {}", key.to_ascii_lowercase(), value.trim())?;
            }
        }
    }
    writeln!(report)?;
    Ok(())
}

fn write_optional_proc_line(report: &mut String, label: &str, path: &str) -> Result<()> {
    if let Ok(value) = fs::read_to_string(path) {
        let value = value.lines().next().unwrap_or_default().trim();
        if !value.is_empty() {
            writeln!(report, "  {label}: {value}")?;
        }
    }
    Ok(())
}

#[derive(Debug)]
struct ProcessSnapshot {
    pid: u32,
    name: String,
    rss: Option<String>,
    virtual_size: Option<String>,
    threads: Option<String>,
    fd_count: Option<usize>,
    cpu_runtime_ms: Option<u64>,
}

fn write_process_summary(report: &mut String) -> Result<()> {
    let mut processes = nightshift_processes();
    processes.sort_by_key(|process| process.pid);

    writeln!(report, "nightshift_processes")?;
    writeln!(report, "  count: {}", processes.len())?;

    let mut known_rss_kib = 0u64;
    for process in &processes {
        if let Some(rss) = process.rss.as_deref().and_then(parse_kib) {
            known_rss_kib = known_rss_kib.saturating_add(rss);
        }
    }
    writeln!(report, "  known_total_rss_kib: {known_rss_kib}")?;

    for process in processes {
        writeln!(report, "  - pid: {}", process.pid)?;
        writeln!(report, "    name: {}", process.name)?;
        writeln!(
            report,
            "    rss: {}",
            process.rss.as_deref().unwrap_or("unknown")
        )?;
        writeln!(
            report,
            "    virtual_size: {}",
            process.virtual_size.as_deref().unwrap_or("unknown")
        )?;
        writeln!(
            report,
            "    threads: {}",
            process.threads.as_deref().unwrap_or("unknown")
        )?;
        match process.fd_count {
            Some(count) => writeln!(report, "    open_fds: {count}")?,
            None => writeln!(report, "    open_fds: unknown")?,
        }
        match process.cpu_runtime_ms {
            Some(ms) => writeln!(report, "    cpu_runtime_ms: {ms}")?,
            None => writeln!(report, "    cpu_runtime_ms: unknown")?,
        }
    }
    writeln!(report)?;
    Ok(())
}

fn nightshift_processes() -> Vec<ProcessSnapshot> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
            read_process_snapshot(pid)
        })
        .collect()
}

fn read_process_snapshot(pid: u32) -> Option<ProcessSnapshot> {
    let base = PathBuf::from("/proc").join(pid.to_string());
    let name = fs::read_to_string(base.join("comm")).ok()?.trim().to_string();
    if !name.starts_with("nightshift") {
        return None;
    }

    let status = fs::read_to_string(base.join("status")).ok();
    let rss = status
        .as_deref()
        .and_then(|status| proc_status_value(status, "VmRSS"))
        .map(str::to_string);
    let virtual_size = status
        .as_deref()
        .and_then(|status| proc_status_value(status, "VmSize"))
        .map(str::to_string);
    let threads = status
        .as_deref()
        .and_then(|status| proc_status_value(status, "Threads"))
        .map(str::to_string);
    let fd_count = fs::read_dir(base.join("fd")).ok().map(|fds| fds.count());
    let cpu_runtime_ms = fs::read_to_string(base.join("schedstat"))
        .ok()
        .and_then(|value| value.split_whitespace().next()?.parse::<u64>().ok())
        .map(|nanoseconds| nanoseconds / 1_000_000);

    Some(ProcessSnapshot {
        pid,
        name,
        rss,
        virtual_size,
        threads,
        fd_count,
        cpu_runtime_ms,
    })
}

fn proc_status_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name == key).then_some(value.trim())
    })
}

fn parse_kib(value: &str) -> Option<u64> {
    value.split_whitespace().next()?.parse().ok()
}

fn prune_managed_reports(directory: &Path, keep: usize) -> Result<()> {
    let mut reports = fs::read_dir(directory)?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            is_managed_report_name(name).then_some((name.to_string(), entry.path()))
        })
        .collect::<Vec<_>>();

    reports.sort_by(|a, b| a.0.cmp(&b.0));
    let remove_count = reports.len().saturating_sub(keep);
    for (_, path) in reports.into_iter().take(remove_count) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn is_managed_report_name(name: &str) -> bool {
    let Some(timestamp) = name
        .strip_prefix(REPORT_PREFIX)
        .and_then(|name| name.strip_suffix(REPORT_SUFFIX))
    else {
        return false;
    };
    !timestamp.is_empty() && timestamp.bytes().all(|byte| byte.is_ascii_digit())
}

fn phase_name(phase: SleepPhase) -> &'static str {
    match phase {
        SleepPhase::Awake => "awake",
        SleepPhase::EnteringSleep => "entering_sleep",
        SleepPhase::Sleeping => "sleeping",
        SleepPhase::Waking => "waking",
    }
}

fn overlay_mode_name(mode: OverlaySpawnMode) -> &'static str {
    match mode {
        OverlaySpawnMode::InFront => "in-front",
        OverlaySpawnMode::RestoreLastPosition => "restore-last-position",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_status_values() {
        let status = "Name:\tnightshift\nVmRSS:\t  1234 kB\nThreads:\t4\n";
        assert_eq!(proc_status_value(status, "VmRSS"), Some("1234 kB"));
        assert_eq!(proc_status_value(status, "Threads"), Some("4"));
        assert_eq!(parse_kib("1234 kB"), Some(1234));
    }

    #[test]
    fn managed_report_names_are_strict() {
        assert!(is_managed_report_name("nightshift-debug-1234567890.txt"));
        assert!(!is_managed_report_name("nightshift-debug-not-a-time.txt"));
        assert!(!is_managed_report_name("my-nightshift-debug-123.txt"));
        assert!(!is_managed_report_name("nightshift-debug-123.log"));
    }
}
