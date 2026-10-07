import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { PlaybackSettingsController } from "../../desktop/main/playback-settings";
import { MpvHost } from "../../desktop/native/mpv-host/mpv-host";
import type { MpvHostCommand, MpvHostDiagnostics } from "../../desktop/native/mpv-host/types";
import { parsePlaybackSettings } from "../../desktop/shared/playback-settings";

function fixture(file?: string, rendererBackend?: string) {
  const commands: MpvHostCommand[] = [];
  const diagnostics: MpvHostDiagnostics = { mpvVersion: "0.41.0", videoCodec: null, hardwareDecoder: null, buffering: false, cacheSeconds: null, timeSeconds: 0, durationSeconds: 60, tracks: [], audioDevices: [{ name: "private-receiver-id", description: "Receiver", supportedPassthroughCodecs: ["ac3", "untrusted"] }], audioOutputFormat: "spdif-ac3" };
  diagnostics.rendererBackend = rendererBackend;
  let fail = "";
  const host = new MpvHost({ getDiagnostics: () => diagnostics, destroy() {}, dispatch(command) { if (command.type === "setProperty" && command.name === fail) { fail = ""; throw new Error("native private error"); } commands.push(command); } }, "http://127.0.0.1:11471");
  const settings = new PlaybackSettingsController(host, file);
  settings.initialize(diagnostics);
  const request = (passthrough = true) => ({ deviceId: settings.getState().devices[1].id, channels: "auto" as const, video: "auto" as const, passthrough });
  return { settings, commands, diagnostics, request, failNext(name: string) { fail = name; } };
}

