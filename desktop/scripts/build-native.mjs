import { spawn } from "node:child_process";
import path from "node:path";
import { createRequire } from "node:module";

const projectRoot = process.cwd();
const require = createRequire(import.meta.url);
const electronVersion = require("electron/package.json").version;

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: projectRoot, stdio: "inherit", env: process.env });
    child.once("error", reject);
    child.once("exit", (code) => code === 0 ? resolve() : reject(new Error(`${command} exited with ${code}`)));
  });
}

if (process.platform === "darwin") {
  await run("python3", [path.join(projectRoot, "desktop/scripts/build-macos-libmpv.py")]);
}

if (process.platform === "win32") {
  await run(process.execPath, [path.join(projectRoot, "desktop/scripts/stage-windows-libmpv.mjs")]);
}

await run(process.execPath, [
  path.join(projectRoot, "node_modules/node-gyp/bin/node-gyp.js"),
  "rebuild",
  "--directory", "desktop/native/mpv-host",
  `--target=${electronVersion}`,
  "--dist-url=https://electronjs.org/headers",
  `--arch=${process.arch}`,
  `--devdir=${path.join(projectRoot, ".cache/node-gyp")}`,
]);
