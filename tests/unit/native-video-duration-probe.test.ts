import { createRequire } from "node:module";
import { afterEach, describe, expect, it, vi } from "vitest";

const require = createRequire(import.meta.url);
const withStreamingServer = require("@stremio/stremio-video/src/withStreamingServer/withStreamingServer.js");
const platform = require("@stremio/stremio-video/src/platform.js");

type Listener = (...args: unknown[]) => void;

class LoadedShellVideo {
  static canPlayStream = () => Promise.resolve(true);
  static manifest = {
    name: "TestShellVideo",
    external: false,
    props: ["loaded"],
    commands: ["load", "unload", "destroy"],
    events: ["propValue", "propChanged", "ended", "error"],
  };

  private listeners = new Map<string, Listener[]>();

  on(eventName: string, listener: Listener): void {
    this.listeners.set(eventName, [...(this.listeners.get(eventName) ?? []), listener]);
  }

  dispatch(action: { type?: string; propName?: string }): void {
    if (action.type === "observeProp" && action.propName === "loaded") {
      for (const listener of this.listeners.get("propChanged") ?? []) listener("loaded", true);
    }
  }
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("native media duration probing", () => {
  it("publishes the container duration after native playback starts on Windows", async () => {
    platform.set("windows");
    vi.stubGlobal("fetch", vi.fn(async (input: string | URL) => {
      const requestUrl = String(input);
      if (requestUrl.includes("/hlsv2/probe?")) {
        return {
          ok: true,
          json: async () => ({
            format: { name: "matroska", duration: 6_015.488 },
            streams: [{ track: "video", codec: "hevc", frameRate: 24 }],
          }),
        };
      }
      if (requestUrl.includes("/opensubHash?")) {
        return { ok: true, json: async () => ({ result: { hash: null, size: null } }) };
      }
      return { ok: false, status: 404, statusText: "Not Found", json: async () => ({}) };
    }));

    const VideoWithStreamingServer = withStreamingServer(LoadedShellVideo);
    const video = new VideoWithStreamingServer();
    let videoParams: { durationMs?: number | null } | null = null;
    video.on("propChanged", (propName: unknown, propValue: unknown) => {
      if (propName === "videoParams") videoParams = propValue as { durationMs?: number | null };
    });
    video.dispatch({ type: "observeProp", propName: "videoParams" });
    video.dispatch({
      type: "command",
      commandName: "load",
      commandArgs: {
        stream: { url: "https://media.example/movie.mkv" },
        streamingServerURL: "http://127.0.0.1:11470",
        probeVideoParams: true,
      },
    });

    await vi.waitFor(() => expect(videoParams).toMatchObject({ durationMs: 6_015_488 }));
    expect(fetch).toHaveBeenCalledWith(expect.stringContaining("/hlsv2/probe?mediaURL="), expect.objectContaining({ signal: expect.any(AbortSignal) }));
  });
});
