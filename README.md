# ComTap

Makes a TCP device show up as a COM port. ComTap is for serial-only software that has to read a device that only talks TCP, such as a Datalogic Ethernet barcode scanner.

It's a sister tool to [RawTap](../rawtap). Status: **design only, no working code yet.**

## How it works

- The **com0com** driver creates a linked pair of virtual ports, e.g. `COM5 <-> COM50`.
- Your software opens `COM5`. ComTap opens `COM50` (hidden from other apps) and copies bytes both ways between it and the TCP device.
- ComTap carries the com0com files inside its exe and drives com0com through `setupc.exe`. Nobody opens or configures com0com by hand.

## Planned commands

| Command | What it does |
|---|---|
| `comtap install` (admin, once) | Unpacks and silently installs the com0com driver, creates a port pair for each link in the config, then installs and starts the auto-start service. |
| *(every boot)* | The service starts before anyone logs in, checks the port pairs against the config and repairs them, then connects each link and reconnects with backoff for ever. |
| `comtap status` | Shows each link as connected or retrying, with the time of its last data. |
| `comtap uninstall` | Stops and removes the service and the port pairs. |

The config is the source of truth. If a port is deleted or renumbered, the service puts it back on its next start.

## Config

See `comtap_config.example.json`. Each link has a `name`, a `tcp` address (the device is the TCP server) and the `com` port the software will open.

## Risks to prove on a target PC first

1. **Will the driver load?** The signed com0com 2.2.2.0 driver has to load with Secure Boot on, and with Memory Integrity (HVCI) on if that's enabled, and company driver policy must allow it.
2. **Will the software see the port?** It has to list the com0com port. Some apps only list devices in Device Manager's "Ports" group.

## Licence note

com0com is GPL. ComTap ships its files unchanged, along with its licence and a link to its source, and calls it as a separate program.

## Build

```powershell
cargo build --release          # -> target\release\comtap.exe
```
