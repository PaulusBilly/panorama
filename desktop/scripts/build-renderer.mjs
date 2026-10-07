import { spawn } from "node:child_process";

const child = spawn(
  process.execPath,
  ["node_modules/next/dist/bin/next", "build", "--webpack"],
  {
    stdio: "inherit",
    env: {
      ...process.env,
      PANORAMA_DESKTOP_BUILD: "1",
      PANORAMA_DIST_DIR: ".next-desktop",
    },
  },
);

child.once("error", (error) => {
  console.error(error);
  process.exitCode = 1;
});

child.once("exit", (code, signal) => {
  process.exitCode = signal ? 1 : (code ?? 1);
});
