#!/usr/bin/env node

"use strict";

const path = require("node:path");
const { spawnSync } = require("node:child_process");

const executable = path.join(
  __dirname,
  "native",
  process.platform === "win32" ? "doom.exe" : "doom"
);
const result = spawnSync(executable, process.argv.slice(2), { stdio: "inherit" });

if (result.error) {
  console.error(`doom: could not start native game: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status === null ? 1 : result.status);
