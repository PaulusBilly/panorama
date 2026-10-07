"use client";

import { Slider } from "@base-ui/react/slider";
import { Button } from "@base-ui/react/button";

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  IconBadgeCc,
  IconBadgeCcFilled,
  IconListDetails,
  IconListDetailsFilled,
  IconLoader2,
  IconMaximize,
  IconMinimize,
  IconPlayerPauseFilled,
  IconPlayerPlayFilled,
  IconPlaylist,
  IconPlaylistFilled,
  IconVolume2,
  IconVolumeOff,
  IconX,
} from "@tabler/icons-react";
import type { RuntimeSnapshot, StremioRuntime } from "@/runtime/types";
import { bindNativeVideoSurface, type NativeVideoSurfaceBinding } from "@/runtime/native-video-surface";
import { CascadeLoader } from "./CascadeLoader";
import {
  PlayerPanelProvider,
  PlayerPanelSurface,
  PlayerPopoverTrigger,
  PlayerTrackPopover,
  usePlayerPanels,
  type PlayerPopoverHandle,
} from "./PlayerTrackPopover";
import { PlayerSourcesPopover, playerSourcesPopoverId } from "./PlayerSourcesPopover";
import { PlayerSettingsPopover } from "./PlayerSettingsPopover";
import { PlayerSubtitlesPopover } from "./PlayerSubtitlesPopover";
import { formatMbps, formatWait, usePlaybackHealth } from "./usePlaybackHealth";

type Props = {
  runtime: StremioRuntime;
  snapshot: RuntimeSnapshot;
  onClose(): void;
};

function formatTime(value: number): string {
  if (!Number.isFinite(value) || value < 0) return "0:00";
  const seconds = Math.floor(value % 60).toString().padStart(2, "0");
  const minutes = Math.floor(value / 60) % 60;
  const hours = Math.floor(value / 3600);
  return hours > 0 ? `${hours}:${minutes.toString().padStart(2, "0")}:${seconds}` : `${minutes}:${seconds}`;
}

const playerIconProps = { size: 20, stroke: 1.8 } as const;
const playerIconButtonClass = "focus-ring grid size-9 place-items-center rounded-full border-0 p-0 transition-[background-color,scale] duration-[120ms] ease-out hover:bg-player-ink/10 active:scale-97";
// Solid, unblurred chrome surface shared by the control pills.
const playerPillClass = "rounded-full bg-player-canvas/72 inset-ring inset-ring-player-ink/12";
const controlsHideDelayMs = 3000;

