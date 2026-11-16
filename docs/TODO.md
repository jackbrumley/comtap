# ComTap TODO

Open work only; remove items when done. The target is the App Standard and
the Release and Update Standard (see `AGENTS.md`).

## Now

- **Hand-test the update:** install v1.0.0, release v1.0.1, and update from
  the screen (F1 → Update to 1.0.1) and with `comtap update`.

## Next

- **Session log to the App Standard (§3):** `debug/session.log` with the
  previous run kept as `last-session.log`, the header (version, exe path, OS),
  RFC 3339 timestamps and `[time] LEVEL: message` lines through the `log`
  crate, and a configurable level.
- **F1 → Debug (§4):** Copy log, Open log, Copy system info, Log level.
- **Split `src/tui.rs`** (over the 600-line soft budget): move the device
  form into `src/tui/form.rs`.

## Ideas / Parking Lot

- **Linux:** a pseudo-terminal per device (no driver needed), a stable
  symlink such as `/dev/comtap0`, a systemd unit instead of the service,
  settings in `/etc/comtap`; `install.sh` and a Linux asset.
- Line-ending fix-up per device, for software that expects a different
  ending from the one the device sends.
