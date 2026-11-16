#!/usr/bin/env node

// Copies the Windows release build into release-artifacts/ under its standard
// release name (from release-assets.mjs). The release workflow uploads the
// folder's whole contents.
//
//   npm run release:windows && node scripts/package-release.mjs

import { copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { ASSET_NAMES, VERSION, assertPlainVersion } from "./release-assets.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const OUT_DIR = resolve(ROOT, "release-artifacts");
const BUILD = resolve(ROOT, "target", "release", "comtap.exe");

function fail(message) {
  console.error(`\nPackaging failed: ${message}`);
  process.exit(1);
}

assertPlainVersion();
if (process.platform !== "win32") {
  fail("ComTap ships only for Windows; package it on Windows.");
}
if (!existsSync(BUILD)) {
  fail(`${BUILD} not found. Run 'npm run release:windows' first.`);
}

// Refuse to package a build whose own version differs from the configured
// release version, so a stale build is never mislabeled.
const reported = execFileSync(BUILD, ["--version"], { encoding: "utf8" }).trim();
if (reported !== `comtap ${VERSION}`) {
  fail(`the build reports '${reported}', expected 'comtap ${VERSION}'. Rebuild it.`);
}

// Start from an empty folder so its whole contents can be uploaded as-is.
rmSync(OUT_DIR, { recursive: true, force: true });
mkdirSync(OUT_DIR, { recursive: true });

// No checksum files are produced: GitHub computes a SHA-256 digest for every
// uploaded release asset, and install.ps1 verifies against that digest.
copyFileSync(BUILD, resolve(OUT_DIR, ASSET_NAMES.windowsExe));
console.log(`ComTap v${VERSION}: comtap.exe  →  release-artifacts/${ASSET_NAMES.windowsExe}`);
