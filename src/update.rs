//! In-app updates, per the Release and Update Standard
//! (https://github.com/jackbrumley/jackbrumley.github.io/blob/main/RELEASE_STANDARD.md),
//! "Terminal apps": the check reads GitHub's latest release, and updating re-runs the same
//! one-line installer a new user runs.
//!
//! Windows: a running exe can't be replaced, so the installer runs in its own console window
//! and ComTap exits. install.ps1 asks for admin (UAC) itself, stops the service, swaps the
//! exe, starts the service again and opens the new version. If it fails, the installed
//! version starts again with `--update-failed`.
//!
//! Linux: ComTap leaves its screen and replaces itself with install.sh in the same terminal,
//! so its output (and the sudo password prompt) shows there.
use crate::{log, paths};
use serde::Deserialize;
#[cfg(any(windows, test))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

const GITHUB_LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/jackbrumley/comtap/releases/latest";
const INSTALL_SITE: &str = "https://brumley.info/comtap";
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_url: String,
}

#[derive(Deserialize)]
struct GitHubLatestRelease {
    tag_name: String,
    html_url: String,
}

/// Asks GitHub for the latest release. Blocking; `spawn_check` runs it off the UI thread.
pub fn check() -> Result<UpdateCheck, String> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    log!("Checking for updates at {GITHUB_LATEST_RELEASE_API_URL}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(CHECK_TIMEOUT))
        .build()
        .into();
    let release: GitHubLatestRelease = agent
        .get(GITHUB_LATEST_RELEASE_API_URL)
        .header("User-Agent", "comtap-update-checker")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| match error {
            ureq::Error::StatusCode(404) => {
                "No ComTap release is published on GitHub yet".to_string()
            }
            error => format!("Could not reach GitHub to check for updates: {error}"),
        })?
        .body_mut()
        .read_json()
        .map_err(|error| format!("Could not read GitHub's latest release: {error}"))?;
    let latest_version = normalize_version(&release.tag_name)
        .ok_or_else(|| format!("Invalid latest release tag: {}", release.tag_name))?;
    let current = parse_version(&current_version)
        .ok_or_else(|| format!("Invalid current version: {current_version}"))?;
    let latest = parse_version(&latest_version)
        .ok_or_else(|| format!("Invalid latest version: {latest_version}"))?;
    let update_available = latest > current;
    log!(
        "Update check: current {current_version}, latest {latest_version}, update available: {update_available}"
    );
    Ok(UpdateCheck {
        current_version,
        latest_version,
        update_available,
        release_url: release.html_url,
    })
}

/// Checks in a background thread; the result arrives on the returned channel.
pub fn spawn_check() -> Receiver<Result<UpdateCheck, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = check();
        if let Err(e) = &result {
            log!("Update check failed: {e}");
        }
        // The screen may have closed meanwhile; then nobody needs the result.
        let _ = tx.send(result);
    });
    rx
}

pub fn update_log() -> PathBuf {
    paths::logs_dir().join("update.log")
}

/// The message `--update-failed` shows.
pub fn update_failed_message() -> String {
    format!(
        "ComTap could not install the update, so this version is still installed. Update log: {}",
        update_log().display()
    )
}

/// The exe to start again with `--update-failed`: the installed copy, else this one.
#[cfg(windows)]
fn relaunch_target() -> Option<PathBuf> {
    let installed = paths::installed_exe();
    if installed.exists() {
        return Some(installed);
    }
    std::env::current_exe().ok()
}

/// Wraps `value` in single quotes for PowerShell, which escapes an embedded quote by doubling it.
#[cfg(any(windows, test))]
fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Downloads install.ps1 and runs it from the file, so its `exit 1` on failure ends the
/// script, not this updater, and sets `$LASTEXITCODE`. On failure the installed version
/// starts again with `--update-failed`.
#[cfg(any(windows, test))]
fn windows_update_script(relaunch: Option<&Path>) -> String {
    let relaunch = match relaunch {
        Some(path) => {
            let path = powershell_single_quote(&path.to_string_lossy());
            format!(
                "if (Test-Path -LiteralPath {path}) {{ Start-Process -FilePath {path} -ArgumentList '--update-failed' }}"
            )
        }
        None => "Write-Output '[comtap] No installed version found to start again'".to_string(),
    };
    format!(
        r#"$installer = Join-Path $env:TEMP 'comtap-update-install.ps1'
try {{
  Invoke-RestMethod -Uri '{INSTALL_SITE}/install.ps1' -OutFile $installer -ErrorAction Stop
  $LASTEXITCODE = 0
  & $installer
  $succeeded = $LASTEXITCODE -eq 0
}} catch {{
  Write-Output "[comtap] ERROR: $_"
  $succeeded = $false
}}
if (-not $succeeded) {{
  Write-Output '[comtap] Update failed; starting the installed version again'
  {relaunch}
  Read-Host 'Press Enter to close'
}}
"#
    )
}

