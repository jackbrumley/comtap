//! Where ComTap keeps its files.
//!
//! Windows: the exe and com0com files in C:\Program Files\ComTap; config, status, logs and
//! traffic in C:\ProgramData\ComTap.
//! Linux: the binary at /usr/local/bin/comtap; config in /etc/comtap; status and traffic in
//! /var/lib/comtap; logs in /var/log/comtap.
use std::path::PathBuf;

#[cfg(windows)]
fn env_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(fallback))
}

/// C:\Program Files\ComTap: the installed exe and the com0com files.
#[cfg(windows)]
pub fn program_dir() -> PathBuf {
    env_dir("ProgramFiles", r"C:\Program Files").join("ComTap")
}

/// /usr/local/bin: the installed binary (already on every user's PATH).
#[cfg(not(windows))]
pub fn program_dir() -> PathBuf {
    PathBuf::from("/usr/local/bin")
}

/// C:\ProgramData\ComTap: config, status and logs.
#[cfg(windows)]
pub fn data_dir() -> PathBuf {
    env_dir("ProgramData", r"C:\ProgramData").join("ComTap")
}

/// /var/lib/comtap: status and traffic.
#[cfg(not(windows))]
pub fn data_dir() -> PathBuf {
    PathBuf::from("/var/lib/comtap")
}

#[cfg(windows)]
pub fn installed_exe() -> PathBuf {
    program_dir().join("comtap.exe")
}

#[cfg(not(windows))]
pub fn installed_exe() -> PathBuf {
    program_dir().join("comtap")
}

#[cfg(windows)]
pub fn installed_com0com() -> PathBuf {
    program_dir().join("com0com")
}

#[cfg(windows)]
pub fn installed_config() -> PathBuf {
    data_dir().join("comtap_config.json")
}

#[cfg(not(windows))]
pub fn installed_config() -> PathBuf {
    PathBuf::from("/etc/comtap/comtap_config.json")
}

pub fn status_file() -> PathBuf {
    data_dir().join("status.json")
}

#[cfg(windows)]
pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

#[cfg(not(windows))]
pub fn logs_dir() -> PathBuf {
    PathBuf::from("/var/log/comtap")
}

pub fn traffic_dir() -> PathBuf {
    data_dir().join("traffic")
}

/// ComTap.lnk on the Public Desktop (every user sees it).
#[cfg(windows)]
pub fn shortcut() -> PathBuf {
    env_dir("PUBLIC", r"C:\Users\Public")
        .join("Desktop")
        .join("ComTap.lnk")
}

/// The systemd unit that runs ComTap at boot.
#[cfg(not(windows))]
pub fn systemd_unit() -> PathBuf {
    PathBuf::from("/etc/systemd/system/comtap.service")
}

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// --config if given, else the installed config, else comtap_config.json next to the exe.
pub fn resolve_config(arg: Option<PathBuf>) -> PathBuf {
    if let Some(p) = arg {
        return p;
    }
    let installed = installed_config();
    if installed.exists() {
        return installed;
    }
    exe_dir().join("comtap_config.json")
}
