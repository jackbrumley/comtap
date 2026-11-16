<p align="center">
  <img src="packaging/icons/comtap-128.png" alt="" width="96" height="96" />
</p>

<h1 align="center">ComTap</h1>

<p align="center">Make a network (TCP) device show up as a COM port on Windows,<br />for software that only talks serial.</p>

<p align="center">
  <a href="https://github.com/jackbrumley/comtap/releases/latest"><img src="https://img.shields.io/github/v/release/jackbrumley/comtap?label=release&color=0284c7" alt="Latest release" /></a>
  <img src="https://img.shields.io/badge/platform-Windows-0284c7" alt="Windows" />
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-AGPL--3.0-0284c7" alt="AGPL-3.0" /></a>
</p>

<p align="center">
  <a href="https://brumley.info/comtap/">Website</a> ·
  <a href="https://github.com/jackbrumley/comtap/releases/latest">Downloads</a> ·
  <a href="docs/GUIDE.md">Guide</a> ·
  <a href="https://github.com/jackbrumley/comtap/issues">Report an issue</a>
</p>

## Install

**Windows** (PowerShell; asks for admin)

```powershell
irm https://brumley.info/comtap/install.ps1 | iex
```

Prefer a download? The [latest release](https://github.com/jackbrumley/comtap/releases/latest) has `comtap-X.Y.Z-windows-x64.exe`: run it and its setup screen does the rest. ComTap checks for a new version each time it opens.

## What it does

ComTap is a Windows service that makes a network (TCP) device show up as a COM port, so software that only talks serial can read it. It was built for a production X-ray inspection line, where an Ethernet barcode scanner had to feed software that only reads a serial port, and it's in daily use there. It sets up its own virtual COM ports, starts with Windows, reconnects by itself, and shows every byte in hex and text.

- Makes any TCP device a COM port: barcode scanners, scales, PLCs, printers
- Runs as a Windows service: starts at boot with no login, and reconnects by itself
- Sets up its own virtual COM ports with the signed com0com driver, with Secure Boot and Memory Integrity on
- Any number of devices, each on its own COM port
- A RawTap-style screen shows each device live, with every byte in hex and text
- Updates itself from GitHub and keeps your settings

## Build from source

```powershell
cargo build --release   # -> target\release\comtap.exe (com0com embedded from vendor\com0com)
cargo test              # config, setupc parsing, bridge, traffic log and screen tests
```

## License

[GNU AGPLv3](LICENSE). Use it, learn from it and build on it freely; if you improve it, share the changes back. The bundled com0com driver is GPL ([vendor/com0com](vendor/com0com)).

---

<p align="center">
  ComTap is one of the free apps by <a href="https://brumley.info">Jack Brumley</a>:<br />
  <a href="https://voquill.org">Voquill</a> · <a href="https://brumley.info/rusty-g6/">Rusty G6</a> · <a href="https://somedrive.org/">SomeDrive</a> · <a href="https://yambuck.com/">Yambuck</a> · <a href="https://brumley.info/rawtap/">RawTap</a> · <b>ComTap</b> · <a href="https://bmuster.com/">BMuster</a>
</p>