/// Starts the update in its own console window. ComTap should exit straight after, so the
/// installer can replace comtap.exe.
#[cfg(windows)]
pub const STARTED_MESSAGE: &str =
    "The installer is running in a new window. Windows will ask for admin permission.";
#[cfg(not(windows))]
pub const STARTED_MESSAGE: &str = "The installer did not start.";

/// Windows: nothing to do (the installer already runs in its own window).
#[cfg(windows)]
pub fn run_pending() {}

#[cfg(not(windows))]
static PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The command `comtap update` and the screen run on Linux.
#[cfg(any(not(windows), test))]
fn linux_update_command() -> String {
    format!("curl --proto '=https' --tlsv1.2 -sSf {INSTALL_SITE}/install.sh | bash")
}

/// Linux: the screen has closed and the terminal is back to normal, so replace this process
/// with the installer. Returns only if that fails.
#[cfg(not(windows))]
pub fn run_pending() {
    use std::os::unix::process::CommandExt;
    if !PENDING.swap(false, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let cmd = linux_update_command();
    log!("Starting the update: {cmd}");
    println!("Updating ComTap: {cmd}");
    let e = std::process::Command::new("bash").args(["-c", &cmd]).exec();
    eprintln!("Could not start the update: {e}");
}

/// Linux: marks the update to run once the screen has closed (see `run_pending`).
#[cfg(not(windows))]
pub fn run_update() -> Result<(), String> {
    PENDING.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[cfg(windows)]
pub fn run_update() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let script = windows_update_script(relaunch_target().as_deref());
    log!("Starting the update:\n{script}");
    std::process::Command::new("powershell.exe")
        .env_remove("PSModulePath")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not start the update: {error}"))
}

/// Opens a web page in the default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    let mut cmd = std::process::Command::new("rundll32.exe");
    #[cfg(windows)]
    cmd.args(["url.dll,FileProtocolHandler", url]);
    #[cfg(not(windows))]
    let mut cmd = std::process::Command::new("xdg-open");
    #[cfg(not(windows))]
    cmd.arg(url);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open {url}: {e}"))
}

fn normalize_version(raw: &str) -> Option<String> {
    let parsed = parse_version(raw.trim().trim_start_matches('v'))?;
    Some(format!("{}.{}.{}", parsed.0, parsed.1, parsed.2))
}

/// Plain `MAJOR.MINOR.PATCH` (Release Standard section 2).
fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let mut segments = value.trim().split('.');
    let major = segments.next()?.parse::<u64>().ok()?;
    let minor = segments.next()?.parse::<u64>().ok()?;
    let patch = segments.next()?.parse::<u64>().ok()?;
    if segments.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_quoting_doubles_embedded_quotes() {
        assert_eq!(
            powershell_single_quote(r"C:\Program Files\o'brien\comtap.exe"),
            r"'C:\Program Files\o''brien\comtap.exe'"
        );
    }

    #[test]
    fn versions_normalize_from_tags() {
        assert_eq!(normalize_version("v1.0.0").as_deref(), Some("1.0.0"));
        assert_eq!(normalize_version("1.2"), None);
        assert_eq!(normalize_version("v1.2.3-beta.1"), None);
        assert!(parse_version("1.10.0") > parse_version("1.9.9"));
    }

    #[test]
    fn linux_update_runs_the_one_line_installer() {
        assert_eq!(
            linux_update_command(),
            "curl --proto '=https' --tlsv1.2 -sSf https://brumley.info/comtap/install.sh | bash"
        );
    }

    #[test]
    fn update_script_runs_the_installer_from_a_file_and_relaunches_on_failure() {
        let script = windows_update_script(Some(Path::new(r"C:\Program Files\ComTap\comtap.exe")));
        assert!(script.contains("https://brumley.info/comtap/install.ps1"));
        assert!(script.contains("& $installer"));
        assert!(
            script.contains(
                "'C:\\Program Files\\ComTap\\comtap.exe' -ArgumentList '--update-failed'"
            )
        );
    }
}
