"use client";

import { Button } from "@base-ui/react/button";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { IconArrowLeft, IconLoader2, IconPlayerPlayFilled } from "@tabler/icons-react";
import { firstPlayableSource } from "@/runtime/normalize";
import { parseTmdbPublicId } from "@/runtime/tmdb";
import { CascadeLoader } from "./CascadeLoader";
import PlayerDialog from "./PlayerDialog";
import { LoginDialog } from "./LoginDialog";
import { usePanoramaRuntime, usePanoramaSnapshot } from "./RuntimeMount";

export function FilmWatchPage() {
  const runtime = usePanoramaRuntime();
  const snapshot = usePanoramaSnapshot();
  const router = useRouter();
  const [loginOpen, setLoginOpen] = useState(false);
  const startRequestedRef = useRef(false);
  const defaultSource = firstPlayableSource(snapshot.details.sources.groups);
  const tmdbId = snapshot.details.filmId ? parseTmdbPublicId(snapshot.details.filmId) : null;
  const playerActive = snapshot.player.status !== "idle";

  useEffect(() => () => {
    // Stopping cancels any start, so let a remount (Strict Mode, cached route) start again.
    startRequestedRef.current = false;
    runtime?.flushPlaybackProgress();
    void runtime?.stopPlayback();
  }, [runtime]);

  useEffect(() => {
    if (
      startRequestedRef.current ||
      !runtime ||
      !defaultSource ||
      snapshot.account.status !== "signedIn" ||
      snapshot.service.status !== "online" ||
      snapshot.player.status !== "idle"
    ) {
      return;
    }
    startRequestedRef.current = true;
    const mode = new URLSearchParams(window.location.search).has("resume") ? "resume" : "restart";
    void runtime.startPlayback(defaultSource.id, mode);
  }, [
    defaultSource,
    runtime,
    snapshot.account.status,
    snapshot.player.status,
    snapshot.service.status,
  ]);

  const backToDetails = () => {
    if (tmdbId) router.replace(`/films/${tmdbId}`);
    else router.replace("/");
  };

  if (runtime && playerActive) {
    return <PlayerDialog runtime={runtime} snapshot={snapshot} onClose={backToDetails} />;
  }

  const loading = snapshot.details.metadata.status === "loading" || snapshot.details.sources.status === "loading";
  const waitingForFilm = snapshot.details.metadata.status === "idle" || snapshot.details.sources.status === "idle";
  const eligibleStartup = Boolean(runtime && snapshot.account.status === "signedIn" && snapshot.service.status === "online" && (waitingForFilm || loading || defaultSource));

  if (eligibleStartup) {
    return (
      <main className="grid min-h-dvh place-items-center bg-player-canvas" aria-busy="true">
        <div role="status" aria-label="Preparing playback">
          <CascadeLoader />
        </div>
      </main>
    );
  }

  const label = snapshot.account.status !== "signedIn"
    ? "Sign in to watch"
    : loading
      ? "Loading sources"
      : !defaultSource
        ? "No playable source"
        : snapshot.service.status !== "online"
          ? "Check service"
          : "Play movie";

  return (
    <main className="relative grid min-h-dvh place-items-center bg-player-canvas px-5 text-player-ink">
      <Button className="focus-ring absolute left-5 top-5 grid min-h-11 min-w-11 place-items-center border-0 bg-transparent" type="button" aria-label="Back to film details" onClick={backToDetails}>
        <IconArrowLeft aria-hidden="true" size={24} stroke={1.7} />
      </Button>
      <div className="max-w-lg text-center">
        <h1 className="type-title">{snapshot.details.metadata.item?.name ?? "Preparing film"}</h1>
        <Button
          className="focus-ring mx-auto mt-8 grid size-20 place-items-center rounded-full border border-player-ink bg-transparent disabled:cursor-not-allowed disabled:opacity-50"
          type="button"
          disabled={snapshot.account.status === "signedIn" && (loading || !defaultSource)}
          aria-label={label}
          title={label}
          onClick={() => {
            if (snapshot.account.status !== "signedIn") {
              setLoginOpen(true);
              return;
            }
            if (!runtime || !defaultSource) return;
            if (snapshot.service.status !== "online") {
              void runtime.checkService();
              return;
            }
            void runtime.startPlayback(defaultSource.id, "restart");
          }}
        >
          {loading ? <IconLoader2 aria-hidden="true" className="animate-spin" size={28} /> : <IconPlayerPlayFilled aria-hidden="true" size={30} />}
        </Button>
        <p className="type-body mt-4 text-player-ink/80" role="status">{label}</p>
        {snapshot.service.status === "offline" && defaultSource ? (
          <Button className="focus-ring mt-4 min-h-10 border-0 border-b border-current bg-transparent px-1 text-sm" type="button" onClick={() => void runtime?.checkService()}>Check service</Button>
        ) : null}
      </div>
      <LoginDialog
        open={loginOpen}
        account={snapshot.account}
        onClose={() => setLoginOpen(false)}
        onSubmit={(email, password) => runtime?.login(email, password) ?? Promise.resolve()}
      />
    </main>
  );
}
