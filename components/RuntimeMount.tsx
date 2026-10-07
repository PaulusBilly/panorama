"use client";

import { createContext, useContext, useEffect, useState, useSyncExternalStore } from "react";
import { domAnimation, LazyMotion, MotionConfig } from "motion/react";
import { initialRuntimeSnapshot } from "@/runtime/snapshot";
import { createRuntime } from "@/runtime/create-runtime";
import type { RuntimeSnapshot, StremioRuntime } from "@/runtime/types";
import { discordArtwork } from "@/desktop/shared/discord-presence";

const RuntimeContext = createContext<StremioRuntime | null>(null);
const noopSubscribe = () => () => undefined;

export function usePanoramaRuntime(): StremioRuntime | null {
  return useContext(RuntimeContext);
}

export function usePanoramaSnapshot(): RuntimeSnapshot {
  const runtime = usePanoramaRuntime();
  return useSyncExternalStore(
    runtime?.subscribe ?? noopSubscribe,
    runtime?.getSnapshot ?? (() => initialRuntimeSnapshot),
    () => initialRuntimeSnapshot,
  );
}

export function RuntimeProvider({ children, runtime: providedRuntime }: { children: React.ReactNode; runtime?: StremioRuntime }) {
  const [runtime, setRuntime] = useState<StremioRuntime | null>(providedRuntime ?? null);

  useEffect(() => {
    const update = window.panoramaDesktop?.updateDiscordPlayback;
    if (!runtime || !update) return;
    let previous = "";
    const publish = () => {
      const { player, details } = runtime.getSnapshot();
      const film = details.metadata.item;
      const playback = player.filmId && player.title && ["ready", "buffering"].includes(player.status) ? {
        filmId: player.filmId,
        title: player.title.slice(0, 512),
        year: film?.id === player.filmId ? film.year : null,
        directorTmdbId: film?.id === player.filmId ? film.directorTmdbId ?? null : null,
        director: film?.id === player.filmId ? film.director?.trim().slice(0, 512) || null : null,
        artwork: film?.id === player.filmId ? discordArtwork(film.landscapeUrl, film.posterUrl) : null,
        time: player.time,
        duration: player.duration,
        state: player.paused ? "paused" as const : player.buffering || player.status === "buffering" ? "buffering" as const : "playing" as const,
      } : null;
      const signature = JSON.stringify(playback);
      if (signature !== previous) { previous = signature; update(playback); }
    };
    const clear = () => update(null);
    const unsubscribe = runtime.subscribe(publish);
    publish();
    window.addEventListener("pagehide", clear);
    return () => { unsubscribe(); window.removeEventListener("pagehide", clear); clear(); };
  }, [runtime]);

  useEffect(() => {
    const desktop = window.panoramaDesktop;
    if (!desktop?.onFullscreenChange) return;
    const unsubscribe = desktop.onFullscreenChange((fullscreen) => {
      document.body.classList.toggle("panorama-desktop-fullscreen", fullscreen);
    });
    return () => {
      unsubscribe();
      document.body.classList.remove("panorama-desktop-fullscreen");
    };
  }, []);

  useEffect(() => {
    if (providedRuntime) return;
    let active = true;
    let current: StremioRuntime | null = null;
    const flushProgress = () => current?.flushPlaybackProgress();
    window.addEventListener("pagehide", flushProgress);

    void createRuntime().then((created) => {
      if (!active) {
        created.destroy();
        return;
      }
      current = created;
      setRuntime(created);
      void created.initialize();
    });

    return () => {
      active = false;
      window.removeEventListener("pagehide", flushProgress);
      current?.destroy();
    };
  }, [providedRuntime]);

  return (
    <LazyMotion features={domAnimation} strict>
      <MotionConfig reducedMotion="never">
        <RuntimeContext.Provider value={runtime}>{children}</RuntimeContext.Provider>
      </MotionConfig>
    </LazyMotion>
  );
}
