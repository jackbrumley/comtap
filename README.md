<p align="center">
  <img src="packaging/icons/comtap-128.png" alt="" width="96" height="96" />
</p>

<h1 align="center">ComTap</h1>

<p align="center">Make a network (TCP) device show up as a serial (COM) port on Windows and Linux,<br />for software that only talks serial.</p>

<p align="center">
  <a href="https://github.com/jackbrumley/comtap/releases/latest"><img src="https://img.shields.io/github/v/release/jackbrumley/comtap?label=release&color=0284c7" alt="Latest release" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux-0284c7" alt="Windows | Linux" />
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-0284c7" alt="AGPL-3.0" /></a>
</p>

<p align="center">
  <a href="https://brumley.info/comtap/">Website</a> ·
  <a href="https://github.com/jackbrumley/comtap/releases/latest">Downloads</a> ·
  <a href="docs/GUIDE.md">Guide</a> ·
  <a href="https://github.com/jackbrumley/comtap/issues">Report an issue</a>
</p>

## Install

**Linux** (Terminal; asks for your password)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://brumley.info/comtap/install.sh | bash
```

**Windows** (PowerShell; asks for admin)

```powershell
irm https://brumley.info/comtap/install.ps1 | iex
```

Prefer a download? The [latest release](https://github.com/jackbrumley/comtap/releases/latest) has `comtap-X.Y.Z-windows-x64.exe` and `comtap-X.Y.Z-linux-x64`: run it as administrator (or with `sudo`) and its setup screen does the rest. ComTap checks for a new version each time it opens.

## What it does

ComTap is a service for Windows and Linux that makes a network (TCP) device show up as a serial (COM) port, so software that only talks serial can read it. It was built for a production X-ray inspection line, where an Ethernet barcode scanner had to feed software that only reads a serial port, and it's in daily use there. It sets up its own virtual serial ports, starts at boot, reconnects by itself, and shows every byte in hex and text.

- Makes any TCP device a COM port: barcode scanners, scales, PLCs, printers
- Runs as a service (Windows service or systemd): starts at boot with no login, and reconnects by itself
- Windows: sets up its own virtual COM ports with the signed com0com driver, with Secure Boot and Memory Integrity on
- Linux: a pseudo-terminal per device at a fixed name such as `/dev/ttyComTap0`, with no driver to install
- Any number of devices, each on its own COM port
- A RawTap-style screen shows each device live, with every byte in hex and text
- Updates itself from GitHub and keeps your settings

## Build from source

```bash
cargo build --release   # Windows: target\release\comtap.exe (com0com embedded from vendor\com0com)
                        # Linux:   target/release/comtap
cargo test              # config, setupc parsing, pty, bridge, traffic log and screen tests
```

## License

[GNU AGPLv3](LICENSE). Use it, learn from it and build on it freely; if you improve it, share the changes back. The bundled com0com driver (Windows only) is GPL ([vendor/com0com](vendor/com0com)).

---

<p align="center">
  ComTap is one of the free apps by <a href="https://brumley.info">Jack Brumley</a>:<br />
  <a href="https://voquill.org">Voquill</a> · <a href="https://brumley.info/rusty-g6/">Rusty G6</a> · <a href="https://somedrive.org/">SomeDrive</a> · <a href="https://yambuck.com/">Yambuck</a> · <a href="https://brumley.info/rawtap/">RawTap</a> · <b>ComTap</b> · <a href="https://bmuster.com/">BMuster</a>
</p>
