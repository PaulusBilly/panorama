import { createRequire } from "node:module";
import { afterEach, describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const platform = require("@stremio/stremio-video/src/platform.js") as {
  set(value: string | null): void;
};
const supportsTranscoding = require("@stremio/stremio-video/src/supportsTranscoding.js") as () => Promise<boolean>;

afterEach(() => platform.set(null));

describe("Stremio Video native stream selection", () => {
  it("skips browser HLS conversion when ShellVideo runs on macOS", async () => {
    platform.set("macos");

    await expect(supportsTranscoding()).resolves.toBe(false);
  });

  it("skips browser HLS conversion when ShellVideo runs on Windows", async () => {
    platform.set("windows");
    await expect(supportsTranscoding()).resolves.toBe(false);
  });

  it("retains browser transcoding for browser platforms", async () => {
    platform.set("web");
    await expect(supportsTranscoding()).resolves.toBe(true);
  });
});
