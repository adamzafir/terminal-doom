#!/usr/bin/env node

"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const root = path.resolve(__dirname, "..");
const executableName = process.platform === "win32" ? "doom.exe" : "doom";
const source = path.join(root, "target", "release", executableName);
const nativeDirectory = path.join(root, "npm", "bin", "native");
const destination = path.join(nativeDirectory, executableName);

console.log("terminal-doom: building the native Rust executable...");
const build = spawnSync(
  "cargo",
  ["build", "--release", "--locked", "--bin", "doom"],
  { cwd: root, stdio: "inherit" }
);

if (build.error && build.error.code === "ENOENT") {
  console.error(
    "terminal-doom: Rust is required. Install it from https://rustup.rs and retry."
  );
  process.exit(1);
}
if (build.status !== 0) {
  process.exit(build.status === null ? 1 : build.status);
}

fs.mkdirSync(nativeDirectory, { recursive: true });
fs.copyFileSync(source, destination);
if (process.platform !== "win32") {
  fs.chmodSync(destination, 0o755);
}
console.log(`terminal-doom: installed native executable at ${destination}`);
