# ComTap

Makes a TCP device show up as a COM port. ComTap is for serial-only software that has to read a device that only talks TCP, such as a Datalogic Ethernet barcode scanner.

It's a sister tool to [RawTap](https://github.com/jackbrumley/rawtap). It's one ~1.8 MB `comtap.exe` that runs as a Windows service and starts on every boot.

**Status:** the bridge, service and com0com control are built and unit-tested. They have **not yet run against the real com0com driver**. See [Risks](#risks-to-prove-on-a-target-pc).

## How it works

- The **com0com** driver creates a linked pair of virtual ports for each link, e.g. `COM5 <-> CNCB0`.
- Your software opens `COM5`. ComTap opens `CNCB0`, which is hidden from other apps so nobody picks the wrong port, and copies bytes both ways between it and the TCP device.
- ComTap carries the com0com files inside its exe and drives com0com through `setupc.exe`. Nobody opens or configures com0com by hand.

## The screen

Double-click `comtap.exe`, or the **ComTap** shortcut that install puts on the Public Desktop.

```
 ComTap 0.1.0  TCP devices as COM ports                              VIEW ONLY · F2 admin
┌ Status ───────────────────────────────────────────────────────────────────────────────┐
│Service  ● Running  · starts on boot                                                   │
│Driver   ● com0com Running  · files inside exe                                         │
└───────────────────────────────────────────────────────────────────────────────────────┘
┌ Scanners ─────────────────────────────────────────────────────────────────────────────┐
│▶ ● Line 1 scanner     COM5   ⇄ 192.168.1.50:51236     connected  last data 12:01:40   │
│  ● Line 2 scanner     COM6   ⇄ 192.168.1.51:51236     retrying                        │
└───────────────────────────────────────────────────────────────────────────────────────┘
┌ Line 1 scanner · traffic · hex+text (F8) · following ─────────────────────────────────┐
│12:01:20 -- connected to 192.168.1.50:51236                                            │
│12:01:29 RX 30 39 38 37 36 35 34 33 32 31 0D 0A              0987654321<CR><LF>        │
│12:01:40 TX 54 0D                                            T<CR>                     │
└───────────────────────────────────────────────────────────────────────────────────────┘
 ↑↓ select · Enter edit · A add · D delete · S service · F8 format · PgUp/PgDn scroll · L folder · Q quit
```

- **Anyone can view** the service, the driver, each scanner and its live traffic. **Changes need admin.** Press F2, or any change key, and ComTap offers to reopen itself as admin (UAC prompt).
- **Enter / A** edits or adds a scanner: name, IP, port, COM port (←→ picks a free one), and **F5 tests the connection**. Saving applies straight away: it fixes the ports and restarts the service.
- **S** opens the service menu: Install; or Restart, Stop/Start, Repair virtual ports, Reinstall, Uninstall.
- **Several scanners:** add one link per scanner. One service runs them all, each with its own COM port.

## Command line

```powershell
# 1. Put comtap_config.json next to comtap.exe (see below), then, as administrator:
comtap install       # installs com0com, creates the ports, installs + starts the service, desktop shortcut

comtap status        # each link: connected / retrying, last data, byte counts
comtap restart       # after editing C:\ProgramData\ComTap\comtap_config.json
comtap ports         # create/repair the virtual ports only
comtap run           # run in this window instead of as a service (testing)
comtap uninstall     # remove service and ports  (--keep-ports, --remove-driver)
```

`install` is safe to run again, e.g. to upgrade the exe or apply a new config. If there's no config anywhere, it writes a starter one and tells you where.

## Config

`C:\ProgramData\ComTap\comtap_config.json` after install:

```json
{
  "links": [
    { "name": "Line 1 scanner", "tcp": "192.168.1.50:51236", "com": "COM5" }
  ]
}
```

- `tcp` is the device's address. The device is the TCP server.
- `com` is the port your software opens. Each link gets its own pair.

## Reliability

| Situation | What ComTap does |
|---|---|
| PC boots | The service is set to auto-start, with no login needed. If the driver isn't ready yet, it retries every 30 s. |
| Port pair deleted or renamed | It re-creates the pair from the config on the next service start (`comtap restart` or a reboot). |
| Scanner off, unplugged or rebooting | It reconnects with backoff from 0.5 s up to 30 s, for ever. The COM port stays open the whole time, so the software never sees it vanish. |
| Scanner dies silently | TCP keepalive notices within about 30 s and reconnects. |
| Software sends data while the scanner is offline | The data is read and dropped, so the software never blocks. |
| Software not reading | Scanner data is dropped, as a real serial line would, and logged. |
| Service crashes | Windows restarts it after 5 s. |

The virtual port raises DSR, CTS and DCD, so the software sees a "device attached" the way it would on a real cable.

Logs go to `C:\ProgramData\ComTap\logs\comtap-YYYY-MM-DD.log` and are kept for 14 days. Live status is in `status.json`.

**Traffic log:** every byte each way is kept in `C:\ProgramData\ComTap\traffic\traffic-COM5-YYYY-MM-DD.log`. Bytes are grouped like RawTap's raw log: a new entry on a newline, a 30 ms gap, a change of direction, or every 1 KB. It's readable in Notepad. Files are kept for 14 days, capped at 50 MB per link.

```
2026-11-16 12:01:29.120 RX 30 39 38 37 36 35 34 33 32 31 0D 0A | 0987654321<CR><LF>
2026-11-16 12:05:00.000 -- disconnected: device closed the connection
```

## Build

```powershell
# Put the com0com files in vendor\com0com\ first (see vendor\com0com\README.md) to embed them.
cargo build --release          # -> target\release\comtap.exe
cargo test                     # config, setupc parsing, bridge (round trip, reconnect, late device), traffic log, screen render
```

If you build without the com0com files, `install` looks for a `com0com\` folder next to `comtap.exe` instead.

## Risks to prove on a target PC

1. **Will the driver load?** The signed com0com 2.2.2.0 driver has to load with Secure Boot on, and with Memory Integrity (HVCI) on if that's enabled, and company driver policy must allow it.
2. **Will the software see the port?** It has to list the com0com port. Some apps only list devices in Device Manager's "Ports" group.
3. **Is `setupc` happy?** It must accept `--silent` and `HiddenMode=yes`, and its `list` output must match what ComTap parses (`src/com0com.rs`).

## Licence note

com0com is GPL. ComTap ships its files unchanged, along with its licence and a link to its source, and calls it as a separate program.
