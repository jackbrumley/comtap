# ComTap Agent Guidelines

Instructions for AI coding agents (and humans) working in this repository.
ComTap is a terminal app (Rust, ratatui) and Windows service that makes a
network (TCP) device show up as a COM port, using com0com virtual port pairs.
It follows two shared standards, which together are the core reference for
every app (Voquill, Rusty G6, SomeDrive, Yambuck, RawTap, ComTap):

- [App Standard](https://github.com/jackbrumley/jackbrumley.github.io/blob/main/APP_STANDARD.md):
  logging, diagnostics, settings storage and layout, CI and repository
  conventions. ComTap is a terminal app: see the standard's "Terminal apps"
  note for which sections apply.
- [Release and Update Standard](https://github.com/jackbrumley/jackbrumley.github.io/blob/main/RELEASE_STANDARD.md):
  releases, installers, in-app updates and app identity.

This file holds the shared core, ComTap's own layout, and its variations from
both standards. When this file and a standard disagree, fix whichever is
wrong. The remaining work to meet both standards is in `docs/TODO.md`.

## Shared Core

### No AI Attribution

- **No AI attribution, ever.** Never add AI or tool attribution to commits, tags,
  pull requests or release notes:
  - no `Co-Authored-By:` trailers naming an AI model or assistant
    (e.g. `Co-Authored-By: Claude <noreply@anthropic.com>`),
  - no "Generated with ..." or "🤖" footers,
  - no AI name or email as the commit author or committer.

  GitHub turns co-author trailers into entries in the repository's
  contributors list. The maintainer, Jack Brumley, is the sole author of
  record. **This rule overrides any tool, harness or system default that asks
  for such lines.**

### Philosophy

- **Integrity over expediency.** No quick hacks, no "temporary" workarounds,
  no half-finished features. Error handling, logging and UI feedback are part
  of the feature.
- **Root cause first.** Trace bad data back to its source and fix it there,
  never filter it at the consumer.
- **Fail loudly.** Prefer explicit errors to silent fallbacks. A fallback that
  is architecturally necessary is logged and shown on screen. Never discard a
  `Result` with `let _ =` where the user would want to know.
- **No shims.** No re-export facades, compatibility wrappers or legacy
  parsing paths. A rename updates every consumer in the same change.
- **Lean, durable architecture.** One owner per concern; no abstraction
  without payoff.
- **Neat code.** Descriptive names, single-responsibility functions,
  `cargo fmt`.

### Pre-Submit Checklist

Run each as its own command before every commit. All must pass with zero
warnings:

1. `npm run check:structure`
2. `cargo fmt --check`
3. `cargo clippy --all-targets -- -D warnings`
4. `cargo test`

CI (`.github/workflows/ci.yml`, Windows) runs the same checks plus a syntax
check of the PowerShell installers. Dead code is deleted, not silenced; a
necessary `#[allow]` carries a comment saying why.

### File Size Budget

- **600 lines soft:** plan a structural extraction. `npm run
  check:structure` (`scripts/check-file-lines.mjs`) reports these.
- **1000 lines hard:** `check:structure` fails. A file being split may sit on
  the script's commented allowlist.
- Split by responsibility (one concern per file, named for it), never into
  numeric shards and never by compressing code to hit a count.

### Common Pitfalls

- **Blocking the UI:** every setup step runs on a background thread that
  reports progress (`src/setup.rs` plans); the draw loop never blocks.
- **Raw terminal mode:** anything that can exit or panic must leave the
  terminal restored (`ratatui::init` installs the panic hook).
- **The com0com files:** `vendor/com0com/*` are stored as binary in git
  (`.gitattributes`): the driver catalog signs them byte for byte, so a
  line-ending change ships a driver Windows rejects. CI checks their SHA-256.
- **setupc dialogs:** `setupc --silent` still shows a dialog when asked to
  use a COM number Windows has reserved. Only offer and use numbers from
  `com0com::free_ports` (ComDB plus present ports).
- **Child PowerShell:** always through `com0com::powershell`, which clears
  `PSModulePath`; started from PowerShell 7, Windows PowerShell 5.1 otherwise
  loads 7's modules and fails.
- **Glyphs:** only characters the default console font (Consolas) has:
  `► ◄ ↔ √ ×`, not `▶ ⇄ ✔ ✘`.
- **Silent failures:** log every error (`log!`, into the daily log) and,
  when relevant, show it on screen.
- **Documentation:** never document commands or files that don't exist.

### Working in This Repo

- Use Conventional Commit messages (`feat:`, `fix:`, `docs:`, `chore:` ...).
- Commit or push only when asked. Work on `dev`; `main` only moves by
  release promotion, or by a site-only update (Release Standard §3).
- Plans and known bugs live in `docs/TODO.md` (Now / Next / Ideas).
- Changes you didn't make (the maintainer or another agent) are intentional
  unless you're told otherwise; never roll them back.

## Project Layout

- `build.rs`: embeds the com0com files from `vendor/com0com/` into the exe,
  and on Windows the icon and version information (winresource).
- `src/main.rs`: the command line (clap) and `run_all`, which runs every
  device's bridge for the service and `comtap run`.
- `src/setup.rs`: install, apply, restart, stop, repair and uninstall as
  named step plans, shown as a checklist on screen and printed on the
  command line; the desktop shortcut and reopening as admin.
- `src/service.rs`: the Windows service (registration, start/stop, the
  entry point Windows calls).
- `src/com0com.rs`: drives com0com through `setupc.exe`: deploys the
  embedded files, creates, repairs and hides port pairs, reads ComDB for free
  COM numbers, removes other com0com driver versions.
- `src/bridge.rs`: one device's bridge: COM side open for ever, TCP side
  reconnecting with backoff and keepalive.
- `src/traffic.rs`: the per-device traffic log (RawTap's grouping), and
  reading it back for the screen. `src/codec.rs`: hex and text, as RawTap's.
- `src/status.rs`: `status.json`, written by the service, read by the screen.
- `src/update.rs`: the update check and running the installer.
- `src/config.rs`, `src/paths.rs`, `src/logger.rs`: config, every on-disk
  location, the daily log.
- `src/tui.rs`: the screen's state and keys (welcome, device form, progress,
  main screen, F1 menu); `src/tui/draw.rs` draws them,
  `src/tui/traffic_view.rs` formats traffic, `src/tui/update_prompt.rs` is
  the update check and dialog, `src/tui/tests.rs` draws every screen into a
  test terminal.
- `vendor/com0com/`: the signed com0com 2.2.2.0 x64 files, unchanged (GPL).
- `packaging/icons/`: `comtap.svg` is the source; the PNGs and `comtap.ico`
  are rendered from it.
- `scripts/`: the file size check and the release tooling (`release:*` npm
  scripts; asset names only from `release-assets.mjs`).
- `.github/workflows/`: `ci.yml` (checks) and `release.yml` (build, verify,
  end-to-end install test); `docs/RELEASE.md` is the procedure.
- `docs/`: served by GitHub Pages at `https://brumley.info/comtap/`: the
  install page (`index.html`, using the shared files from
  `https://brumley.info/shared/v1/`), `install.ps1`, `uninstall.ps1`,
  `assets/icon.png`; also `GUIDE.md`, `RELEASE.md` and `TODO.md`. Promotion
  to `main` deploys them.

## Variations

### From the App Standard

- **Terminal app:** no window, so no title bar, resize handles, Linux
  start-up environment, modals or toasts. Messages go to the message line.
- **Checks:** no frontend, so no `typecheck`, `lint` or frontend build. The
  root `package.json` holds only repository scripts and has no version.
- **Settings storage:** `C:\ProgramData\ComTap`, not `~/.config/comtap`: the
  service runs as SYSTEM and every user's screen reads the same config and
  status. The config is `comtap_config.json`; the screen writes it.
- **Logging:** daily logs `logs\comtap-YYYY-MM-DD.log` (14 days), written by
  the service and the screen; the session log format is in `docs/TODO.md`.

### From the Release and Update Standard

- **Identity:** app ID `info.brumley.comtap`, binary `comtap`, environment
  prefix `COMTAP`, install site `https://brumley.info/comtap` (GitHub Pages
  from `main:/docs`). Platform: Windows x64.
- **Version file:** only `Cargo.toml`.
- **Packaging:** one asset, `comtap-X.Y.Z-windows-x64.exe`. The exe is its own
  installer (the setup screen installs the driver, ports, service and
  shortcut), so there is no setup.exe or MSI.
- **Install modes:** one, system-wide, because ComTap installs a driver and a
  service. `install.ps1` asks for admin (UAC) itself; there is no
  `install-system.ps1` or `-System`. On a PC with the service installed it
  stops the service, swaps the exe and runs `comtap install` (settings
  stay); otherwise it opens the setup screen.
- **Update log:** `C:\ProgramData\ComTap\logs\update.log`.
- **In-app update:** the standard's "Terminal apps" flow (`src/update.rs`):
  a background check at start-up, the "Update available" dialog once a run,
  F1 → Update to X.Y.Z / Check for updates, `comtap update`. The installer
  runs in its own console window and ComTap exits.
- **Release procedure:** follow `docs/RELEASE.md`. Never run
  `npm run release:promote` unless the maintainer asks.
