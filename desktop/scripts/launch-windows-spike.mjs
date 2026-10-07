import { spawn } from "node:child_process";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const electron = createRequire(import.meta.url)("electron");
const entry = path.join(projectRoot, "desktop-dist", "spike", "main.js");
const mode = process.argv.includes("--transparent") ? "transparent" : "opaque";

const child = spawn(electron, [entry], {
  cwd: projectRoot,
  detached: process.platform === "win32",
  stdio: process.platform === "win32" ? "ignore" : "inherit",
  windowsHide: true,
  env: {
    ...process.env,
    PANORAMA_SPIKE_WINDOW_MODE: mode,
  },
});
if (process.platform === "win32") child.unref();
else child.once("exit", (code) => { process.exitCode = code ?? 1; });
