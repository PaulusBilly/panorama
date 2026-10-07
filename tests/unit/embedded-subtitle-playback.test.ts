import { createRequire } from "node:module";
import { afterEach, describe, expect, it, vi } from "vitest";

const require = createRequire(import.meta.url);

afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

describe("embedded HLS subtitle playback", () => {
  it("selects the matching HLS subtitle rendition without reloading video", async () => {
    const htmlVideoPath = require.resolve("@stremio/stremio-video/src/HTMLVideo/HTMLVideo.js");
    const packageRequire = createRequire(htmlVideoPath);
    const Hls = packageRequire("hls.js") as {
      isSupported(): boolean;
      prototype: { subtitleTrack: number };
    };
    vi.spyOn(Hls, "isSupported").mockReturnValue(true);
    const subtitleTrackSetter = vi.spyOn(Hls.prototype, "subtitleTrack", "set");
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => undefined);
    const HTMLVideo = require(htmlVideoPath) as new (options: { containerElement: HTMLElement }) => {
      dispatch(action: Record<string, unknown>): void;
    };
    const containerElement = document.createElement("div");
    document.body.appendChild(containerElement);
    const player = new HTMLVideo({ containerElement });
    const videoElement = containerElement.querySelector("video");
    expect(videoElement).not.toBeNull();
    Object.defineProperty(videoElement, "textTracks", {
      configurable: true,
      value: [
        { mode: "disabled", language: "eng", label: "Forced" },
        { mode: "disabled", language: "eng", label: "English" },
      ],
    });

    player.dispatch({
      type: "command",
      commandName: "load",
      commandArgs: {
        stream: {
          url: "http://127.0.0.1:11470/hlsv2/master.m3u8",
          behaviorHints: {
            proxyHeaders: { response: { "content-type": "application/vnd.apple.mpegurl" } },
          },
        },
        autoplay: false,
        time: 0,
      },
    });
    await vi.waitFor(() => expect(Hls.isSupported).toHaveBeenCalled());

    player.dispatch({
      type: "setProp",
      propName: "selectedSubtitlesTrackId",
      propValue: "EMBEDDED_1",
    });

    expect(subtitleTrackSetter).toHaveBeenCalledWith(1);
    expect(videoElement?.getAttribute("src")).toBeNull();
  });
});
