use std::path::PathBuf;

fn env_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(fallback))
}

/// C:\Program Files\ComTap: the installed exe and the com0com files.
pub fn program_dir() -> PathBuf {
    env_dir("ProgramFiles", r"C:\Program Files").join("ComTap")
}

/// C:\ProgramData\ComTap: config, status and logs.
pub fn data_dir() -> PathBuf {
    env_dir("ProgramData", r"C:\ProgramData").join("ComTap")
}

pub fn installed_exe() -> PathBuf {
    program_dir().join("comtap.exe")
}

pub fn installed_com0com() -> PathBuf {
    program_dir().join("com0com")
}

pub fn installed_config() -> PathBuf {
    data_dir().join("comtap_config.json")
}

pub fn status_file() -> PathBuf {
    data_dir().join("status.json")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
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
