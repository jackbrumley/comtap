//! Screen wording that differs between Windows and Linux.

#[cfg(windows)]
mod os {
    pub const PORT_LABEL: &str = "COM port";
    pub const PORT_HELP: &str =
        "The COM port your software will open. Only free ports are listed. Use ←→ to change it.";
    pub const PICK_PORT: &str = "Pick the COM port your software will open";
    pub const STARTS: &str = "Install. ComTap then runs in the background and starts with Windows.";
    pub const SETUP_NEEDS_ADMIN: &str = "   Setting up needs administrator rights.";
    pub const SETUP_CONTINUE: &str = " to continue. Windows asks for permission (UAC).";
    pub const ADMIN_TITLE: &str = " Needs admin ";
    pub const ADMIN_LINES: [&str; 2] = [
        "This change needs administrator rights.",
        "ComTap will reopen as admin, and Windows will ask for permission.",
    ];
    pub const ADMIN_BUTTON: &str = "reopen as admin";
    pub const VIEW_ONLY: &str = " View only. Changes need admin: press F2.";
    pub const VIEW_ONLY_BADGE: &str = "VIEW ONLY · F2 admin";
    pub const DRIVER_LABEL: &str = "driver com0com";
    pub const UNINSTALL_LINES: [&str; 2] = [
        "Remove the service, its virtual COM ports and the desktop shortcut?",
        "Your settings, logs and the com0com driver stay.",
    ];
    pub const UPDATE_LINES: [&str; 2] = [
        "   Updating opens a window that asks for admin, swaps the program and",
        "   restarts the service. Your devices and settings stay as they are.",
    ];
    pub const UPDATE_STARTING: &str =
        "Starting the ComTap update in a new window. Windows will ask for admin permission.";
}

#[cfg(not(windows))]
mod os {
    pub const PORT_LABEL: &str = "Port";
    pub const PORT_HELP: &str = "The port your software will open, as /dev/<name>. Only free names are listed. Use ←→ to change it.";
    pub const PICK_PORT: &str = "Pick the port your software will open (/dev/ttyComTap0, ...)";
    pub const STARTS: &str = "Install. ComTap then runs in the background and starts at boot.";
    pub const SETUP_NEEDS_ADMIN: &str = "   Setting up needs root.";
    pub const SETUP_CONTINUE: &str =
        " to continue. ComTap reopens with sudo, which asks for your password.";
    pub const ADMIN_TITLE: &str = " Needs root ";
    pub const ADMIN_LINES: [&str; 2] = [
        "This change needs root.",
        "ComTap will reopen with sudo, which asks for your password.",
    ];
    pub const ADMIN_BUTTON: &str = "reopen with sudo";
    pub const VIEW_ONLY: &str = " View only. Changes need root: press F2.";
    pub const VIEW_ONLY_BADGE: &str = "VIEW ONLY · F2 sudo";
    pub const DRIVER_LABEL: &str = "ports";
    pub const UNINSTALL_LINES: [&str; 2] = [
        "Remove the service and its /dev links?",
        "Your settings and logs stay.",
    ];
    pub const UPDATE_LINES: [&str; 2] = [
        "   Updating leaves this screen and runs the installer here (it asks for",
        "   your password), then restarts the service. Your devices and settings stay.",
    ];
    pub const UPDATE_STARTING: &str = "Starting the ComTap update.";
}

pub use os::*;
