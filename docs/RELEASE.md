# Release Process

ComTap follows the
[Release and Update Standard](https://github.com/jackbrumley/jackbrumley.github.io/blob/main/RELEASE_STANDARD.md),
the same process as RawTap and the other apps. This is ComTap's copy of the
procedure; the standard has the reasoning and the full installer and updater
contracts.

Release identity (tag, title, asset name, download link) is consumed by
`docs/install.ps1`, `docs/install.sh` and the in-app updater (`docs/update.ps1`,
`docs/update.sh`), so a format mistake breaks updates
and one-line installs for every user.

## Format

| Where | Format | Example |
|---|---|---|
| `Cargo.toml` (ComTap's only version file) | `X.Y.Z` | `1.0.0` |
| Version bump commit | `chore: bump version to X.Y.Z` | |
| Git tag and release title | `vX.Y.Z` | `v1.0.0` |
| First line of the release notes | `# ComTap vX.Y.Z` | |
| Windows asset | `comtap-X.Y.Z-windows-x64.exe` | `comtap-1.1.0-windows-x64.exe` |
| Linux asset (static) | `comtap-X.Y.Z-linux-x64` | `comtap-1.1.0-linux-x64` |

A complete release carries these two assets, named by
`scripts/release-assets.mjs`.

## Branches

Development happens on `dev`; releases are tagged on `dev`. Promotion
fast-forwards `main` to the release tag, so `main` always equals the latest
release. GitHub Pages serves `brumley.info/comtap/` (install page and
installer scripts) from `main:/docs`, so promotion also deploys installer
changes. Never commit to `main` directly.

## 1. Prepare and Tag

```bash
npm run release:prepare -- X.Y.Z
git push origin dev vX.Y.Z
```

Pushing the tag starts the **Release** workflow
(`.github/workflows/release.yml`):
1. **Prepare** checks the tag matches the `Cargo.toml` version and creates the
   pre-release with the notes skeleton.
2. **Build** checks the com0com files' SHA-256s, builds `comtap.exe` on
   Windows (`npm run release:windows`) and the static Linux binary on Ubuntu
   (`npm run release:linux`, musl), and uploads both.
3. **Verify** fails unless the release carries exactly the two assets, each
   with a SHA-256 digest.
4. **Install test** installs the pre-release with the installer scripts from
   the tag on a fresh Windows runner, then: sets up a device end to end (the
   com0com driver, a virtual port, the service) with a fake TCP device;
   checks that with no software on the port ComTap stays connected and drops
   the data quietly; reads the data from the COM port; updates in place and
   checks the service and config survive; runs the installed build's real
   in-app update (`scripts/test-in-app-update.mjs`: `comtap update` with the
   tag's own `update.ps1`, which updates to the latest published release);
   uninstalls. The same test runs on a fresh Ubuntu runner with `install.sh`,
   `update.sh`, the systemd service and the pty link `/dev/ttyComTap0`.

Follow it with:
```bash
gh run watch "$(gh run list --workflow release.yml --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status
```

## 2. Write the Release Notes

```bash
npm run --silent release:notes > release-notes.md   # commits since the last tag print to the terminal
gh release edit vX.Y.Z --notes-file release-notes.md
```

Replace every `TODO`. Structure: `# ComTap vX.Y.Z`, one summary paragraph,
themed `###` sections of `- Label: sentence` bullets separated by `---`, then
the generated Downloads block (never edited). Write for users, not developers.

## 3. Promote

```bash
npm run release:promote
```

Refuses unless the asset has its digest and the notes contain no `TODO`. It
marks the release latest and fast-forwards `main` to the tag. From then on
the in-app updater and the one-line installer serve the new version.

## 4. Verify

- An older installed version offers the update (header, F1, `comtap update`)
  and the update installs, restarts the service and opens the new version.
- `irm https://brumley.info/comtap/install.ps1 | iex` and
  `curl --proto '=https' --tlsv1.2 -sSf https://brumley.info/comtap/install.sh | bash`
  install the new version.

## When Something Fails

- **Transient failure** (network, runner): `gh run rerun <run-id> --failed`.
- **Code failure:** nothing was promoted, so nobody received it. Fix on `dev`,
  then:
  ```bash
  gh release delete vX.Y.Z --cleanup-tag --yes
  git tag -d vX.Y.Z
  git tag vX.Y.Z
  git push origin dev vX.Y.Z
  ```
- **Dry run** of the build without a release:
  `gh workflow run release.yml --ref dev`.

## Building Locally (Fallback)

On Windows: `npm run release:windows`; on Linux (with `musl-tools` and the
`x86_64-unknown-linux-musl` target): `npm run release:linux`. Then
`node scripts/package-release.mjs` and
`gh release upload vX.Y.Z release-artifacts/*` on each.

## Agent Checklist

- [ ] Version prepared with `npm run release:prepare`; tag is `vX.Y.Z`.
- [ ] Release workflow green: build, verify and install test.
- [ ] Notes from `npm run release:notes`; no `TODO`; Downloads block untouched.
- [ ] Asset only from the workflow or `package-release.mjs`.
- [ ] `npm run release:promote` only when the maintainer asks.
- [ ] No AI attribution in commits, tags or notes.
