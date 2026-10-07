import { spawn } from "node:child_process";
import process from "node:process";

const nextBin = "node_modules/next/dist/bin/next";
const env = {
  ...process.env,
  PANORAMA_DIST_DIR: ".next-playwright",
  NEXT_PUBLIC_PANORAMA_RUNTIME: "fake",
};

function startNext(args) {
  return spawn(process.execPath, [nextBin, ...args], {
    env,
    stdio: "inherit",
    windowsHide: true,
  });
}

function waitForExit(child) {
  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      if (signal) reject(new Error(`Next.js exited from signal ${signal}`));
      else resolve(code ?? 1);
    });
  });
}

const buildCode = await waitForExit(startNext(["build", "--webpack"]));
if (buildCode !== 0) process.exit(buildCode);

const server = startNext(["start", "-H", "127.0.0.1", "-p", "3102"]);
const stopServer = () => {
  if (!server.killed) server.kill();
};
process.once("SIGINT", stopServer);
process.once("SIGTERM", stopServer);
process.once("exit", stopServer);

process.exitCode = await waitForExit(server);
