//! In-app updates, per the Release and Update Standard
//! (https://github.com/jackbrumley/jackbrumley.github.io/blob/main/RELEASE_STANDARD.md),
//! section 7, "Terminal apps": the check reads GitHub's latest release, and updating runs the
//! website's update script (`docs/update.sh` / `docs/update.ps1`), which owns the whole
//! procedure. ComTap only starts it, so a fix to the update reaches every installed version
//! without a release.
//!
//! Windows: a running exe can't be replaced, so the script runs in its own console window and
//! ComTap exits. Its output shows in that window and goes to `update.log`.
//!
//! Linux: ComTap leaves its screen and replaces itself with the script in the same terminal,
//! so its output (and the sudo password prompt) shows there.
use crate::{log, paths};
use serde::Deserialize;
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

/// What the user is told when the update script could not be started.
const NOT_STARTED_MESSAGE: &str = "The updater could not start, so nothing was changed. Download the new version from the release page instead: https://github.com/jackbrumley/comtap/releases/latest";

/// Replaces the update scripts' base URL. Only the release's install test sets it.
const BASE_URL_OVERRIDE_VARIABLE: &str = "COMTAP_UPDATE_BASE_URL";

/// Where `update.sh` / `update.ps1` (and the installers they run) come from.
fn base_url() -> Result<String, String> {
    parse_base_url(std::env::var(BASE_URL_OVERRIDE_VARIABLE).ok())
}

/// The install site, or the override, which must be https and loses any trailing slash.
fn parse_base_url(override_url: Option<String>) -> Result<String, String> {
    let Some(override_url) = override_url else {
        return Ok(INSTALL_SITE.to_string());
    };
    let trimmed = override_url.trim().trim_end_matches('/');
    if !trimmed.starts_with("https://") {
        return Err(format!(
            "{BASE_URL_OVERRIDE_VARIABLE} must be an https:// URL, got '{override_url}'"
        ));
    }
    Ok(trimmed.to_string())
}

