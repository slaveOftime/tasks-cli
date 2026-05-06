#!/usr/bin/env node
// Thin launcher: finds the bundled tli binary and execs it.

const fs = require("fs");
const path = require("path");
const { spawnSync } = require("child_process");

const binaryNames = {
  "linux:x64": "tli-linux-x64",
  "darwin:arm64": "tli-darwin-arm64",
  "win32:x64": "tli-win32-x64.exe",
};

const binaryName = binaryNames[`${process.platform}:${process.arch}`];

if (!binaryName) {
  console.error(
    `tli: unsupported platform/arch: ${process.platform}/${process.arch}. ` +
      "Please download a release from https://github.com/slaveoftime/tasks-cli/releases"
  );
  process.exit(1);
}

const binaryPath = path.join(__dirname, binaryName);

if (!fs.existsSync(binaryPath)) {
  console.error(
    `tli: bundled binary not found at ${binaryPath}. Try reinstalling: npm install -g @slaveoftime/tli`
  );
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), {
  stdio: "inherit",
  env: process.env,
});

process.exit(result.status ?? 1);
