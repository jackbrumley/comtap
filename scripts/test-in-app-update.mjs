#!/usr/bin/env node

// Checks the in-app updater on an installed build (Release and Update
// Standard, section 7, "Install test"): runs `comtap update`, the same path
// as F1 -> Update, and checks that the website's update script started and
// finished with "updated". The release workflow runs this last on each
// platform, with COMTAP_UPDATE_BASE_URL pointing at the tag's docs/ so the
// tag's own update and install scripts are used, and COMTAP_NO_RELAUNCH=1.
// It updates the build to the latest published release.
//
//   Linux:   comtap becomes the update script (exec), so its output is this
//            process's output.
//   Windows: comtap starts the update script in a new console window and
//            exits; the script writes C:\ProgramData\ComTap\logs\update.log.
//
//   node scripts/test-in-app-update.mjs <path to the installed comtap binary>

import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const STARTED_LINE = "[comtap-update] started";
const UPDATED_LINE = "[comtap-update] finished: updated";
const FAILED_LINE = "[comtap-update] finished: failed";
// Downloading and installing the release.
const UPDATE_TIMEOUT_MS = 600_000;
// Windows: how long the new console may take to print the started line.
const START_TIMEOUT_MS = 60_000;
const WINDOWS = process.platform === "win32";
const UPDATE_LOG = WINDOWS
  ? join(process.env.ProgramData ?? "C:\\ProgramData", "ComTap", "logs", "update.log")
  : "/var/log/comtap/update.log";

const binary = process.argv[2];
if (!binary) {
  console.error("Usage: node scripts/test-in-app-update.mjs <comtap binary>");
  process.exit(2);
}
if (!process.env.COMTAP_UPDATE_BASE_URL) {
  console.error("COMTAP_UPDATE_BASE_URL must point at the scripts under test");
  process.exit(2);
}
if (process.env.COMTAP_NO_RELAUNCH !== "1") {
  console.error("Set COMTAP_NO_RELAUNCH=1, so the update opens no ComTap screen");
  process.exit(2);
}

const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

function read(path) {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return "";
  }
}

function fail(message, output = "") {
  console.error(`FAIL: ${message}`);
  if (output) console.error(`--- comtap update output ---\n${output}`);
  console.error(`--- ${UPDATE_LOG} ---\n${read(UPDATE_LOG)}`);
  process.exit(1);
}

// Only this run's part of the update log counts.
const logStart = read(UPDATE_LOG).length;
const newLog = () => read(UPDATE_LOG).slice(logStart);

let output = "";
const app = spawn(binary, ["update"], { stdio: ["ignore", "pipe", "pipe"] });
for (const stream of [app.stdout, app.stderr]) {
  stream.on("data", (chunk) => {
    output += chunk;
    process.stdout.write(chunk);
  });
}
const exit = await Promise.race([
  new Promise((resolve) => app.on("exit", (code, signal) => resolve({ code, signal }))),
  sleep(UPDATE_TIMEOUT_MS).then(() => null),
]);
if (!exit) {
  app.kill();
  fail(`comtap update did not exit within ${UPDATE_TIMEOUT_MS / 1000}s`, output);
}
if (exit.code !== 0) {
  fail(`comtap update exited with ${JSON.stringify(exit)}`, output);
}

if (!WINDOWS) {
  if (!output.includes(STARTED_LINE)) fail("the update script never printed its started line", output);
  if (!output.includes(UPDATED_LINE)) fail("the update script did not finish with 'updated'", output);
  console.log("PASS: comtap update ran the update script and the update succeeded");
  process.exit(0);
}

const startDeadline = Date.now() + START_TIMEOUT_MS;
while (!newLog().includes(STARTED_LINE)) {
  if (Date.now() > startDeadline) {
    fail(`the update script did not log its started line within ${START_TIMEOUT_MS / 1000}s`, output);
  }
  await sleep(1_000);
}
console.log("Updater started; waiting for the update to finish");
const deadline = Date.now() + UPDATE_TIMEOUT_MS;
while (Date.now() < deadline) {
  const log = newLog();
  if (log.includes(UPDATED_LINE)) {
    console.log(`--- ${UPDATE_LOG} (this run) ---\n${log}`);
    console.log("PASS: comtap update started the update script and the update succeeded");
    process.exit(0);
  }
  if (log.includes(FAILED_LINE)) fail("the update script reported a failed update", output);
  await sleep(2_000);
}
fail(`the update did not finish within ${UPDATE_TIMEOUT_MS / 1000}s`, output);
