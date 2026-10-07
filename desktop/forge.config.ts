import { execFileSync } from "node:child_process";
import { readdir } from "node:fs/promises";
import path from "node:path";
import type { ForgeConfig } from "@electron-forge/shared-types";
import { MakerZIP } from "@electron-forge/maker-zip";

const config: ForgeConfig = {
  packagerConfig: {
    asar: true,
    icon: "public/favicon/favicon",
    extraResource: ["desktop-resources"],
    ignore: [
      /^\/(?!desktop-dist(?:\/|$)|package\.json$)/,
    ],
  },
  hooks: {
    postPackage: async (_config, result) => {
      if (result.platform !== "darwin") return;
      for (const output of result.outputPaths) {
        for (const entry of await readdir(output)) {
          if (!entry.endsWith(".app")) continue;
          const app = path.join(output, entry);
          execFileSync("codesign", ["--force", "--deep", "--sign", "-", app]);
          execFileSync("codesign", ["--verify", "--deep", "--strict", app]);
        }
      }
    },
  },
  makers: [new MakerZIP({}, ["darwin", "win32"])],
};

export default config;
