# ComTap Guide

Everything ComTap does, in detail. The [README](../README.md) has the short version and the install command.

ComTap makes a TCP device show up as a serial (COM) port, for serial-only software that has to read a device that only talks TCP, such as a Datalogic Ethernet barcode scanner. It's a sister tool to [RawTap](https://brumley.info/rawtap/): one program that runs as a service (a Windows service, or systemd on Linux) and starts on every boot.

## Requirements

- **Windows:** 64-bit Windows 10 or 11 (Windows Server 2019 or later also works). Works with Secure Boot and Memory Integrity (HVCI) on: the signed com0com 2.2.2.0 driver loads with both.
- **Linux:** any 64-bit (x86_64) distribution with systemd. The binary is static, so it needs no libraries.
- Administrator rights (Windows) or root (`sudo`, Linux) to install, update or change settings. Anyone can open the screen to look.

## How it works

**Windows**

- The **com0com** driver creates a linked pair of virtual ports for each device, e.g. `COM5 <-> CNCB0`.
- Your software opens `COM5`. ComTap opens `CNCB0`, which is hidden from other apps so nobody picks the wrong port, and copies bytes both ways between it and the TCP device.
- ComTap carries the com0com files inside its exe and drives com0com through `setupc.exe`. Nobody opens or configures com0com by hand.

**Linux**

- The service makes a pseudo-terminal (pty) for each device and links it at a fixed name, e.g. `/dev/ttyComTap0 -> /dev/pts/3`. No driver is needed.
- Your software opens `/dev/ttyComTap0`. The port is in raw mode (bytes pass unchanged) and belongs to the serial-port group (`dialout`, or `uucp` on Arch); the installer adds you to it. Log out and back in once for that to take effect.
- The link appears when the service starts and goes when it stops. A pty has no modem lines, so there is no DSR, CTS or DCD.

## The screen

Double-click `comtap.exe`, or the **ComTap** shortcut that install puts on the Public Desktop. On Linux, open **ComTap** from your app menu (the installer adds it): it opens in a terminal window of its own, with ComTap's name and icon on the taskbar, or run `comtap` in a terminal. It looks and works like RawTap.

**First run:** a welcome page explains the three steps and asks for admin once (UAC). Then:

