import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import PlayerDialog from "../../components/PlayerDialog";
import { RuntimeProvider } from "../../components/RuntimeMount";
import { FakeRuntime } from "../../runtime/fake-runtime";

beforeEach(() => {
  vi.stubGlobal("innerWidth", 1920);
  vi.stubGlobal("innerHeight", 1080);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    if (this.classList.contains("player-range-control")) return new DOMRect(0, 0, 200, 20);
    if (this.classList.contains("player-range-thumb")) return new DOMRect(0, 0, 14, 14);
    return Element.prototype.getBoundingClientRect.call(this);
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
  document.body.classList.remove("panorama-windows");
  delete window.panoramaDesktop;
  document.body.classList.remove("panorama-desktop-fullscreen");
  vi.restoreAllMocks();
});

function readyRuntime(player: Partial<ReturnType<FakeRuntime["getSnapshot"]>["player"]> = {}) {
  const runtime = new FakeRuntime();
  runtime.setStateForTest({
    player: {
      ...runtime.getSnapshot().player,
      status: "ready",
      stage: null,
      paused: false,
      title: "Aftersun",
      time: 120,
      duration: 6120,
      ...player,
    },
  });
  return runtime;
}

describe("player native surface", () => {
  it("toggles playback from a click on the video, except a click that only dismisses a panel", () => {
    const runtime = readyRuntime();
    const pause = vi.spyOn(runtime, "setPlaybackPaused");
    const view = render(<PlayerDialog runtime={runtime} snapshot={runtime.getSnapshot()} onClose={vi.fn()} />);
    const video = view.container.querySelector<HTMLElement>(".player-video");
    if (!video) throw new Error("Missing player video surface");

    fireEvent.pointerDown(video);
    fireEvent.click(video);
    expect(pause).toHaveBeenCalledWith(true);

    pause.mockClear();
    const subtitles = screen.getByRole("button", { name: "Subtitles" });
    fireEvent.click(subtitles);
    expect(subtitles).toHaveAttribute("aria-expanded", "true");
    fireEvent.pointerDown(video);
    fireEvent.click(video);
    expect(pause).not.toHaveBeenCalled();
    expect(subtitles).toHaveAttribute("aria-expanded", "false");
  });

  it("keeps the controls up while paused", () => {
    const runtime = readyRuntime({ paused: true });
    render(<PlayerDialog runtime={runtime} snapshot={runtime.getSnapshot()} onClose={vi.fn()} />);
    vi.useFakeTimers();
    try {
      fireEvent.pointerMove(screen.getByTestId("player-surface"));
      act(() => vi.advanceTimersByTime(5_000));
      expect(screen.getByTestId("player-controls-overlay")).toBeVisible();
    } finally {
      vi.useRealTimers();
    }
  });

  it("titles the seek pill and morphs one shared panel between settings", async () => {
    const runtime = readyRuntime();
    render(<PlayerDialog runtime={runtime} snapshot={runtime.getSnapshot()} onClose={vi.fn()} />);
    const overlay = screen.getByTestId("player-controls-overlay");
    expect(within(overlay).getByText("Aftersun")).toBeVisible();
    expect(within(overlay).getByText("2:00")).toBeVisible();
    expect(within(overlay).getByText("-1:40:00")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Seek back 10 seconds" })).not.toBeInTheDocument();

    const subtitles = screen.getByRole("button", { name: "Subtitles" });
    const audio = screen.getByRole("button", { name: "Audio" });
    fireEvent.click(subtitles);
    expect(screen.getByRole("dialog", { name: "Subtitles" })).toBeInTheDocument();

    fireEvent.click(audio);
    expect(audio).toHaveAttribute("aria-expanded", "true");
    expect(subtitles).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByRole("dialog", { name: "Audio" })).toBeInTheDocument();
    await waitFor(() => expect(screen.getAllByRole("dialog")).toHaveLength(1));
    expect(within(overlay).getByRole("button", { name: "Subtitles" })).toBeVisible();

    fireEvent.click(subtitles);
    const off = within(screen.getByRole("dialog", { name: "Subtitles" })).getByRole("radio", { name: "Off" });
    off.focus();
    fireEvent.keyDown(off, { key: "Escape" });
    expect(subtitles).toHaveAttribute("aria-expanded", "false");
    expect(subtitles).toHaveFocus();
  });

  it("opens Sources from the error state while the controls are hidden", () => {
    const runtime = readyRuntime({ status: "error", error: "This source could not be played." });
    render(<PlayerDialog runtime={runtime} snapshot={runtime.getSnapshot()} onClose={vi.fn()} />);
    expect(screen.queryByTestId("player-controls-overlay")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Back to sources" }));
    expect(screen.getByRole("dialog", { name: "Sources" })).toBeInTheDocument();
  });

  it("syncs native fullscreen state at the desktop shell root", () => {
    let fullscreenListener: ((fullscreen: boolean) => void) | null = null;
    const unsubscribe = vi.fn();
    window.panoramaDesktop = {
      getCapabilities: vi.fn(),
      openExternal: vi.fn(),
      onFullscreenChange(listener: (fullscreen: boolean) => void) {
        fullscreenListener = listener;
        return unsubscribe;
      },
    } as never;
    const view = render(<RuntimeProvider runtime={new FakeRuntime()}><div /></RuntimeProvider>);

    act(() => fullscreenListener?.(true));
    expect(document.body).toHaveClass("panorama-desktop-fullscreen");
    act(() => fullscreenListener?.(false));
    expect(document.body).not.toHaveClass("panorama-desktop-fullscreen");

    view.unmount();
    expect(unsubscribe).toHaveBeenCalledOnce();
  });

  it("focuses playback so Space and arrow shortcuts control the film", async () => {
    const runtime = new FakeRuntime();
    const current = runtime.getSnapshot();
    runtime.setStateForTest({
      player: {
        ...current.player,
        status: "ready",
        stage: null,
        paused: false,
        time: 120,
        duration: 6120,
      },
    });
    const pause = vi.spyOn(runtime, "setPlaybackPaused");
    const seek = vi.spyOn(runtime, "seekPlayback");

    const view = render(<PlayerDialog runtime={runtime} snapshot={runtime.getSnapshot()} onClose={vi.fn()} />);
    const video = view.container.querySelector<HTMLElement>(".player-video");
    if (!video) throw new Error("Missing player video surface");

    await waitFor(() => expect(video).toHaveFocus());
    fireEvent.keyDown(video, { key: " ", code: "Space" });
    fireEvent.keyDown(video, { key: "ArrowLeft", code: "ArrowLeft" });
    fireEvent.keyDown(video, { key: "ArrowRight", code: "ArrowRight" });

    expect(pause).toHaveBeenCalledWith(true);
    expect(seek).toHaveBeenNthCalledWith(1, 110);
    expect(seek).toHaveBeenNthCalledWith(2, 130);

    pause.mockClear();
    seek.mockClear();
    video.blur();
    fireEvent.keyDown(window, { key: " ", code: "Space" });
    fireEvent.keyDown(window, { key: "ArrowLeft", code: "ArrowLeft" });
    fireEvent.keyDown(window, { key: "ArrowRight", code: "ArrowRight" });

    expect(pause).toHaveBeenCalledWith(true);
    expect(seek).toHaveBeenNthCalledWith(1, 110);
    expect(seek).toHaveBeenNthCalledWith(2, 130);

    pause.mockClear();
    seek.mockClear();
    const close = screen.getByRole("button", { name: "Close" });
    close.focus();
    fireEvent.keyDown(close, { key: " ", code: "Space" });
    fireEvent.keyDown(close, { key: "ArrowLeft", code: "ArrowLeft" });
    fireEvent.keyDown(close, { key: "ArrowRight", code: "ArrowRight" });

    expect(pause).toHaveBeenCalledWith(true);
    expect(seek).toHaveBeenNthCalledWith(1, 110);
    expect(seek).toHaveBeenNthCalledWith(2, 130);
  });

  it("never masks native video with a black layer on window blur or buffering", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    const setVideoSurface = vi.fn();
    window.panoramaDesktop = {
      getCapabilities: vi.fn(),
      openExternal: vi.fn(),
      mpv: { send: vi.fn(), on: vi.fn(() => vi.fn()), setVideoSurface },
    };
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 0,
      left: 0,
      top: 0,
      right: 1280,
      bottom: 720,
      width: 1280,
      height: 720,
      toJSON: () => ({}),
    });
    const runtime = new FakeRuntime();
    const current = runtime.getSnapshot();
    runtime.setStateForTest({
      player: {
        ...current.player,
        status: "ready",
        stage: null,
        buffering: false,
      },
    });

    const view = render(<PlayerDialog runtime={runtime} snapshot={runtime.getSnapshot()} onClose={vi.fn()} />);

    expect(screen.queryByTestId("native-video-fallback")).not.toBeInTheDocument();
    fireEvent.blur(window);
    expect(screen.queryByTestId("native-video-fallback")).not.toBeInTheDocument();
    expect(setVideoSurface).not.toHaveBeenLastCalledWith(expect.objectContaining({ visible: false }));

    runtime.setStateForTest({
      player: {
        ...runtime.getSnapshot().player,
        status: "buffering",
        buffering: true,
      },
    });
    view.rerender(<PlayerDialog runtime={runtime} snapshot={runtime.getSnapshot()} onClose={vi.fn()} />);
    expect(screen.queryByTestId("native-video-fallback")).not.toBeInTheDocument();
  });

  it("commits each accessible slider change once without duplicate release or blur seeks", async () => {
    const runtime = new FakeRuntime();
    const current = runtime.getSnapshot();
    runtime.setStateForTest({
      player: {
        ...current.player,
        status: "ready",
        stage: null,
        title: "Aftersun",
        time: 1671,
        duration: 6120,
      },
    });
    const seek = vi.spyOn(runtime, "seekPlayback");

    render(<PlayerDialog runtime={runtime} snapshot={runtime.getSnapshot()} onClose={vi.fn()} />);

    const progress = await screen.findByRole("slider", { name: "Playback position" });
    expect(progress).toHaveAttribute("max", "6120");
    expect(progress).toHaveValue("1671");
    expect(screen.getByText("27:51")).toBeVisible();
    expect(screen.getByText("-1:14:09")).toBeVisible();
    expect(Number(progress.closest<HTMLElement>(".player-range")!.style.getPropertyValue("--played"))).toBeCloseTo(1671 / 6120);
    expect(screen.queryByTestId("player-buffered-range")).not.toBeInTheDocument();

    fireEvent.change(progress, { target: { value: "2520" } });
    expect(seek).toHaveBeenCalledTimes(1);
    expect(seek).toHaveBeenCalledWith(2520);
    fireEvent.pointerUp(progress);
    fireEvent.blur(progress);
    expect(seek).toHaveBeenCalledTimes(1);

    fireEvent.change(progress, { target: { value: "3000" } });
    fireEvent.keyUp(progress, { key: "ArrowRight" });
    expect(seek).toHaveBeenCalledTimes(2);
    expect(seek).toHaveBeenNthCalledWith(2, 3000);

  });

  it("disables a genuinely unknown duration instead of exposing a fake seek range", async () => {
    const runtime = new FakeRuntime();
    const current = runtime.getSnapshot();
    runtime.setStateForTest({
      player: {
        ...current.player,
        status: "ready",
        stage: null,
        time: 18,
        duration: 0,
      },
    });

    render(<PlayerDialog runtime={runtime} snapshot={runtime.getSnapshot()} onClose={vi.fn()} />);

    expect(await screen.findByRole("slider", { name: "Playback position" })).toBeDisabled();
    expect(screen.getByText("0:18")).toBeVisible();
    expect(screen.getByText("--:--")).toBeVisible();
  });

  it("places the 100% default at the midpoint of a 0-200 control", async () => {
    const runtime = new FakeRuntime();
    const current = runtime.getSnapshot();
    runtime.setStateForTest({
      player: {
        ...current.player,
        status: "ready",
        stage: null,
        duration: 6120,
        volume: 1,
        muted: false,
      },
    });
    const setVolume = vi.spyOn(runtime, "setPlaybackVolume");

    render(<PlayerDialog runtime={runtime} snapshot={runtime.getSnapshot()} onClose={vi.fn()} />);

    const volume = await screen.findByRole("slider", { name: "Volume" });
    expect(volume).toHaveAttribute("max", "200");
    expect(volume).toHaveValue("100");
    expect(volume).toHaveAttribute("aria-valuetext", "100%");
    expect(volume.closest<HTMLElement>(".player-range")!.style.getPropertyValue("--played")).toBe("0.5");

    fireEvent.change(volume, { target: { value: "165" } });
    expect(setVolume).toHaveBeenCalledWith(1.65);
  });

  it("uses native window fullscreen so the MPV surface stays attached", async () => {
    const setFullscreen = vi.fn(async () => undefined);
    let fullscreenListener: ((fullscreen: boolean) => void) | null = null;
    window.panoramaDesktop = {
      getCapabilities: vi.fn(),
      openExternal: vi.fn(),
      setFullscreen,
      onFullscreenChange(listener: (fullscreen: boolean) => void) {
        fullscreenListener = listener;
        return vi.fn();
      },
      mpv: { send: vi.fn(), on: vi.fn(() => vi.fn()), setVideoSurface: vi.fn() },
    } as never;
    const runtime = new FakeRuntime();
    const current = runtime.getSnapshot();
    runtime.setStateForTest({
      player: {
        ...current.player,
        status: "ready",
        stage: null,
        title: "Aftersun",
        trackDiscoveryReady: true,
        audio: { status: "ready", tracks: [], selectedId: null, error: null },
        subtitles: { ...current.player.subtitles, status: "ready" },
      },
    });
    const view = render(<PlayerDialog runtime={runtime} snapshot={runtime.getSnapshot()} onClose={vi.fn()} />);
    const playerSurface = screen.getByTestId("player-surface");
    const requestFullscreen = vi.fn(async () => undefined);
    playerSurface.requestFullscreen = requestFullscreen;

    fireEvent.click(screen.getByRole("button", { name: "Fullscreen" }));

    expect(setFullscreen).toHaveBeenCalledWith(true);
    expect(requestFullscreen).not.toHaveBeenCalled();
    act(() => fullscreenListener?.(true));
    expect(screen.getByRole("button", { name: "Exit fullscreen" })).toBeVisible();
    expect(document.body).toHaveClass("panorama-desktop-fullscreen");
    view.unmount();
    expect(setFullscreen).toHaveBeenLastCalledWith(false);
    expect(document.body).not.toHaveClass("panorama-desktop-fullscreen");
  });

  it("shows the native surface while preparing so MPV can render the readiness frame", async () => {
    const setVideoSurface = vi.fn();
    window.panoramaDesktop = {
      getCapabilities: vi.fn(),
      openExternal: vi.fn(),
      mpv: { send: vi.fn(), on: vi.fn(() => vi.fn()), setVideoSurface },
    };
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 0,
      left: 0,
      top: 0,
      right: 1280,
      bottom: 720,
      width: 1280,
      height: 720,
      toJSON: () => ({}),
    });
    const runtime = new FakeRuntime();
    const current = runtime.getSnapshot();
    runtime.setStateForTest({
      player: {
        ...current.player,
        status: "preparing",
        stage: "loadingVideo",
      },
    });

    render(<PlayerDialog runtime={runtime} snapshot={runtime.getSnapshot()} onClose={vi.fn()} />);

    await waitFor(() => expect(setVideoSurface).toHaveBeenCalledWith(expect.objectContaining({
      visible: true,
      width: 1280,
      height: 720,
    })));
  });

  it("shows the bounded surface only for ready playback and hides it before unmount", async () => {
    const setVideoSurface = vi.fn();
    window.panoramaDesktop = {
      getCapabilities: vi.fn(),
      openExternal: vi.fn(),
      mpv: { send: vi.fn(), on: vi.fn(() => vi.fn()), setVideoSurface },
    };
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 0,
      left: 0,
      top: 0,
      right: 1280,
      bottom: 720,
      width: 1280,
      height: 720,
      toJSON: () => ({}),
    });
    const runtime = new FakeRuntime();
    const current = runtime.getSnapshot();
    runtime.setStateForTest({
      player: {
        ...current.player,
        status: "ready",
        stage: null,
        title: "Aftersun",
        trackDiscoveryReady: true,
        audio: { status: "ready", tracks: [], selectedId: null, error: null },
        subtitles: { ...current.player.subtitles, status: "ready" },
      },
    });

    const view = render(<PlayerDialog runtime={runtime} snapshot={runtime.getSnapshot()} onClose={vi.fn()} />);
    await waitFor(() => expect(setVideoSurface).toHaveBeenCalledWith(expect.objectContaining({
      visible: true,
      width: 1280,
      height: 720,
    })));

    view.unmount();
    expect(setVideoSurface).toHaveBeenLastCalledWith(expect.objectContaining({ visible: false }));
  });
});
