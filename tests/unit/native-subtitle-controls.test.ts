import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { normalizeMpvSubtitleColor } from "../../runtime/video-engine";

describe("native subtitle controls", () => {
  it("converts session subtitle colors to the RGBA shape consumed by ShellVideo", () => {
    expect(normalizeMpvSubtitleColor("#ffffff")).toBe("#ffffffff");
    expect(normalizeMpvSubtitleColor("rgba(0, 0, 0, 0.68)")).toBe("#000000ad");
    expect(normalizeMpvSubtitleColor("rgba(0, 0, 0, 0.78)")).toBe("#000000c7");
  });

  it("converts runtime milliseconds to MPV subtitle-delay seconds", async () => {
    const sends: Array<[string, unknown]> = [];
    const listeners = new Map<string, (payload: unknown) => void>();
    window.panoramaDesktop = {
      getCapabilities: async () => ({
        platform: "darwin",
        architecture: "arm64",
        nativePlayback: { status: "ready", device: "ShellVideo", mpvVersion: "0.41.0" },
        appVersion: "0.1.0",
      }),
      openExternal: async () => undefined,
      mpv: {
        send: (channel, payload) => sends.push([channel, payload]),
        on: (channel, listener) => {
          listeners.set(channel, listener);
          return () => listeners.delete(channel);
        },
        setVideoSurface: () => undefined,
      },
    };
    const { createVideoEngine } = await import("../../runtime/video-engine");
    const session = await createVideoEngine(document.createElement("div"));

    session.engine.dispatch({
      type: "command",
      commandName: "load",
      commandArgs: { stream: { url: "http://127.0.0.1:11470/media" }, time: 0 },
    });
    listeners.get("mpv-prop-change")?.({ name: "mpv-version", data: "0.41.0" });
    await Promise.resolve();
    session.engine.dispatch({ type: "setProp", propName: "subtitlesDelay", propValue: 1500 });
    session.engine.dispatch({ type: "setProp", propName: "subtitlesBackgroundColor", propValue: "rgba(0, 0, 0, 0.68)" });
    session.engine.dispatch({ type: "setProp", propName: "subtitlesPaddingX", propValue: 14 });
    session.engine.dispatch({ type: "setProp", propName: "subtitlesPaddingY", propValue: 6 });
    session.engine.dispatch({ type: "setProp", propName: "subtitlesFontWeight", propValue: "semibold" });

    expect(session.device).toBe("ShellVideo");
    expect(sends).toContainEqual(["mpv-set-prop", ["sub-delay", 1.5]]);
    expect(sends).toContainEqual(["mpv-set-prop", ["sub-back-color", "#ad000000"]]);
    expect(sends.some((call) => call[0] === "mpv-set-prop" && Array.isArray(call[1]) && call[1][0] === "sub-shadow-offset")).toBe(false);
    expect(sends).toContainEqual(["mpv-set-prop", ["sub-font", "DM Sans Medium"]]);
    expect(sends).toContainEqual(["mpv-set-prop", ["sub-bold", false]]);
    session.destroy();
  });

  it("uses the packaged DM Sans medium face with an Apple TV-like background box", () => {
    const mainSource = readFileSync("desktop/main/main.ts", "utf8");
    const font = readFileSync("public/fonts/DMSans-Medium.ttf");

    expect(font.subarray(0, 4)).toEqual(Buffer.from([0x00, 0x01, 0x00, 0x00]));
    expect(mainSource).toContain('name: "sub-font", value: "DM Sans"');
    expect(mainSource).toContain('name: "sub-border-style", value: "background-box"');
    expect(mainSource).toContain('name: "sub-border-size", value: 0');
    expect(mainSource).toContain('name: "sub-shadow-offset", value: 10');
  });

  it("lets explicit HTML subtitle cues own addon appearance styles", () => {
    const css = readFileSync("app/globals.css", "utf8");
    const patch = readFileSync("patches/@stremio__stremio-video@0.0.93.patch", "utf8");

    expect(css).toContain(".player-video .panorama-subtitle-cue");
    expect(css).not.toContain(".player-video > div > *");
    expect(css).not.toMatch(/panorama-subtitle-cue[^}]*padding:[^;]+!important/);
    expect(css).not.toMatch(/panorama-subtitle-cue[^}]*font-weight:[^;]+!important/);
    expect(patch).toContain("subtitlesElement.className = 'panorama-subtitles'");
    expect(patch).toContain("cueNode.classList.add('panorama-subtitle-cue')");
  });

  it("dispatches remote native subtitle loading asynchronously", () => {
    const windowsSource = readFileSync("desktop/native/mpv-host/src/addon_win.cc", "utf8");
    const macosSource = readFileSync("desktop/native/mpv-host/src/addon.mm", "utf8");

    expect(windowsSource).toContain('values.front() == "sub-add"');
    expect(windowsSource).toContain("api_->command_async(mpv_, 0, command.data())");
    expect(macosSource).toContain('values.front() == "sub-add"');
    expect(macosSource).toContain("mpv_command_async([view_ mpvHandle], 0, command.data())");
  });

  it("forwards changing positive MPV durations instead of freezing the first value", () => {
    const patch = readFileSync("patches/@stremio__stremio-video@0.0.93.patch", "utf8");
    const shellVideo = readFileSync("node_modules/@stremio/stremio-video/src/ShellVideo/ShellVideo.js", "utf8");

    expect(patch).toContain("props[args.name] = duration === null ? null : Math.round(duration * 1000)");
    expect(shellVideo).not.toContain("intDuration === avgDuration");
    expect(shellVideo).toContain("props[args.name] = duration === null ? null : Math.round(duration * 1000)");
  });
});
