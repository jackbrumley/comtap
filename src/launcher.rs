//! Linux: ComTap in the desktop's app menu. The installer (`install.sh`)
//! runs `comtap launcher` as root, which writes the menu entry and the
//! icon; `uninstall.sh` removes them. The entry starts `comtap
//! --own-window` (`linux_window`), so ComTap gets a terminal window of its
//! own, with its name and icon on the taskbar.

use crate::APP_ID;
use anyhow::{Context, Result};
use std::path::Path;

/// The menu entry. Its file name is the app ID (Release Standard).
pub const DESKTOP_FILE: &str = "/usr/share/applications/info.brumley.comtap.desktop";
/// The icon the entry names (`Icon=comtap`).
pub const ICON_FILE: &str = "/usr/share/icons/hicolor/256x256/apps/comtap.png";
const ICON: &[u8] = include_bytes!("../packaging/icons/comtap-256.png");

/// The menu entry's text.
pub fn desktop_entry(exe: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=ComTap\n\
         Comment=Bridge TCP devices to virtual serial ports\n\
         Exec={} --own-window\n\
         Icon=comtap\n\
         Terminal=false\n\
         StartupWMClass={APP_ID}\n\
         Categories=Development;Utility;\n\
         Keywords=serial;tcp;com;port;bridge;\n\
         StartupNotify=false\n",
        exe.display()
    )
}

/// `comtap launcher`: writes the menu entry (starting the installed
/// program) and the icon. Needs root.
pub fn install() -> Result<()> {
    let exe = crate::paths::installed_exe();
    write(Path::new(ICON_FILE), ICON)?;
    write(Path::new(DESKTOP_FILE), desktop_entry(&exe).as_bytes())?;
    println!("Added ComTap to the app menu ({DESKTOP_FILE}).");
    Ok(())
}

fn write(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_opens_comtaps_own_window_under_its_app_id() {
        let entry = desktop_entry(Path::new("/usr/local/bin/comtap"));
        assert!(entry.contains("\nExec=/usr/local/bin/comtap --own-window\n"));
        assert!(entry.contains("\nTerminal=false\n"));
        assert!(entry.contains(&format!("\nStartupWMClass={APP_ID}\n")));
        assert!(entry.contains("\nIcon=comtap\n"));
        assert!(DESKTOP_FILE.ends_with(&format!("/{APP_ID}.desktop")));
        assert!(ICON.starts_with(b"\x89PNG"));
    }
}
