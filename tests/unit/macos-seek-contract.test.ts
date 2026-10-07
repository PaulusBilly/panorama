import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const source = readFileSync("desktop/native/mpv-host/src/addon.mm", "utf8");

describe("macOS native seek contract", () => {
  it("uses exact seeking for committed seeks", () => {
    const seek = source.match(/- \(void\)seekToSeconds:\(double\)seconds \{[\s\S]*?\n}/)?.[0] ?? "";

    expect(seek).toContain('"absolute+exact"');
  });

  it("starts a replacement source at the exact resume position", () => {
    expect(source).toContain('mpv_set_option_string(_mpv, "hr-seek", "default")');
  });
});
