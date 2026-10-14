use std::path::{Path, PathBuf};
use sysinfo::{Disks, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameProcess {
    pub pid: u32,
    pub started: u64,
    pub executable: Option<PathBuf>,
}

pub fn running_game(system: &mut System, channel: &str) -> Option<GameProcess> {
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    system
        .processes()
        .iter()
        .filter(|(_, process)| {
            process
                .name()
                .to_string_lossy()
                .eq_ignore_ascii_case("StarCitizen.exe")
        })
        .filter(|(_, process)| {
            process
                .exe()
                .and_then(channel_from_exe)
                .is_none_or(|c| c.eq_ignore_ascii_case(channel))
        })
        .max_by_key(|(_, process)| process.start_time())
        .map(|(pid, process)| GameProcess {
            pid: pid.as_u32(),
            started: process.start_time(),
            executable: process.exe().map(Path::to_path_buf),
        })
}

fn channel_from_exe(exe: &Path) -> Option<&str> {
    exe.parent()?.parent()?.file_name()?.to_str()
}

pub fn log_from_exe(exe: &Path) -> Option<PathBuf> {
    // ...\LIVE\Bin64\StarCitizen.exe
    Some(exe.parent()?.parent()?.join("Game.log"))
}

pub fn candidate_logs(channel: &str, game: Option<&GameProcess>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = game
        .and_then(|g| g.executable.as_deref())
        .and_then(log_from_exe)
    {
        paths.push(path);
    }
    for key in ["LOCALAPPDATA", "APPDATA"] {
        if let Some(root) = std::env::var_os(key) {
            for vendor in ["Star Citizen", "Roberts Space Industries"] {
                paths.push(
                    PathBuf::from(&root)
                        .join(vendor)
                        .join("StarCitizen")
                        .join(channel)
                        .join("Game.log"),
                );
            }
        }
    }
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(key) {
            paths.push(
                PathBuf::from(root)
                    .join("Roberts Space Industries/StarCitizen")
                    .join(channel)
                    .join("Game.log"),
            );
        }
    }
    // Small fixed set on every mounted volume; never recursively scan the user's disk.
    for disk in Disks::new_with_refreshed_list().list() {
        for relative in [
            "Roberts Space Industries/StarCitizen",
            "Program Files/Roberts Space Industries/StarCitizen",
            "Games/StarCitizen",
            "Games/Roberts Space Industries/StarCitizen",
            "StarCitizen",
        ] {
            paths.push(
                disk.mount_point()
                    .join(relative)
                    .join(channel)
                    .join("Game.log"),
            );
        }
    }
    let mut unique = Vec::new();
    for path in paths {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    unique
}

pub fn discover_log(channel: &str, game: Option<&GameProcess>) -> Option<PathBuf> {
    let paths = candidate_logs(channel, game);
    // Prefer the active executable's installation even if its log is not created yet.
    if let Some(path) = game
        .and_then(|g| g.executable.as_deref())
        .and_then(log_from_exe)
    {
        return Some(path);
    }
    paths
        .into_iter()
        .filter(|p| p.is_file())
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn derives_the_channel_log_from_game_not_launcher() {
        let exe = Path::new("Games/StarCitizen/EPTU/Bin64/StarCitizen.exe");
        assert_eq!(
            log_from_exe(exe).unwrap(),
            PathBuf::from("Games/StarCitizen/EPTU/Game.log")
        );
        assert_eq!(channel_from_exe(exe), Some("EPTU"));
    }
}