export default function PlayerDialog({ runtime, snapshot, onClose }: Props) {
  const playerSurfaceRef = useRef<HTMLDivElement>(null);
  const videoRef = useRef<HTMLDivElement>(null);
  const nativeSurfaceRef = useRef<NativeVideoSurfaceBinding | null>(null);
  const inactivityRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const keyboardInteractionRef = useRef(false);
  const scrubPreviewRef = useRef<number | null>(null);
  const [controlsVisible, setControlsVisible] = useState(true);
  const [controlsFocused, setControlsFocused] = useState(false);
  const [scrubPreview, setScrubPreview] = useState<number | null>(null);
  const [desktopFullscreen, setDesktopFullscreen] = useState(false);
  const sourcesPopoverRef = useRef<PlayerPopoverHandle>(null);
  const panelPillRef = useRef<HTMLDivElement>(null);
  const seekPillRef = useRef<HTMLDivElement>(null);
  const panelOpenAtPressRef = useRef(false);
  // The shared panel sits 8px above the icon pill (or the seek pill when the chrome is hidden),
  // right-aligned with it, and scales from the centre of the trigger that opened it.
  const panels = usePlayerPanels((id) => {
    const surface = playerSurfaceRef.current?.getBoundingClientRect();
    if (!surface) return null;
    const pill = panelPillRef.current?.getBoundingClientRect();
    const anchorTop = pill?.height ? pill.top : seekPillRef.current?.getBoundingClientRect().top;
    const trigger = document.querySelector(`[aria-controls="${id}"]`)?.getBoundingClientRect();
    return {
      bottom: anchorTop ? surface.bottom - anchorTop + 8 : 96,
      originRight: pill?.width && trigger?.width ? pill.right - (trigger.left + trigger.width / 2) : 20,
    };
  });
  const player = snapshot.player;
  const nativePlaybackAvailable = typeof window !== "undefined" && Boolean(window.panoramaDesktop?.mpv);
  const playableStatus = player.status === "ready" || player.status === "buffering" || player.status === "ended";
  const [readiness, setReadiness] = useState(() => ({ status: player.status, hasReachedPlayable: playableStatus }));
  const hasReachedPlayable = readiness.hasReachedPlayable || playableStatus;
  if (readiness.status !== player.status) {
    setReadiness({ status: player.status, hasReachedPlayable });
  }
  const initialPreparing = !hasReachedPlayable && player.status === "preparing";
  const preparationMessage =
    player.stage === "checkingService"
      ? "Connecting to Stremio Service"
      : player.stage === "loadingVideo"
        ? "Loading video"
        : "Resolving source";
  const overlayOpen = panels.activeId !== null;
  const controlsReady =
    player.status === "ready" || player.status === "buffering";
  const interactionChromeShown = controlsVisible || controlsFocused || overlayOpen || player.buffering || player.paused;
  const controlsShown =
    controlsReady && interactionChromeShown;
  const durationKnown = Number.isFinite(player.duration) && player.duration > 0;
  const displayedTime = scrubPreview ?? player.time;
  const volumePercent = Math.round(Math.min(2, Math.max(0, player.muted ? 0 : player.volume)) * 100);
  const chromeShown = !initialPreparing && (player.status !== "ready" || interactionChromeShown);
  const pointerShown = initialPreparing || chromeShown;
  const health = usePlaybackHealth(nativePlaybackAvailable && hasReachedPlayable, player.sourceId);
  const [dismissedHeavySourceId, setDismissedHeavySourceId] = useState<string | null>(null);
  const heavySourceNoticeShown = health.tooHeavy && health.sourceMbps !== null && health.downloadMbps !== null
    && dismissedHeavySourceId !== player.sourceId && (player.status === "ready" || player.status === "buffering");
  const durationFraction = (value: number) => Math.min(1, Math.max(0, value / player.duration));
  const playedFraction = durationKnown ? durationFraction(displayedTime) : 0;
  const bufferedFraction = durationKnown && health.bufferedUntil !== null ? durationFraction(health.bufferedUntil) : 0;
  const desktopFullscreenAvailable = typeof window !== "undefined"
    && Boolean(window.panoramaDesktop?.setFullscreen && window.panoramaDesktop.onFullscreenChange);

  useLayoutEffect(() => {
    if (videoRef.current) {
      nativeSurfaceRef.current = bindNativeVideoSurface(videoRef.current);
      void runtime.attachPlayer(videoRef.current);
    }
    return () => {
      nativeSurfaceRef.current?.destroy();
      nativeSurfaceRef.current = null;
      runtime.detachPlayer();
    };
  }, [runtime]);

  useEffect(() => {
    if (!initialPreparing) videoRef.current?.focus();
  }, [initialPreparing]);

  useEffect(() => {
    nativeSurfaceRef.current?.setVisible(
      player.status === "preparing" || player.status === "ready" || player.status === "buffering" || player.status === "ended",
    );
  }, [player.status]);

  useEffect(() => {
    const desktop = window.panoramaDesktop;
    if (!desktop?.setFullscreen || !desktop.onFullscreenChange) return;
    const unsubscribe = desktop.onFullscreenChange((fullscreen) => {
      setDesktopFullscreen(fullscreen);
      document.body.classList.toggle("panorama-desktop-fullscreen", fullscreen);
    });
    return () => {
      unsubscribe();
      document.body.classList.remove("panorama-desktop-fullscreen");
      void desktop.setFullscreen?.(false).catch(() => undefined);
    };
  }, []);

  const showControls = (ignoreFocusedControl = false) => {
    setControlsVisible(true);
    if (inactivityRef.current) clearTimeout(inactivityRef.current);
    if ((ignoreFocusedControl || !controlsFocused) && !overlayOpen && !player.buffering && !player.paused && player.status === "ready") {
      inactivityRef.current = setTimeout(() => setControlsVisible(false), controlsHideDelayMs);
    }
  };

  const showPointerControls = () => {
    keyboardInteractionRef.current = false;
    setControlsFocused(false);
    showControls(true);
  };

  useEffect(() => {
    if (inactivityRef.current) clearTimeout(inactivityRef.current);
    if (!controlsFocused && !overlayOpen && !player.buffering && !player.paused && player.status === "ready") {
      inactivityRef.current = setTimeout(() => setControlsVisible(false), controlsHideDelayMs);
    }
    return () => {
      if (inactivityRef.current) clearTimeout(inactivityRef.current);
    };
  }, [controlsFocused, overlayOpen, player.buffering, player.paused, player.status]);

  const close = () => {
    void runtime.stopPlayback().finally(() => {
      onClose();
    });
  };

  const togglePaused = () => runtime.setPlaybackPaused(!player.paused);
  const previewSeek = (value: number) => {
    if (!durationKnown) return;
    const next = Math.min(player.duration, Math.max(0, value));
    scrubPreviewRef.current = next;
    setScrubPreview(next);
  };
  const commitSeek = () => {
    const next = scrubPreviewRef.current;
    if (next === null) return;
    scrubPreviewRef.current = null;
    setScrubPreview(null);
    runtime.seekPlayback(next);
  };
  const cancelSeek = () => {
    scrubPreviewRef.current = null;
    setScrubPreview(null);
  };
  const setFullscreen = (fullscreen: boolean) => {
    const playerSurface = playerSurfaceRef.current;
    if (!playerSurface) return;
    setControlsVisible(true);
    const desktop = window.panoramaDesktop;
    if (desktop?.setFullscreen && desktop.onFullscreenChange) {
      void desktop.setFullscreen(fullscreen).catch(() => undefined);
      return;
    }
    runtime.setPlaybackFullscreen(fullscreen, playerSurface);
  };
  const fullscreen = desktopFullscreenAvailable ? desktopFullscreen : player.fullscreen;

  const handleShortcut = (event: KeyboardEvent) => {
    if (event.defaultPrevented) return;
    keyboardInteractionRef.current = true;
    const target = event.target as HTMLElement;
    if (event.key === "Escape") {
      event.preventDefault();
      if (overlayOpen) panels.close(true);
      else close();
      return;
    }
    if (["INPUT", "SELECT", "TEXTAREA"].includes(target.tagName) || target.closest?.("[role=radio], [role=combobox], [role=checkbox], [role=tab], [data-player-select]")) {
      setControlsFocused(true);
      showControls();
      return;
    }
    const key = event.key.toLowerCase();
    if (key === " " || key === "k") {
      event.preventDefault();
      togglePaused();
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      runtime.seekPlayback(player.time - 10);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      runtime.seekPlayback(player.time + 10);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      runtime.setPlaybackVolume(player.volume + 0.1);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      runtime.setPlaybackVolume(player.volume - 0.1);
    } else if (key === "m") {
      runtime.setPlaybackMuted(!player.muted);
    } else if (key === "f") {
      setFullscreen(!fullscreen);
    }
    showControls();
  };

  useEffect(() => {
    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
  });

  return (
    <PlayerPanelProvider panels={panels}>
    <main
      className="desktop-player-page w-screen overflow-hidden bg-player-canvas text-player-ink"
      aria-labelledby="player-title"
      onFocusCapture={(event) => {
        const target = event.target as HTMLElement;
        setControlsFocused(keyboardInteractionRef.current && target.matches("button, a, input, select, textarea"));
      }}
      onBlurCapture={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setControlsFocused(false);
      }}
    >
      <div
        ref={playerSurfaceRef}
        className={`relative h-full w-full bg-player-canvas fullscreen:h-dvh fullscreen:w-screen ${pointerShown ? "cursor-default" : "cursor-none"}`}
        data-player-surface=""
        data-testid="player-surface"
        onPointerMove={showPointerControls}
        onPointerDown={showPointerControls}
      >
        <div className="absolute inset-0 z-0 overflow-hidden">
          <div
            ref={videoRef}
            className="player-video h-full w-full outline-none [&_video]:absolute [&_video]:inset-0 [&_video]:block [&_video]:h-full [&_video]:w-full [&_video]:object-contain [&_video]:object-center"
            tabIndex={-1}
            onPointerDown={() => {
              panelOpenAtPressRef.current = overlayOpen;
              videoRef.current?.focus();
            }}
            onClick={() => {
              // A press that only dismissed the settings panel must not also pause the film.
              if (panelOpenAtPressRef.current || !controlsReady) return;
              togglePaused();
            }}
          />
        </div>
        <h2 className="sr-only" id="player-title">Playing {player.title ?? "movie"}</h2>

        {!initialPreparing ? (
          <div className={`pointer-events-none absolute inset-x-0 top-0 z-30 flex items-center justify-between gap-3 bg-gradient-to-b from-player-canvas/70 to-transparent [padding-inline:max(20px,env(safe-area-inset-left))] [padding-block-start:max(16px,env(safe-area-inset-top))] pb-12 transition-opacity duration-fast ${chromeShown ? "opacity-100 [&>*]:pointer-events-auto" : "opacity-0"}`}>
            <Button className={`${playerIconButtonClass} size-10 ${playerPillClass}`} type="button" aria-label="Close" title="Close" onClick={close}>
              <IconX aria-hidden="true" {...playerIconProps} />
            </Button>
            {controlsReady ? (
              <div className="flex items-center gap-2">
                <div className={`flex items-center gap-1 p-0.5 ${playerPillClass}`}>
                  <Button className={`${playerIconButtonClass} ${player.muted ? "bg-player-ink/15" : ""}`} type="button" aria-label={player.muted ? "Unmute" : "Mute"} title={player.muted ? "Unmute" : "Mute"} onClick={() => runtime.setPlaybackMuted(!player.muted)}>
                    {player.muted
                      ? <IconVolumeOff aria-hidden="true" {...playerIconProps} />
                      : <IconVolume2 aria-hidden="true" {...playerIconProps} />}
                  </Button>
                  <Slider.Root className="player-range focus-ring mr-2 w-24 max-[540px]:hidden" style={{ ["--played" as string]: volumePercent / 200, ["--buffered" as string]: volumePercent / 200 }} min={0} max={200} step={1} value={volumePercent} thumbAlignment="edge" onValueChange={(value) => runtime.setPlaybackVolume(Number(value) / 100)}>
                    <Slider.Control className="player-range-control"><Slider.Track className="player-range-track" /><Slider.Thumb className="player-range-thumb" getAriaLabel={() => "Volume"} aria-valuetext={`${volumePercent}%`} /></Slider.Control>
                  </Slider.Root>
                </div>
                <Button className={`${playerIconButtonClass} size-10 ${fullscreen ? "rounded-full bg-player-ink/20 inset-ring inset-ring-player-ink/12" : playerPillClass}`} type="button" aria-label={fullscreen ? "Exit fullscreen" : "Fullscreen"} title={fullscreen ? "Exit fullscreen" : "Fullscreen"} aria-pressed={fullscreen} onClick={() => setFullscreen(!fullscreen)}>
                  {fullscreen
                    ? <IconMinimize aria-hidden="true" {...playerIconProps} />
                    : <IconMaximize aria-hidden="true" {...playerIconProps} />}
                </Button>
              </div>
            ) : null}
          </div>
        ) : null}

        {initialPreparing ? (
          <div className="pointer-events-none absolute inset-0 z-40 grid place-items-center bg-player-canvas" role="status" aria-label="Preparing playback" aria-live="polite">
            <CascadeLoader />
          </div>
        ) : null}

        {!initialPreparing && (player.status === "preparing" || player.status === "buffering") ? (
          <div className="pointer-events-none absolute inset-0 z-10 grid place-items-center" role="status" aria-live="polite">
            <p className="type-body flex items-center gap-3 bg-player-canvas/75 px-5 py-3">
              <IconLoader2 aria-hidden="true" className="animate-spin" size={22} stroke={1.8} />
              <span>
                {player.status === "preparing"
                  ? preparationMessage
                  : health.waitSeconds !== null && health.waitSeconds >= 5
                    ? `Buffering · about ${formatWait(health.waitSeconds)}`
                    : "Buffering"}
              </span>
            </p>
          </div>
        ) : null}

        {heavySourceNoticeShown ? (
          <div className="absolute inset-x-0 top-20 z-30 flex justify-center px-5" role="status" aria-live="polite">
            <div className="type-body flex max-w-2xl flex-wrap items-center gap-x-5 gap-y-3 bg-player-canvas/85 px-5 py-4">
              <p className="min-w-0 flex-1 basis-72">
                This source needs about {formatMbps(health.sourceMbps!)} Mbps. The connection is delivering {formatMbps(health.downloadMbps!)} Mbps. Pause to let it download ahead, or choose a lighter source.
              </p>
              <div className="flex gap-3">
                <Button className="focus-ring min-h-11 border border-player-ink bg-player-ink px-5 text-sm text-player-canvas" type="button" onClick={() => sourcesPopoverRef.current?.show()}>Sources</Button>
                <Button className="focus-ring min-h-11 border border-player-ink/60 bg-transparent px-5 text-sm" type="button" onClick={() => setDismissedHeavySourceId(player.sourceId)}>Dismiss</Button>
              </div>
            </div>
          </div>
        ) : null}

        {player.status === "error" ? (
          <div className="absolute inset-0 z-20 grid place-items-center bg-player-canvas/75 px-5">
            <div className="max-w-lg text-center" role="alert">
              <p className="type-heading">{player.error}</p>
              <div className="mt-6 flex flex-wrap justify-center gap-3">
                {snapshot.service.status === "offline" ? (
                  <Button className="focus-ring min-h-11 border border-player-ink bg-player-ink px-5 text-sm text-player-canvas" type="button" onClick={() => void runtime.checkService().then((status) => status === "online" ? runtime.retryPlayback() : undefined)}>Check service</Button>
                ) : (
                  <Button className="focus-ring min-h-11 border border-player-ink bg-player-ink px-5 text-sm text-player-canvas" type="button" onClick={() => void runtime.retryPlayback()}>Retry playback</Button>
                )}
                <Button className="focus-ring min-h-11 border border-player-ink/60 bg-transparent px-5 text-sm" type="button" onClick={() => sourcesPopoverRef.current?.show()}>Back to sources</Button>
              </div>
            </div>
          </div>
        ) : null}

        {player.status === "ended" ? (
          <div className="absolute inset-0 z-20 grid place-items-center bg-player-canvas/70 px-5">
            <div className="text-center">
              <p className="type-title">Playback ended</p>
              <div className="mt-6 flex gap-3">
                <Button className="focus-ring min-h-11 border border-player-ink bg-player-ink px-5 text-sm text-player-canvas" type="button" onClick={() => { runtime.seekPlayback(0); runtime.setPlaybackPaused(false); }}>Replay</Button>
                <Button className="focus-ring min-h-11 border border-player-ink/60 bg-transparent px-5 text-sm" type="button" onClick={close}>Back to details</Button>
              </div>
            </div>
          </div>
        ) : null}

        {!initialPreparing && controlsShown ? <div data-testid="player-controls-overlay" className="pointer-events-none absolute inset-x-0 bottom-0 z-30 bg-gradient-to-t from-player-canvas/80 to-transparent [padding-inline:max(20px,env(safe-area-inset-left))] [padding-block-end:max(18px,env(safe-area-inset-bottom))] pt-20" onFocusCapture={() => setControlsVisible(true)}>
          <div className="flex items-center justify-between gap-6">
            <p className="type-title min-w-0 truncate font-bold">{player.title}</p>
            <div ref={panelPillRef} className={`pointer-events-auto flex shrink-0 items-center gap-0.5 p-0.5 ${playerPillClass}`}>
              <PlayerSubtitlesPopover
                id="player-subtitles"
                triggerLabel="Subtitles"
                triggerIcon={player.subtitles.selectedId
                  ? <IconBadgeCcFilled aria-hidden="true" {...playerIconProps} />
                  : <IconBadgeCc aria-hidden="true" {...playerIconProps} />}
                triggerActiveIcon={<IconBadgeCcFilled aria-hidden="true" {...playerIconProps} />}
                tracks={player.subtitles.tracks}
                selectedId={player.subtitles.pendingId ?? player.subtitles.selectedId}
                emptyText="No subtitles"
                error={player.subtitles.error}
                busy={player.subtitles.status === "loading"}
                steppers={[
                  {
                    label: "Text opacity",
                    valueText: `${player.subtitles.style.textOpacity}%`,
                    decreaseLabel: "Decrease text opacity",
                    increaseLabel: "Increase text opacity",
                    onDecrease: () => runtime.setSubtitleStyle({ textOpacity: player.subtitles.style.textOpacity - 1 }),
                    onIncrease: () => runtime.setSubtitleStyle({ textOpacity: player.subtitles.style.textOpacity + 1 }),
                    editable: { value: player.subtitles.style.textOpacity, min: 0, max: 100, suffix: "%", onCommit: (textOpacity: number) => runtime.setSubtitleStyle({ textOpacity }) },
                  },
                  {
                    label: "Delay",
                    valueText: `${player.subtitles.offset.toFixed(1)}s`,
                    decreaseLabel: "Decrease delay",
                    increaseLabel: "Increase delay",
                    onDecrease: () => runtime.setSubtitleOffset(player.subtitles.offset - 0.5),
                    onIncrease: () => runtime.setSubtitleOffset(player.subtitles.offset + 0.5),
                  },
                  {
                    label: "Font size",
                    valueText: `${player.subtitles.style.fontSizePx}px`,
                    decreaseLabel: "Decrease font size",
                    increaseLabel: "Increase font size",
                    onDecrease: () => runtime.setSubtitleStyle({ fontSizePx: player.subtitles.style.fontSizePx - 1 }),
                    onIncrease: () => runtime.setSubtitleStyle({ fontSizePx: player.subtitles.style.fontSizePx + 1 }),
                    editable: {
                      value: player.subtitles.style.fontSizePx,
                      min: 12,
                      max: 96,
                      suffix: "px",
                      onCommit: (fontSizePx: number) => runtime.setSubtitleStyle({ fontSizePx }),
                    },
                  },
                  {
                    label: "Vertical Position",
                    valueText: `${player.subtitles.verticalPosition}%`,
                    decreaseLabel: "Decrease vertical position",
                    increaseLabel: "Increase vertical position",
                    onDecrease: () => runtime.setSubtitleVerticalPosition(player.subtitles.verticalPosition - 5),
                    onIncrease: () => runtime.setSubtitleVerticalPosition(player.subtitles.verticalPosition + 5),
                    editable: {
                      value: player.subtitles.verticalPosition,
                      min: 0,
                      max: 100,
                      suffix: "%",
                      onCommit: (position: number) => runtime.setSubtitleVerticalPosition(position),
                    },
                  },
                  {
                    label: "Horizontal Padding",
                    valueText: `${player.subtitles.style.paddingX}px`,
                    decreaseLabel: "Decrease horizontal padding",
                    increaseLabel: "Increase horizontal padding",
                    onDecrease: () => runtime.setSubtitleStyle({ paddingX: player.subtitles.style.paddingX - 1 }),
                    onIncrease: () => runtime.setSubtitleStyle({ paddingX: player.subtitles.style.paddingX + 1 }),
                    editable: { value: player.subtitles.style.paddingX, min: 0, max: 64, suffix: "px", onCommit: (paddingX: number) => runtime.setSubtitleStyle({ paddingX }) },
                  },
                  {
                    label: "Vertical Padding",
                    valueText: `${player.subtitles.style.paddingY}px`,
                    decreaseLabel: "Decrease vertical padding",
                    increaseLabel: "Increase vertical padding",
                    onDecrease: () => runtime.setSubtitleStyle({ paddingY: player.subtitles.style.paddingY - 1 }),
                    onIncrease: () => runtime.setSubtitleStyle({ paddingY: player.subtitles.style.paddingY + 1 }),
                    editable: { value: player.subtitles.style.paddingY, min: 0, max: 64, suffix: "px", onCommit: (paddingY: number) => runtime.setSubtitleStyle({ paddingY }) },
                  },
                  {
                    label: "Line height",
                    valueText: player.subtitles.style.lineHeight.toFixed(2),
                    decreaseLabel: "Decrease line height",
                    increaseLabel: "Increase line height",
                    onDecrease: () => runtime.setSubtitleStyle({ lineHeight: player.subtitles.style.lineHeight - 0.05 }),
                    onIncrease: () => runtime.setSubtitleStyle({ lineHeight: player.subtitles.style.lineHeight + 0.05 }),
                    editable: { value: player.subtitles.style.lineHeight, min: 1, max: 2, step: 0.05, suffix: "", onCommit: (lineHeight: number) => runtime.setSubtitleStyle({ lineHeight }) },
                  },
                  {
                    label: "Box opacity",
                    valueText: `${player.subtitles.style.backgroundOpacity}%`,
                    decreaseLabel: "Decrease box opacity",
                    increaseLabel: "Increase box opacity",
                    onDecrease: () => runtime.setSubtitleStyle({ backgroundOpacity: player.subtitles.style.backgroundOpacity - 1 }),
                    onIncrease: () => runtime.setSubtitleStyle({ backgroundOpacity: player.subtitles.style.backgroundOpacity + 1 }),
                    editable: { value: player.subtitles.style.backgroundOpacity, min: 0, max: 100, suffix: "%", onCommit: (backgroundOpacity: number) => runtime.setSubtitleStyle({ backgroundOpacity }) },
                  },
                  {
                    label: "Box rounding",
                    valueText: `${player.subtitles.style.borderRadius}px`,
                    decreaseLabel: "Decrease box rounding",
                    increaseLabel: "Increase box rounding",
                    onDecrease: () => runtime.setSubtitleStyle({ borderRadius: player.subtitles.style.borderRadius - 1 }),
                    onIncrease: () => runtime.setSubtitleStyle({ borderRadius: player.subtitles.style.borderRadius + 1 }),
                    editable: { value: player.subtitles.style.borderRadius, min: 0, max: 32, suffix: "px", onCommit: (borderRadius: number) => runtime.setSubtitleStyle({ borderRadius }) },
                  },
                ]}
                appearance={player.subtitles.style}
                onTextColorChange={(textColor) => runtime.setSubtitleStyle({ textColor })}
                appearanceLimitation={player.subtitles.appearanceLimitation}
                fontWeight={player.subtitles.style.fontWeight}
                onFontWeightChange={(fontWeight) => runtime.setSubtitleStyle({ fontWeight })}
                onSelect={(trackId) => runtime.selectSubtitle(trackId)}
              />
              <PlayerTrackPopover
                id="player-audio-tracks"
                title="Audio"
                triggerLabel="Audio"
                triggerIcon={<IconPlaylist aria-hidden="true" {...playerIconProps} />}
                triggerActiveIcon={<IconPlaylistFilled aria-hidden="true" {...playerIconProps} />}
                name="player-audio-track"
                items={player.audio.tracks.map((track) => ({
                  id: track.id,
                  label: track.label,
                  description: track.description,
                }))}
                selectedId={player.audio.selectedId}
                emptyText="No audio tracks"
                error={player.audio.error}
                onSelect={(trackId) => {
                  if (trackId) runtime.selectAudioTrack(trackId);
                }}
              />
              <PlayerPopoverTrigger
                id={playerSourcesPopoverId}
                label="Sources"
                icon={<IconListDetails aria-hidden="true" {...playerIconProps} />}
                activeIcon={<IconListDetailsFilled aria-hidden="true" {...playerIconProps} />}
              />
              {nativePlaybackAvailable && window.panoramaDesktop?.getPlaybackSettings ? <PlayerSettingsPopover /> : null}
            </div>
          </div>
          <div ref={seekPillRef} className={`pointer-events-auto mt-3 flex h-9 items-center gap-3 px-4 ${playerPillClass}`}>
            <Button className="focus-ring sr-only rounded-full focus-visible:not-sr-only focus-visible:grid focus-visible:size-7 focus-visible:shrink-0 focus-visible:place-items-center" type="button" onClick={togglePaused} aria-label={player.paused ? "Play" : "Pause"}>
              {player.paused
                ? <IconPlayerPlayFilled aria-hidden="true" size={16} />
                : <IconPlayerPauseFilled aria-hidden="true" size={16} />}
            </Button>
            <span className="type-caption shrink-0 font-medium tabular-nums text-player-ink/65">{formatTime(displayedTime)}</span>
            <Slider.Root
              className="player-range player-range--seek focus-ring min-w-0 flex-1"
              style={{ ["--played" as string]: playedFraction, ["--buffered" as string]: Math.max(playedFraction, bufferedFraction) }}
              min={0} max={durationKnown ? player.duration : 1} step={0.1}
              value={durationKnown ? Math.min(displayedTime, player.duration) : 0}
              disabled={!durationKnown} thumbAlignment="edge"
              onValueChange={(value) => previewSeek(Number(value))}
              onValueCommitted={commitSeek}
              onPointerCancel={cancelSeek}
            >
              <Slider.Control className="player-range-control"><Slider.Track className="player-range-track" /><Slider.Thumb className="player-range-thumb" getAriaLabel={() => "Playback position"} aria-valuetext={durationKnown ? formatTime(displayedTime) : "Duration unavailable"} onBlur={commitSeek} /></Slider.Control>
            </Slider.Root>
            <span className="type-caption shrink-0 font-medium tabular-nums text-player-ink/65">
              {durationKnown ? `-${formatTime(Math.max(0, player.duration - displayedTime))}` : "--:--"}
            </span>
          </div>
        </div> : null}

        <PlayerSourcesPopover
          ref={sourcesPopoverRef}
          groups={snapshot.details.sources.groups}
          currentSourceId={snapshot.player.sourceId}
          serviceOnline={snapshot.service.status === "online"}
          onSelect={(sourceId) => {
            void runtime.switchPlaybackSource(sourceId);
            sourcesPopoverRef.current?.hide();
          }}
        />

        <PlayerPanelSurface className="right-[max(20px,env(safe-area-inset-right))]" />

        <div className="sr-only-stable" role="status" aria-live="polite">
          {player.status === "preparing" && !initialPreparing ? "Preparing playback." : null}
          {player.buffering ? "Buffering." : null}
          {player.subtitles.pendingId ? "Loading subtitles." : null}
        </div>
      </div>
    </main>
    </PlayerPanelProvider>
  );
}