/// The program the script opens with `--update-failed`: on Windows the installed copy when
/// there is one, otherwise this one.
fn relaunch_target() -> String {
    #[cfg(windows)]
    {
        let installed = paths::installed_exe();
        if installed.exists() {
            return installed.to_string_lossy().into_owned();
        }
    }
    std::env::current_exe()
        .map(|exe| exe.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Wraps `value` in single quotes for PowerShell, which escapes an embedded quote by doubling it.
#[cfg(any(windows, test))]
fn powershell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// The PowerShell the new console window runs: download update.ps1 and run it. If the download
/// fails, the window stays open with the error and the release page.
#[cfg(any(windows, test))]
fn windows_launcher(base_url: &str, relaunch: &str) -> String {
    let script_url = powershell_single_quote(&format!("{base_url}/update.ps1"));
    let base_url = powershell_single_quote(base_url);
    let relaunch = powershell_single_quote(relaunch);
    let not_started = powershell_single_quote(NOT_STARTED_MESSAGE);
    format!(
        r#"[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
try {{
  $updater = Invoke-RestMethod -Uri {script_url} -ErrorAction Stop
}} catch {{
  Write-Host "[comtap-update] ERROR: could not download the updater: $_" -ForegroundColor Red
  Write-Host {not_started}
  Read-Host 'Press Enter to close' | Out-Null
  exit 1
}}
& ([scriptblock]::Create($updater)) -BaseUrl {base_url} -Relaunch {relaunch}
"#
    )
}

/// The bash `comtap update` and the screen replace themselves with on Linux. The base URL,
/// relaunch path and message are its positional arguments ($1, $2, $3), never shell code. The
/// script is downloaded first and run with `bash -c`, so a failed download says so instead of
/// piping an empty script, and the update keeps the terminal as its input.
#[cfg(any(not(windows), test))]
const LINUX_LAUNCHER: &str = r#"updater="$(curl --proto '=https' --tlsv1.2 -fsSL --retry 3 --connect-timeout 10 "$1/update.sh")" || { echo "[comtap-update] ERROR: could not download $1/update.sh" >&2; echo "$3" >&2; exit 1; }
exec bash -c "$updater" comtap-update --base-url "$1" --relaunch "$2""#;

/// Said once the update has started and the screen has closed.
#[cfg(windows)]
pub const STARTED_MESSAGE: &str =
    "The updater is running in a new window. Windows will ask for admin permission.";

/// Windows: nothing to do (the updater already runs in its own window).
#[cfg(windows)]
pub fn run_pending() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
static PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Linux: the screen has closed and the terminal is back to normal, so replace this process
/// with the update script. Returns only if no update is pending, or the exec failed.
#[cfg(not(windows))]
pub fn run_pending() -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    if !PENDING.swap(false, std::sync::atomic::Ordering::Relaxed) {
        return Ok(());
    }
    let base_url = base_url()?;
    let relaunch = relaunch_target();
    log!("Starting the updater from {base_url}/update.sh (relaunch: {relaunch})");
    println!("Updating ComTap from {base_url}/update.sh");
    let error = std::process::Command::new("bash")
        .args([
            "-c",
            LINUX_LAUNCHER,
            "comtap-update",
            &base_url,
            &relaunch,
            NOT_STARTED_MESSAGE,
        ])
        .exec();
    let message = format!("Could not start bash for the update: {error}. {NOT_STARTED_MESSAGE}");
    log!("{message}");
    Err(message)
}

/// Linux: marks the update to run once the screen has closed (see `run_pending`).
#[cfg(not(windows))]
pub fn run_update() -> Result<(), String> {
    base_url()?;
    PENDING.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Windows: starts the updater in its own console window. ComTap exits straight after, so the
/// installer can replace comtap.exe.
#[cfg(windows)]
pub fn run_update() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let script = windows_launcher(&base_url()?, &relaunch_target());
    log!("Starting the updater:\n{script}");
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
        .map_err(|error| {
            let message = format!(
                "Could not start PowerShell for the update: {error}. {NOT_STARTED_MESSAGE}"
            );
            log!("{message}");
            message
        })
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
    fn base_url_defaults_to_the_install_site() {
        assert_eq!(parse_base_url(None).unwrap(), "https://brumley.info/comtap");
    }

    #[test]
    fn base_url_override_must_be_https() {
        assert_eq!(
            parse_base_url(Some(
                "https://raw.githubusercontent.com/jackbrumley/comtap/v1.2.3/docs/".to_string()
            ))
            .unwrap(),
            "https://raw.githubusercontent.com/jackbrumley/comtap/v1.2.3/docs"
        );
        assert!(parse_base_url(Some("http://127.0.0.1:8000".to_string())).is_err());
    }

    #[test]
    fn windows_launcher_runs_update_ps1_with_quoted_arguments() {
        let script = windows_launcher(
            "https://brumley.info/comtap",
            r"C:\Program Files\o'brien\comtap.exe",
        );
        assert!(script.contains("Invoke-RestMethod -Uri 'https://brumley.info/comtap/update.ps1'"));
        assert!(script.contains(
            r"-BaseUrl 'https://brumley.info/comtap' -Relaunch 'C:\Program Files\o''brien\comtap.exe'"
        ));
    }

    #[test]
    fn linux_launcher_takes_its_values_as_arguments() {
        assert!(LINUX_LAUNCHER.contains(r#""$1/update.sh""#));
        assert!(LINUX_LAUNCHER.contains(r#"--base-url "$1" --relaunch "$2""#));
        assert!(!LINUX_LAUNCHER.contains("brumley.info"));
    }

    /// Every shipped version relies on this line coming first (Release Standard section 7).
    #[test]
    fn update_scripts_print_the_started_line_first() {
        let first_output = |script: &str, command: &str| {
            script
                .lines()
                .find(|line| line.starts_with(command))
                .map(str::to_string)
        };
        assert_eq!(
            first_output(include_str!("../docs/update.ps1"), "Write-Output "),
            Some("Write-Output \"[comtap-update] started\"".to_string())
        );
        assert_eq!(
            first_output(include_str!("../docs/update.sh"), "echo "),
            Some("echo \"[comtap-update] started\"".to_string())
        );
    }
}