1. **Device form** (like RawTap's setup box): name, device IP, TCP port, and the COM port your software will open. ←→ picks from free ports only. Ports Windows has reserved for other devices, even unplugged ones, are left out. **F5** tests the connection.
2. **Install checklist:** every step is ticked as it runs, so you can see exactly what happened:

```
   √  Check administrator rights
   √  Save settings
   √  Copy ComTap to Program Files
   √  Unpack the com0com driver
   -  Remove other com0com driver versions      none found
   √  Trust the com0com driver publisher
   √  Create the virtual COM ports              COM23
   √  Install the ComTap service (starts at boot)
   √  Start the service
   √  Create the desktop shortcut
   √  Check each device connects                1 connected

   Done.  Your software can now open COM23.
```

If a step fails, it shows × with the reason, and Enter goes back to the form to fix it.

**After that:** ComTap opens on your device list. Everything works with four keys: **↑↓** choose, **Enter** open, **Esc** back, **F1** menu.

```
 ComTap   ● SERVICE RUNNING   driver com0com Running                       v1.3.0  ADMIN
┌ Devices ───────────────────────────────────────────────────────────────────────────────┐
│   + Add a device                                                                       │
│ ► Dock 3 scanner          COM23       ● Connected                                      │
│   Scale                   COM24       ● Can't reach the device                         │
│   Spare scanner           COM25       ○ Off                                            │
└────────────────────────────────────────────────────────────────────────────────────────┘
 ↑↓ choose  Enter open  F1 menu
```

**Enter on a device** opens its page: what is happening in plain words, and what you can do.

```
┌ Dock 3 scanner ────────────────────────────────────────────────────────────────────────┐
│  Connected. Your software reads the device on COM23.                                   │
│                                                                                        │
│  Device      10.1.128.186, TCP port 51236                                              │
│  COM port    COM23                                                                     │
│  Last data   12:01:29                                                                  │
│                                                                                        │
│ ►  Watch data                                                                          │
│    Edit settings                                                                       │
│    Delete this device                                                                  │
│    Back                                                                                │
└────────────────────────────────────────────────────────────────────────────────────────┘
 ↑↓ choose  Enter open  Esc back  F1 menu
```

- **Watch data** shows the device's traffic, every byte in hex and text. **←→** changes the format (hex+text, hex, text, compact), **PgUp/PgDn** scrolls.

```
┌ Watch data · Dock 3 scanner · hex+text ────────────────────────────────────────────────┐
│12:01:20.000 √ connected to 10.1.128.186:51236                                          │
│12:01:29.120 RX HEX  30 39 38 37 36 35 34 33 32 31 0D 0A                                │
│12:01:29.120 RX TEXT 0987654321<CR><LF>                                                 │
└────────────────────────────────────────────────────────────────────────────────────────┘
 ←→ format  PgUp/PgDn scroll  Esc back  F1 menu
```

- **Edit settings** is the device form, with one more field, **Status** (◄ On ►). Off keeps the device in the list with its settings and its COM port, so your software keeps its setting, but ComTap doesn't connect to it. That frees one of the device's connections (a barcode scanner may allow only a few). Enter saves and applies everything, with the same kind of checklist.
- **Delete this device** asks first, then removes the device and its virtual port.
- **Add a device** is the first line of the list.
- **F1** opens the menu (type to filter): restart, stop or start the service, repair virtual ports, reinstall, uninstall, quit, update, check for updates, open the log folder. **Ctrl+C** also quits.
- **Anyone can view.** Changes need admin: ComTap offers to reopen itself as admin, straight back at what you chose (F1 → Reopen as admin does it any time). The top line shows the version you run.
- **Any number of devices:** each gets its own COM port. One service runs them all.

## Updates

ComTap checks GitHub for a newer version each time the screen opens. When one is out, the header says so and a dialog offers it once, as soon as nothing else is open on the screen. **F1 → Update to X.Y.Z** (or `comtap update`) downloads ComTap's update script from brumley.info and runs it (on Windows in its own window; on Linux in the same terminal). The script runs the same installer as a new install:

1. asks for admin (UAC) or your password (`sudo`),
2. downloads the new program and checks it against GitHub's SHA-256 digest,
3. stops the service and swaps the program,
4. runs `comtap install`, which keeps your devices and settings and starts the service again.

If anything fails, the version you had starts again and says so. If the update script can't be downloaded, nothing changes, and ComTap points you to the release page to download the new version by hand. The update log is `C:\ProgramData\ComTap\logs\update.log` (Windows) or `/var/log/comtap/update.log` (Linux).

## Command line

```powershell
# As administrator (Windows) or with sudo (Linux):
comtap install       # set up from comtap_config.json next to the program (or the installed config)
comtap restart       # after editing the config by hand
comtap ports         # create or repair the virtual ports
comtap update        # update to the latest release
comtap uninstall     # remove the service and ports (Windows: also the shortcut; --keep-ports, --remove-driver)

# Anyone:
comtap               # the screen
comtap status        # each device: connected / retrying, last data, byte counts
comtap run           # run the bridges in this window instead of as a service (testing)
comtap --version
```

`install` prints the same checklist as the screen, and is safe to run again.

## Config

`C:\ProgramData\ComTap\comtap_config.json` (Windows) or `/etc/comtap/comtap_config.json` (Linux), written by the screen:

```json
{
  "links": [
    { "name": "Dock 3 scanner", "tcp": "10.1.128.186:51236", "com": "COM23" }
  ]
}
```

- `tcp` is the device's address. The device is the TCP server.
- `com` is the port your software opens. Windows: `COM1` to `COM255`; each device gets its own pair. Linux: a name such as `ttyComTap0`, opened as `/dev/ttyComTap0`.
- `"enabled": false` turns a device off (Status in Edit settings writes it). Left out, the device is on.

## Reliability

| Situation | What ComTap does |
|---|---|
| PC boots | The service is set to auto-start, with no login needed. If the driver isn't ready yet, it retries every 30 s. |
| Port pair deleted or renamed | It re-creates the pair from the config on the next service start (`comtap restart` or a reboot). |
| Device off, unplugged or rebooting | It reconnects with backoff from 0.5 s up to 30 s, for ever. The COM port stays open the whole time, so the software never sees it vanish. |
| Device dies silently | TCP keepalive notices within about 30 s and reconnects. |
| Software sends data while the device is offline | The data is read and dropped, so the software never blocks. |
| No software has the COM port open | ComTap stays connected to the device and drops its data. The screen shows "waiting for COMx" and the traffic log notes when software opens or closes the port. |
| Software has the port open but isn't reading | Device data is dropped, as a real serial line would, and logged. The connection stays up. |
| Service crashes | Windows (or systemd) restarts it after 5 s. |

On Windows the virtual port raises DSR, CTS and DCD, so the software sees a "device attached" the way it would on a real cable.

## Files

On Windows, everything ComTap writes is in `C:\ProgramData\ComTap` (the service runs as SYSTEM, so not in a user profile):

| File | What |
|---|---|
| `comtap_config.json` | Your devices |
| `status.json` | Live state of each device, written by the service every 2 s |
| `logs\comtap-YYYY-MM-DD.log` | What ComTap did, kept for 14 days |
| `logs\update.log` | What the installer and updater did |
| `traffic\traffic-COM5-YYYY-MM-DD.log` | Every byte each way, per device |

On Linux the same files are in `/etc/comtap` (config), `/var/lib/comtap` (`status.json`, `traffic/`) and `/var/log/comtap` (logs); the program is `/usr/local/bin/comtap` and the service is `/etc/systemd/system/comtap.service` (`journalctl -u comtap` shows its output).

The traffic log groups bytes like RawTap's raw log: a new entry on a newline, a 30 ms gap, a change of direction, or every 1 KB. It's readable in Notepad, kept for 14 days and capped at 50 MB per device. When the device asks for a password (`Password:`, `Passcode:`, `Passphrase:`, `Secret:`, `PIN:`), what your software sends next, up to its Enter, is logged as `<hidden>`, never the characters or how many there were. A prompt worded differently isn't recognised.

```
2026-11-16 12:01:29.120 RX 30 39 38 37 36 35 34 33 32 31 0D 0A | 0987654321<CR><LF>
2026-11-16 12:05:00.000 -- disconnected: device closed the connection
```

## Uninstall

```powershell
irm https://brumley.info/comtap/uninstall.ps1 | iex
```

Removes the service, its virtual COM ports, the desktop shortcut and `C:\Program Files\ComTap`. Your settings and logs stay unless you add `-PurgeData`; the com0com driver stays unless you add `-RemoveDriver` (run the script from a file to pass them).

On Linux:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://brumley.info/comtap/uninstall.sh | bash
```

Removes the service (its `/dev` links go with it) and `/usr/local/bin/comtap`. Your settings and logs stay unless you add `--purge-data` (`... | bash -s -- --purge-data`).

## com0com

On Windows, com0com (GPL) is by Vyacheslav Frolov; the signed x64 2.2.2.0 build is by Steven William Hatchett. ComTap ships its files unchanged in `vendor/com0com`, with its `ReadMe.txt` licence notice, and calls it as a separate program. Source: https://sourceforge.net/projects/com0com/
