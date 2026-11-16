# ComTap Guide

Everything ComTap does, in detail. The [README](../README.md) has the short version and the install command.

ComTap makes a TCP device show up as a COM port, for serial-only software that has to read a device that only talks TCP, such as a Datalogic Ethernet barcode scanner. It's a sister tool to [RawTap](https://brumley.info/rawtap/): one `comtap.exe` that runs as a Windows service and starts on every boot.

## Requirements

- 64-bit Windows 10 or 11 (Windows Server 2019 or later also works).
- Administrator rights to install, update or change settings. Anyone can open the screen to look.
- Works with Secure Boot and Memory Integrity (HVCI) on: the signed com0com 2.2.2.0 driver loads with both.

## How it works

- The **com0com** driver creates a linked pair of virtual ports for each device, e.g. `COM5 <-> CNCB0`.
- Your software opens `COM5`. ComTap opens `CNCB0`, which is hidden from other apps so nobody picks the wrong port, and copies bytes both ways between it and the TCP device.
- ComTap carries the com0com files inside its exe and drives com0com through `setupc.exe`. Nobody opens or configures com0com by hand.

## The screen

Double-click `comtap.exe`, or the **ComTap** shortcut that install puts on the Public Desktop. It looks and works like RawTap.

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

**After that:** the main screen.

```
 ComTap   ● SERVICE RUNNING   driver com0com Running                      VIEW ONLY · F2 admin
┌ Devices ───────────────────────────────────────────────────────────────────────────────────┐
│ ► ● Dock 3 scanner      COM23  ↔ 10.1.128.186:51236     connected     last data 12:01:29   │
│   ● Scale               COM24  ↔ 10.1.128.190:4001      retrying                           │
└────────────────────────────────────────────────────────────────────────────────────────────┘
┌ Traffic · Dock 3 scanner · hex+text (F8) ──────────────────────────────────────────────────┐
│12:01:20.000 √ connected to 10.1.128.186:51236                                              │
│12:01:29.120 RX HEX  30 39 38 37 36 35 34 33 32 31 0D 0A                                    │
│12:01:29.120 RX TEXT 0987654321<CR><LF>                                                     │
│12:01:40.002 TX HEX  54 0D                                                                  │
│12:01:40.002 TX TEXT T<CR>                                                                  │
└────────────────────────────────────────────────────────────────────────────────────────────┘
 F1 menu  ↑↓ device  Enter edit  A add  D delete  F8 format  PgUp/PgDn scroll  L logs  Q quit
```

- **Anyone can view.** Changes need admin: ComTap offers to reopen itself as admin, straight back at what you chose.
- **F1** opens the menu (type to filter): add, edit or delete a device, restart, stop or start the service, repair virtual ports, reinstall, uninstall, check for updates, open the log folder.
- **Any number of devices:** each gets its own COM port. One service runs them all.
- Saving a device applies it straight away, with the same kind of checklist.

## Updates

ComTap checks GitHub for a newer version each time the screen opens. When one is out, the header says so and a dialog offers it once. **F1 → Update to X.Y.Z** (or `comtap update`) runs the same installer as a new install, in its own window:

1. asks for admin (UAC),
2. downloads the new `comtap.exe` and checks it against GitHub's SHA-256 digest,
3. stops the service and swaps the program,
4. runs `comtap install`, which keeps your devices and settings and starts the service again.

If anything fails, the version you had starts again and says so, with the update log's path: `C:\ProgramData\ComTap\logs\update.log`.

## Command line

```powershell
# As administrator:
comtap install       # set up from comtap_config.json next to comtap.exe (or the installed config)
comtap restart       # after editing C:\ProgramData\ComTap\comtap_config.json
comtap ports         # create or repair the virtual ports
comtap update        # update to the latest release
comtap uninstall     # remove the service, ports and shortcut (--keep-ports, --remove-driver)

# Anyone:
comtap               # the screen
comtap status        # each device: connected / retrying, last data, byte counts
comtap run           # run the bridges in this window instead of as a service (testing)
comtap --version
```

`install` prints the same checklist as the screen, and is safe to run again.

## Config

`C:\ProgramData\ComTap\comtap_config.json`, written by the screen:

```json
{
  "links": [
    { "name": "Dock 3 scanner", "tcp": "10.1.128.186:51236", "com": "COM23" }
  ]
}
```

- `tcp` is the device's address. The device is the TCP server.
- `com` is the port your software opens. Each device gets its own pair.

## Reliability

| Situation | What ComTap does |
|---|---|
| PC boots | The service is set to auto-start, with no login needed. If the driver isn't ready yet, it retries every 30 s. |
| Port pair deleted or renamed | It re-creates the pair from the config on the next service start (`comtap restart` or a reboot). |
| Device off, unplugged or rebooting | It reconnects with backoff from 0.5 s up to 30 s, for ever. The COM port stays open the whole time, so the software never sees it vanish. |
| Device dies silently | TCP keepalive notices within about 30 s and reconnects. |
| Software sends data while the device is offline | The data is read and dropped, so the software never blocks. |
| Software not reading | Device data is dropped, as a real serial line would, and logged. |
| Service crashes | Windows restarts it after 5 s. |

The virtual port raises DSR, CTS and DCD, so the software sees a "device attached" the way it would on a real cable.

## Files

Everything ComTap writes is in `C:\ProgramData\ComTap` (the service runs as SYSTEM, so not in a user profile):

| File | What |
|---|---|
| `comtap_config.json` | Your devices |
| `status.json` | Live state of each device, written by the service every 2 s |
| `logs\comtap-YYYY-MM-DD.log` | What ComTap did, kept for 14 days |
| `logs\update.log` | What the installer and updater did |
| `traffic\traffic-COM5-YYYY-MM-DD.log` | Every byte each way, per device |

The traffic log groups bytes like RawTap's raw log: a new entry on a newline, a 30 ms gap, a change of direction, or every 1 KB. It's readable in Notepad, kept for 14 days and capped at 50 MB per device.

```
2026-11-16 12:01:29.120 RX 30 39 38 37 36 35 34 33 32 31 0D 0A | 0987654321<CR><LF>
2026-11-16 12:05:00.000 -- disconnected: device closed the connection
```

## Uninstall

```powershell
irm https://brumley.info/comtap/uninstall.ps1 | iex
```

Removes the service, its virtual COM ports, the desktop shortcut and `C:\Program Files\ComTap`. Your settings and logs stay unless you add `-PurgeData`; the com0com driver stays unless you add `-RemoveDriver` (run the script from a file to pass them).

## com0com

com0com (GPL) is by Vyacheslav Frolov; the signed x64 2.2.2.0 build is by Steven William Hatchett. ComTap ships its files unchanged in `vendor/com0com`, with its `ReadMe.txt` licence notice, and calls it as a separate program. Source: https://sourceforge.net/projects/com0com/
