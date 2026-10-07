import { describe, expect, it } from "vitest";
import {
  parseShellSend,
  parseVideoSurface,
} from "../../desktop/shared/mpv-protocol";

describe("desktop MPV protocol", () => {
  it("accepts the ShellVideo command and property shapes", () => {
    expect(parseShellSend("mpv-command", ["loadfile", "https://media.example/movie.mkv"])).toEqual({
      channel: "mpv-command",
      payload: ["loadfile", "https://media.example/movie.mkv"],
    });
    expect(parseShellSend("mpv-command", ["stop"])).toEqual({ channel: "mpv-command", payload: ["stop"] });
    expect(parseShellSend("mpv-command", [
      "sub-add",
      "https://subtitles.example/english.srt",
      "cached",
      "Panorama addon · p1 · English",
      "en",
    ])).toEqual({
      channel: "mpv-command",
      payload: ["sub-add", "https://subtitles.example/english.srt", "cached", "Panorama addon · p1 · English", "en"],
    });
    expect(parseShellSend("mpv-observe-prop", "time-pos")).toEqual({
      channel: "mpv-observe-prop",
      payload: "time-pos",
    });
    expect(parseShellSend("mpv-set-prop", ["pause", true])).toEqual({
      channel: "mpv-set-prop",
      payload: ["pause", true],
    });
    expect(parseShellSend("mpv-set-prop", ["sub-font", "DM Sans Medium"])).toEqual({
      channel: "mpv-set-prop",
      payload: ["sub-font", "DM Sans Medium"],
    });
    expect(parseShellSend("mpv-set-prop", ["sub-shadow-offset", 6])).toEqual({
      channel: "mpv-set-prop",
      payload: ["sub-shadow-offset", 6],
    });
  });

  it.each([
    ["mpv-command", ["quit"]],
    ["mpv-command", ["loadfile", "file:///tmp/movie.mkv"]],
    ["mpv-command", ["sub-add", "file:///tmp/subtitle.srt", "cached", "English", "en"]],
    ["mpv-command", ["sub-add", "https://subtitles.example/a.srt", "cached", "English\nunsafe", "en"]],
    ["mpv-command", Object.assign(["stop"], { nested: true })],
    ["mpv-observe-prop", "working-directory"],
    ["mpv-set-prop", ["script-opts", "unsafe"]],
    ["unknown", []],
  ])("rejects malformed or unknown messages", (channel, payload) => {
    expect(() => parseShellSend(channel, payload)).toThrow();
  });

  it("hides invalid or empty native surface bounds", () => {
    expect(parseVideoSurface({ visible: true, x: 10, y: 20, width: 0, height: 100, scaleFactor: 2 })).toEqual({
      visible: false,
      x: 0,
      y: 0,
      width: 0,
      height: 0,
      scaleFactor: 2,
    });
    expect(() => parseVideoSurface({ visible: true, x: -1, y: 0, width: 10, height: 10, scaleFactor: 1 })).toThrow();
  });
});
