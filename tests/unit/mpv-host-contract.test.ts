import { describe, expect, it } from "vitest";
import { MpvHost } from "../../desktop/native/mpv-host/mpv-host";
import type {
  MpvHostCommand,
  MpvHostDiagnostics,
  NativeMpvBinding,
} from "../../desktop/native/mpv-host/types";

class RecordingBinding implements NativeMpvBinding {
  readonly commands: MpvHostCommand[] = [];
  destroyCount = 0;

  dispatch(command: MpvHostCommand): void {
    this.commands.push(command);
  }

  getDiagnostics(): MpvHostDiagnostics {
    return {
      mpvVersion: "0.41.0",
      videoCodec: null,
      hardwareDecoder: null,
      buffering: false,
      cacheSeconds: null,
      timeSeconds: null,
      durationSeconds: null,
      tracks: [],
    };
  }

  destroy(): void {
    this.destroyCount += 1;
  }
}

describe("MpvHost", () => {
  it("dispatches validated commands to the native binding", () => {
    const binding = new RecordingBinding();
    const host = new MpvHost(binding, "http://127.0.0.1:11470");

    host.dispatch({ type: "load", url: "http://127.0.0.1:11470/redacted/media", startSeconds: 12 });
    host.dispatch({ type: "setPaused", paused: true });
    host.dispatch({ type: "seek", seconds: 42 });
    host.dispatch({ type: "setBounds", x: 0, y: 0, width: 1280, height: 720, scaleFactor: 1 });
    host.dispatch({ type: "stop" });

    expect(binding.commands).toEqual([
      { type: "load", url: "http://127.0.0.1:11470/redacted/media", startSeconds: 12 },
      { type: "setPaused", paused: true },
      { type: "seek", seconds: 42 },
      { type: "setBounds", x: 0, y: 0, width: 1280, height: 720, scaleFactor: 1 },
      { type: "stop" },
    ]);
  });

  it.each([
    "https://127.0.0.1:11470/media",
    "http://localhost:11470/media",
    "http://127.0.0.1:3000/media",
    "file:///tmp/movie.mkv",
  ])("rejects a media URL outside the exact service origin: %s", (url) => {
    const host = new MpvHost(new RecordingBinding(), "http://127.0.0.1:11470");

    expect(() => host.dispatch({ type: "load", url, startSeconds: 0 })).toThrow("media URL");
  });

  it("allows only the exact pre-authorized external media URL", () => {
    const binding = new RecordingBinding();
    const externalUrl = "https://media.example.test/private/movie.mkv?token=redacted";
    const host = new MpvHost(binding, "http://127.0.0.1:11470", externalUrl);

    host.dispatch({ type: "load", url: externalUrl, startSeconds: 0 });

    expect(binding.commands).toEqual([
      { type: "load", url: externalUrl, startSeconds: 0 },
    ]);
    expect(() => host.dispatch({
      type: "load",
      url: "https://media.example.test/private/other.mkv?token=redacted",
      startSeconds: 0,
    })).toThrow("media URL");
  });

  it("allows media from the bounded discovered Stremio Service origins", () => {
    const binding = new RecordingBinding();
    const host = new MpvHost(binding, [
      "http://127.0.0.1:11470",
      "http://127.0.0.1:11471",
      "http://127.0.0.1:11472",
      "http://127.0.0.1:11473",
      "http://127.0.0.1:11474",
    ]);

    host.dispatch({ type: "load", url: "http://127.0.0.1:11471/redacted/media", startSeconds: 0 });

    expect(binding.commands).toContainEqual({
      type: "load",
      url: "http://127.0.0.1:11471/redacted/media",
      startSeconds: 0,
    });
    expect(() => host.dispatch({
      type: "load",
      url: "http://127.0.0.1:11475/redacted/media",
      startSeconds: 0,
    })).toThrow("media URL");
  });

  it.each([
    { type: "load", url: "http://127.0.0.1:11470/media", startSeconds: -1 },
    { type: "seek", seconds: -1 },
    { type: "setBounds", x: 0, y: 0, width: -1, height: 720, scaleFactor: 1 },
    { type: "setBounds", x: 0, y: 0, width: 1280, height: Number.NaN, scaleFactor: 1 },
    { type: "setBounds", x: 0, y: 0, width: 1280, height: 720, scaleFactor: 0 },
  ] satisfies MpvHostCommand[])("rejects malformed command %#", (command) => {
    const host = new MpvHost(new RecordingBinding(), "http://127.0.0.1:11470");

    expect(() => host.dispatch(command)).toThrow();
  });

  it("rejects commands after destruction and destroys the binding once", () => {
    const binding = new RecordingBinding();
    const host = new MpvHost(binding, "http://127.0.0.1:11470");

    host.destroy();
    host.destroy();

    expect(binding.destroyCount).toBe(1);
    expect(() => host.dispatch({ type: "stop" })).toThrow("destroyed");
    expect(() => host.getDiagnostics()).toThrow("destroyed");
  });
});
