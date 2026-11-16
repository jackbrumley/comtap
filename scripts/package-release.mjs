#!/usr/bin/env node

// Copies this platform's release build into release-artifacts/ under its
// standard release name (from release-assets.mjs). The release workflow
// uploads the folder's whole contents from each platform's runner.
//
//   Windows: npm run release:windows && node scripts/package-release.mjs
//   Linux:   npm run release:linux   && node scripts/package-release.mjs

import { chmodSync, copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { ASSET_NAMES, VERSION, assertPlainVersion } from "./release-assets.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OUT_DIR = resolve(ROOT, "release-artifacts");

const PLATFORMS = {
  win32: {
    build: resolve(ROOT, "target", "release", "comtap.exe"),
    asset: ASSET_NAMES.windowsExe,
    buildScript: "release:windows",
  },
  linux: {
    build: resolve(ROOT, "target", "x86_64-unknown-linux-musl", "release", "comtap"),
    asset: ASSET_NAMES.linuxBin,
    buildScript: "release:linux",
  },
};

function fail(message) {
  console.error(`\nPackaging failed: ${message}`);
  process.exit(1);
}

assertPlainVersion();
const platform = PLATFORMS[process.platform];
if (!platform) {
  fail(`ComTap ships for Windows and Linux; this is ${process.platform}.`);
}
if (!existsSync(platform.build)) {
  fail(`${platform.build} not found. Run 'npm run ${platform.buildScript}' first.`);
}

// Refuse to package a build whose own version differs from the configured
// release version, so a stale build is never mislabeled.
const reported = execFileSync(platform.build, ["--version"], { encoding: "utf8" }).trim();
if (reported !== `comtap ${VERSION}`) {
  fail(`the build reports '${reported}', expected 'comtap ${VERSION}'. Rebuild it.`);
}

// Start from an empty folder so its whole contents can be uploaded as-is.
rmSync(OUT_DIR, { recursive: true, force: true });
mkdirSync(OUT_DIR, { recursive: true });

// No checksum files are produced: GitHub computes a SHA-256 digest for every
// uploaded release asset, and the installers verify against that digest.
const out = resolve(OUT_DIR, platform.asset);
copyFileSync(platform.build, out);
if (process.platform !== "win32") {
  chmodSync(out, 0o755);
}
console.log(`ComTap v${VERSION}: ${platform.build}  →  release-artifacts/${platform.asset}`);
