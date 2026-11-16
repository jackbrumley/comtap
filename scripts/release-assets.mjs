// Single source of truth for release identity: the version, the git tag, the
// release asset file names, and their download URLs. package-release.mjs names
// the build output from here and release-notes-template.mjs writes the
// Downloads block from here, so the two can never disagree.
//
// docs/install.ps1 derives the download URL from the tag and this exact file
// name, so a wrong name breaks updates and installs. See RELEASE_STANDARD.md
// (jackbrumley/jackbrumley.github.io) and docs/RELEASE.md.

import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

// ComTap's one version file (a variation: no package.json or tauri.conf.json
// version). The first `version = "..."` line is the [package] version.
function readCargoVersion() {
  const cargoToml = readFileSync(resolve(ROOT, "Cargo.toml"), "utf8");
  const version = cargoToml.match(/^version = "([^"]+)"$/m)?.[1];
  if (!version) {
    console.error("Could not find the package version in Cargo.toml.");
    process.exit(1);
  }
  return version;
}

export const REPOSITORY = "jackbrumley/comtap";

export const VERSION = readCargoVersion();
export const TAG = `v${VERSION}`;

// A variation: one plain executable per platform, each its own installer.
// comtap.exe installs the com0com driver, the virtual ports and the Windows
// service, so there is no setup.exe or MSI. The Linux build is a static (musl)
// binary that installs its systemd service, so there is no .deb, .rpm or
// AppImage; docs/install.sh downloads it.
export const ASSET_NAMES = {
  windowsExe: `comtap-${VERSION}-windows-x64.exe`,
  linuxBin: `comtap-${VERSION}-linux-x64`,
};

// Every asset a complete release must carry. verify-release.mjs checks the
// published release against this list before it may be promoted.
export const EXPECTED_ASSETS = Object.values(ASSET_NAMES);

// Order and labels of the Downloads block in every release's notes.
export const DOWNLOAD_LINKS = [
  { platform: "Windows", label: "comtap.exe (64-bit)", fileName: ASSET_NAMES.windowsExe },
  { platform: "Linux", label: "comtap (64-bit, static)", fileName: ASSET_NAMES.linuxBin },
];

// The tag (with its `v`) is the URL path segment; the file name carries the
// bare version.
export function downloadUrl(fileName) {
  return `https://github.com/${REPOSITORY}/releases/download/${TAG}/${fileName}`;
}

// Asset names are `comtap-<version>-<os>-<arch>.<ext>`. A plain
// MAJOR.MINOR.PATCH version keeps them unambiguous (a suffix like `-beta.1`
// would add hyphens that collide with the separators).
export function assertPlainVersion() {
  if (!/^\d+\.\d+\.\d+$/.test(VERSION)) {
    console.error(`Release version '${VERSION}' must be plain MAJOR.MINOR.PATCH (no pre-release or build suffix).`);
    process.exit(1);
  }
}
