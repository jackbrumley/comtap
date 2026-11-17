# ComTap TODO

Open work only; remove items when done. The target is the App Standard and
the Release and Update Standard (see `AGENTS.md`).

## Now

- **Hand-test on a real Linux PC:** install with `install.sh`, set up a
  device from the screen (sudo relaunch), open `/dev/ttyComTap0` from real
  serial software, and update from the screen (F1 → Update) once the next
  release is out.

## Next

- **Session log to the App Standard (§3):** `debug/session.log` with the
  previous run kept as `last-session.log`, the header (version, exe path, OS),
  RFC 3339 timestamps and `[time] LEVEL: message` lines through the `log`
  crate, and a configurable level.
- **F1 → Debug (§4):** Copy log, Open log, Copy system info, Log level.

## Ideas / Parking Lot

- Line-ending fix-up per device, for software that expects a different
  ending from the one the device sends.