describe("desktop playback settings", () => {
  it.each(["macvk", "opengl"])("uses decoded system audio with automatic format negotiation on %s", (rendererBackend) => {
    const { commands } = fixture(undefined, rendererBackend);
    expect(commands.filter((command) => command.type === "setProperty" && /^(audio-|volume)/.test(command.name))).toEqual([
      { type: "setProperty", name: "audio-spdif", value: "" },
      { type: "setProperty", name: "audio-device", value: "auto" },
      { type: "setProperty", name: "audio-channels", value: "auto-safe" },
      { type: "setProperty", name: "volume", value: 100 },
      { type: "setProperty", name: "audio-spdif", value: "" },
    ]);
  });

  it("retries macOS output with decoded audio and the last selected track only once", () => {
    const { settings, commands, diagnostics, request } = fixture(undefined, "macvk");
    diagnostics.selectedAudioId = "2";
    settings.poll(diagnostics);
    settings.volume(165);
    settings.set({ ...request(), channels: "5.1" });
    commands.length = 0;
    diagnostics.selectedAudioId = "no";
    diagnostics.audioOutputFormat = null;
    diagnostics.audioOutputErrorSequence = 1;
    settings.poll(diagnostics);
    expect(commands).toEqual([
      { type: "setProperty", name: "audio-spdif", value: "" },
      { type: "setProperty", name: "audio-device", value: "auto" },
      { type: "setProperty", name: "audio-channels", value: "5.1" },
      { type: "setProperty", name: "volume", value: 165 },
      { type: "setProperty", name: "audio-spdif", value: "" },
      { type: "setProperty", name: "aid", value: "2" },
    ]);
    commands.length = 0;
    diagnostics.audioOutputErrorSequence = 2;
    settings.poll(diagnostics);
    expect(commands).toEqual([]);
  });

  it.each(["auto", "stereo", "5.1", "7.1"] as const)("preserves macOS device, passthrough consent, and %s channels", (channels) => {
    const { settings, commands, request } = fixture(undefined, "macvk");
    commands.length = 0;
    settings.set({ ...request(), channels });
    expect(commands).toContainEqual({ type: "setProperty", name: "audio-device", value: "private-receiver-id" });
    expect(commands).toContainEqual({ type: "setProperty", name: "audio-channels", value: channels === "auto" ? "auto-safe" : channels });
    expect(commands).toContainEqual({ type: "setProperty", name: "audio-spdif", value: "ac3" });
    expect(settings.getState().effectivePassthrough).toBe(true);
  });

  it("retains Windows PCM format negotiation for default and fallback output", () => {
    const { settings, commands, diagnostics } = fixture(undefined, "d3d11");
    diagnostics.audioOutputErrorSequence = 1;
    settings.poll(diagnostics);
    expect(commands.some((command) => command.type === "setProperty" && command.name === "audio-format")).toBe(false);
  });

  it("validates exact IPC shape and keeps native device identifiers private", () => {
    expect(() => parsePlaybackSettings({ deviceId: "private-receiver-id", channels: "auto", video: "auto", passthrough: true })).toThrow();
    expect(() => parsePlaybackSettings({ deviceId: "default", channels: "auto", video: "auto", passthrough: false, extra: true })).toThrow();
    const { settings, commands } = fixture();
    expect(JSON.stringify(settings.getState())).not.toContain("private-receiver-id");
    expect(commands).toContainEqual({ type: "setProperty", name: "audio-spdif", value: "" });
    expect(commands.some((command) => command.type === "setProperty" && /samplerate|exclusive/.test(command.name))).toBe(false);
    expect(() => settings.set({ deviceId: "default", channels: "auto", video: "auto", passthrough: true })).toThrow();
  });

  it("allows supported codecs only, prevents amplification and restores PCM volume at other speeds", () => {
    const { settings, request, commands } = fixture();
    settings.volume(170);
    settings.set(request());
    expect(settings.volume(200)).toBe(100);
    expect(commands.at(-3)).toEqual({ type: "setProperty", name: "audio-spdif", value: "ac3" });
    settings.setSpeed(1.5);
    expect(settings.getState().effectivePassthrough).toBe(false);
    expect(commands).toContainEqual({ type: "setProperty", name: "volume", value: 170 });
    settings.setSpeed(1);
    expect(settings.getState().effectivePassthrough).toBe(true);
  });

  it("falls back once on audio output failure and removes unplugged selections", () => {
    const { settings, request, diagnostics, commands } = fixture();
    settings.set(request());
    diagnostics.audioOutputErrorSequence = 1;
    settings.poll(diagnostics);
    expect(settings.getState().effectivePassthrough).toBe(false);
    const count = commands.length;
    diagnostics.audioOutputErrorSequence = 2;
    settings.poll(diagnostics);
    expect(commands).toHaveLength(count);
    diagnostics.audioDevices = [];
    settings.poll(diagnostics);
    expect(settings.getState().deviceId).toBe("default");
    expect(settings.getState().passthrough).toBe(false);
  });

  it("rolls back failed writes and persists per-device consent atomically", () => {
    const directory = mkdtempSync(join(tmpdir(), "panorama-settings-"));
    const file = join(directory, "settings.json");
    try {
      const { settings, request, failNext } = fixture(file);
      settings.set(request());
      expect(JSON.parse(readFileSync(file, "utf8")).receivers).toEqual(["private-receiver-id"]);
      failNext("target-trc");
      expect(() => settings.set({ ...request(), channels: "stereo" })).toThrow("Playback settings could not be saved");
      expect(settings.getState().channels).toBe("auto");
      expect(fixture(file).settings.getState().passthrough).toBe(true);
    } finally { rmSync(directory, { recursive: true, force: true }); }
  });

  it("changes video output without reopening audio and clears recovery tracks on replacement", () => {
    const { settings, request, diagnostics, commands } = fixture();
    settings.set(request(false));
    commands.length = 0;
    settings.set({ ...request(false), video: "sdr" });
    expect(commands.map((command) => command.type === "setProperty" && command.name)).toEqual(["target-prim", "target-trc"]);
    diagnostics.selectedAudioId = "2";
    settings.poll(diagnostics);
    settings.resetSource();
    diagnostics.selectedAudioId = null;
    diagnostics.audioOutputErrorSequence = 1;
    settings.poll(diagnostics);
    expect(commands.some((command) => command.type === "setProperty" && command.name === "aid")).toBe(false);
  });
});
