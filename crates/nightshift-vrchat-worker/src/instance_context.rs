use std::{
    env,
    fs::{self, File},
    io::{self, BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

const VRCHAT_APP_ID: &str = "438100";
const PROTON_LOG_SUFFIX: &str =
    "pfx/drive_c/users/steamuser/AppData/LocalLow/VRChat/VRChat";
const LOG_PREFIX: &str = "output_log_";
const LOG_SUFFIX: &str = ".txt";
const JOIN_MARKER: &str = "] Joining ";

#[derive(Debug, Default)]
pub struct InstanceSource {
    path: Option<PathBuf>,
    offset: u64,
    current: Option<String>,
}

impl InstanceSource {
    pub fn poll(&mut self) -> io::Result<Option<String>> {
        // A persisted output log is not proof that the user is still in that instance. Keep the
        // location unavailable whenever the Proton VRChat process itself is gone.
        if !vrchat_process_active() {
            self.path = None;
            self.offset = 0;
            self.current = None;
            return Ok(None);
        }

        let next_path = latest_log_path();
        if next_path != self.path {
            self.path = next_path;
            self.offset = 0;
            self.current = None;
        }

        let Some(path) = self.path.as_deref() else {
            return Ok(None);
        };

        let (offset, locations) = read_new_locations(path, self.offset)?;
        self.offset = offset;
        if let Some(instance_id) = locations.into_iter().last() {
            self.current = Some(instance_id);
        }
        Ok(self.current.clone())
    }
}

/// Parse only VRChat's location-change line. This follows the same narrow shape used by
/// OyasumiVR and intentionally ignores the similarly named room/friend diagnostic messages.
pub fn parse_instance_id(line: &str) -> Option<&str> {
    if !line.contains("[Behaviour] Joining ")
        || line.contains("] Joining or Creating Room: ")
        || line.contains("] Joining friend: ")
    {
        return None;
    }

    let offset = line.rfind(JOIN_MARKER)? + JOIN_MARKER.len();
    let instance_id = line.get(offset..)?.trim();
    (!instance_id.is_empty()).then_some(instance_id)
}

fn vrchat_process_active() -> bool {
    let Ok(entries) = fs::read_dir("/proc") else {
        return false;
    };

    entries.filter_map(Result::ok).any(|entry| {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }

        let comm = fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
        if looks_like_vrchat_process(&comm) {
            return true;
        }

        let cmdline = fs::read(entry.path().join("cmdline")).unwrap_or_default();
        let cmdline = String::from_utf8_lossy(&cmdline);
        looks_like_vrchat_process(&cmdline)
    })
}

fn looks_like_vrchat_process(value: &str) -> bool {
    value.to_ascii_lowercase().contains("vrchat.exe")
}

fn latest_log_path() -> Option<PathBuf> {
    candidate_log_dirs()
        .into_iter()
        .filter_map(|dir| fs::read_dir(dir).ok())
        .flat_map(|entries| entries.filter_map(Result::ok))
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(LOG_PREFIX) && name.ends_with(LOG_SUFFIX)
        })
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            // Keep empty logs in the candidate set. A freshly started VRChat creates the new
            // output log before it necessarily writes a location line; falling back to the older
            // non-empty log during that window could expose the previous session's instance.
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            Some((modified, path))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

fn candidate_log_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(path) = env::var_os("NIGHTSHIFT_VRCHAT_LOG_DIR") {
        dirs.push(PathBuf::from(path));
    }

    let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
        return dirs;
    };

    let suffix = Path::new("steamapps")
        .join("compatdata")
        .join(VRCHAT_APP_ID)
        .join(PROTON_LOG_SUFFIX);

    // Native Steam packages commonly use one of these two roots.
    dirs.push(home.join(".local/share/Steam").join(&suffix));
    dirs.push(home.join(".steam/steam").join(&suffix));

    // Valve's Flatpak package keeps the Steam tree below the app sandbox.
    dirs.push(
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam")
            .join(&suffix),
    );

    // Steam's Snap package uses a per-user common data root.
    dirs.push(
        home.join("snap/steam/common/.local/share/Steam")
            .join(&suffix),
    );

    dirs
}

fn read_new_locations(path: &Path, offset: u64) -> io::Result<(u64, Vec<String>)> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut committed_offset = if offset <= len { offset } else { 0 };

    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(committed_offset))?;
    let mut locations = Vec::new();
    let mut bytes = Vec::new();

    loop {
        bytes.clear();
        let line_start = committed_offset;
        let read = reader.read_until(b'\n', &mut bytes)?;
        if read == 0 {
            break;
        }

        // Leave a partially written final line for the next poll.
        if !bytes.ends_with(b"\n") {
            committed_offset = line_start;
            break;
        }
        committed_offset = committed_offset.saturating_add(read as u64);

        let line = String::from_utf8_lossy(&bytes);
        if let Some(instance_id) = parse_instance_id(line.trim_end_matches(['\r', '\n'])) {
            locations.push(instance_id.to_owned());
        }
    }

    Ok((committed_offset, locations))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vrchat_joining_instance_line() {
        let line = "2026.10.24 01:23:45 Log -  [Behaviour] Joining wrld_abc:12345~region(eu)";
        assert_eq!(
            parse_instance_id(line),
            Some("wrld_abc:12345~region(eu)")
        );
    }

    #[test]
    fn ignores_similarly_named_non_location_lines() {
        assert_eq!(
            parse_instance_id(
                "2026.10.24 01:23:45 Log -  [Behaviour] Joining or Creating Room: test"
            ),
            None
        );
        assert_eq!(
            parse_instance_id("2026.10.24 01:23:45 Log -  [Behaviour] Joining friend: Bob"),
            None
        );
        assert_eq!(parse_instance_id("unrelated"), None);
    }

    #[test]
    fn empty_instance_is_ignored() {
        assert_eq!(
            parse_instance_id("2026.10.24 01:23:45 Log -  [Behaviour] Joining "),
            None
        );
    }

    #[test]
    fn proton_process_match_is_case_insensitive_and_specific() {
        assert!(looks_like_vrchat_process("VRChat.exe\n"));
        assert!(looks_like_vrchat_process(
            "C:\\Program Files (x86)\\Steam\\steamapps\\common\\VRChat\\VRChat.exe"
        ));
        assert!(!looks_like_vrchat_process("vrserver.exe"));
        assert!(!looks_like_vrchat_process("nightshift-vrchat"));
    }
}
