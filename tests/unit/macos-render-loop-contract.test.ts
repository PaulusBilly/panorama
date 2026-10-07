import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const source = readFileSync("desktop/native/mpv-host/src/addon.mm", "utf8");
const binding = readFileSync("desktop/native/mpv-host/binding.gyp", "utf8");

describe("macOS native render loop contract", () => {
  it("uses a display-linked serialized owner and reports every completed swap", () => {
    expect(binding).toContain("-framework CoreVideo");
    expect(source).toContain("CVDisplayLinkSetOutputCallback");
    expect(source).toContain("dispatch_queue_create");
    expect(source).toContain("mpv_render_context_update");
    expect(source).toContain("MPV_RENDER_UPDATE_FRAME");
    expect(source).toContain("mpv_render_context_report_swap");
  });

  it("keeps the MPV callback signal-only and stops callbacks before teardown", () => {
    const callback = source.match(/static void requestMpvRender[\s\S]*?\n}/)?.[0] ?? "";
    expect(callback).toContain("signalMpvRender");
    expect(callback).not.toContain("setNeedsDisplay");
    expect(callback).not.toContain("mpv_render_context_render");
    expect(source.indexOf("CVDisplayLinkStop")).toBeLessThan(source.indexOf("mpv_render_context_free"));
  });
});
