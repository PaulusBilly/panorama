import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const hlsConfig = require("@stremio/stremio-video/src/HTMLVideo/hlsConfig.js") as {
  maxBufferLength: number;
  maxMaxBufferLength: number;
  maxBufferSize: number;
};

describe("browser playback buffering", () => {
  it("allows roughly one minute of forward buffer for high-bitrate streams", () => {
    expect(hlsConfig.maxBufferLength).toBe(60);
    expect(hlsConfig.maxMaxBufferLength).toBe(120);
    expect(hlsConfig.maxBufferSize).toBe(192 * 1024 * 1024);
  });
});
