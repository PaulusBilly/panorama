"use client";

import { Button } from "@base-ui/react/button";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { AnimatePresence, m } from "motion/react";
import { IconArrowLeft, IconCheck, IconLoader2, IconPlayerPlayFilled, IconPlus, IconStarFilled } from "@tabler/icons-react";
import { firstPlayableSource } from "@/runtime/normalize";
import { formatRatingCount, formatTimeLeft, formatTmdbRating } from "@/runtime/film-display";
import { parseTmdbPublicId } from "@/runtime/tmdb";
import { LoginDialog } from "./LoginDialog";
import { PanoramaHeaderRow, panoramaHeaderIconButtonClass } from "./PanoramaHeaderRow";
import { usePanoramaRuntime, usePanoramaSnapshot } from "./RuntimeMount";
import { usePanoramaStickyHeader } from "./usePanoramaStickyHeader";

const metadataBadgeClass = "border border-inverse/65 px-1.5 py-0.5 text-[11px] font-bold uppercase";

export function FilmDetailsPage() {
  const runtime = usePanoramaRuntime();
  const snapshot = usePanoramaSnapshot();
  const router = useRouter();
  const [loginOpen, setLoginOpen] = useState(false);
  const [failedArtwork, setFailedArtwork] = useState<string[]>([]);
  const pageRef = useRef<HTMLElement>(null);
  const headerRef = useRef<HTMLElement>(null);
  const metadata = snapshot.details.metadata.item;
  const defaultSource = firstPlayableSource(snapshot.details.sources.groups);
  const defaultSourceId = defaultSource?.id ?? null;
  const landscape = metadata?.landscapeUrl && !failedArtwork.includes(metadata.landscapeUrl)
    ? metadata.landscapeUrl
    : null;
  const poster = metadata?.posterUrl && !failedArtwork.includes(metadata.posterUrl)
    ? metadata.posterUrl
    : null;
  const artwork = landscape ?? poster;
  const logo = metadata?.logoUrl && !failedArtwork.includes(metadata.logoUrl) ? metadata.logoUrl : null;
  const artworkBackground = artwork
    ? "bg-player-canvas"
    : snapshot.details.metadata.status === "idle" || snapshot.details.metadata.status === "loading"
      ? "bg-surface"
      : "bg-artwork-empty";
  const foreignTitle = metadata?.alternativeTitle ?? metadata?.originalTitle;
  const nativeTitle = metadata?.originCountry && foreignTitle &&
    foreignTitle.localeCompare(metadata.name, undefined, { sensitivity: "accent" }) !== 0
    ? foreignTitle
    : null;
  const ratingVisible = Boolean(metadata?.rating && metadata.rating > 0 && metadata.ratingCount && metadata.ratingCount > 0);
  const ratingText = metadata?.rating ? formatTmdbRating(metadata.rating) : null;
  const ratingCountText = metadata?.ratingCount ? formatRatingCount(metadata.ratingCount) : null;
  const tmdbId = snapshot.details.filmId ? parseTmdbPublicId(snapshot.details.filmId) : null;
  const { hidden: headerHidden, overHero: headerOverHero } = usePanoramaStickyHeader(headerRef, {
    heroRef: pageRef,
    overHeroInitially: true,
  });
  const awaitingFirstSource = !defaultSource &&
    (snapshot.addons.status === "syncing" || snapshot.details.sources.status === "loading");

  useEffect(() => {
    const viewport = document.querySelector<HTMLElement>(".app-viewport");
    if (viewport) viewport.scrollTop = 0;
  }, []);

  useEffect(() => {
    if (runtime && defaultSourceId) void runtime.preparePlayback(defaultSourceId);
  }, [defaultSourceId, runtime]);

  const resume = snapshot.details.resume;
  const canPlay = snapshot.account.status === "signedIn" && snapshot.service.status === "online" && Boolean(defaultSource);
  const resuming = canPlay && resume.available;
  const timeLeft = resuming ? formatTimeLeft(resume.offset, resume.duration) : null;
  const progress = resume.duration ? Math.min(100, Math.max(0, (resume.offset / resume.duration) * 100)) : 0;

  const play = () => {
    if (!runtime || !defaultSource || !tmdbId) return;
    router.push(`/films/${tmdbId}/watch${resuming ? "?resume=1" : ""}`);
  };

  const watchlist = snapshot.details.watchlist;
  const watchlistLabel = watchlist.saved ? "Remove from Watchlist" : "Add to Watchlist";

  const toggleWatchlist = () => {
    if (snapshot.account.status !== "signedIn") {
      setLoginOpen(true);
      return;
    }
    void runtime?.setWatchlisted(!watchlist.saved);
  };

  const primaryAction = () => {
    if (snapshot.account.status !== "signedIn") {
      setLoginOpen(true);
      return;
    }
    if (snapshot.service.status !== "online") {
      void runtime?.checkService();
      return;
    }
    play();
  };

  const primaryLabel = snapshot.account.status !== "signedIn"
    ? "Sign in to watch"
    : awaitingFirstSource
      ? "Loading sources"
      : !defaultSource
        ? "No playable source"
        : snapshot.service.status !== "online"
          ? "Check service"
          : resuming
            ? timeLeft ? `Resume, ${timeLeft} left` : "Resume"
            : "Play";

  return (
    <main ref={pageRef} className={`desktop-hero-viewport relative isolate min-h-dvh overflow-x-hidden text-inverse ${artworkBackground}`}>
      {artwork ? (
        <img
          className={`absolute inset-0 -z-30 size-full ${landscape ? "object-cover" : "object-cover object-[50%_28%]"}`}
          src={artwork}
          alt=""
          onError={() => setFailedArtwork((current) => current.includes(artwork) ? current : [...current, artwork])}
        />
      ) : null}
      <div className="pointer-events-none absolute inset-0 -z-20 bg-[linear-gradient(180deg,rgba(0,0,0,0.48)_0%,rgba(0,0,0,0.04)_28%,rgba(0,0,0,0.16)_52%,rgba(0,0,0,0.92)_100%),radial-gradient(circle_at_58%_48%,transparent_0%,rgba(0,0,0,0.2)_72%)]" aria-hidden="true" />

      <header
        ref={headerRef}
        data-hidden={headerHidden}
        className={`panorama-site-header fixed inset-x-0 z-40 ${headerOverHero ? metadata ? "bg-transparent text-inverse" : "bg-transparent text-ink" : "bg-canvas text-ink"}`}
        inert={headerHidden || undefined}
      >
        <PanoramaHeaderRow
          leadingControl={(
            <Button
              className={panoramaHeaderIconButtonClass}
              type="button"
              aria-label="Back to films"
              title="Back to films"
              onClick={() => router.back()}
            >
              <IconArrowLeft aria-hidden="true" size={22} stroke={1.6} />
            </Button>
          )}
          signedInEmail={snapshot.account.status === "signedIn" ? snapshot.account.email : null}
          onSignIn={() => setLoginOpen(true)}
          onLogout={() => void runtime?.logout()}
          logoInverted={!headerOverHero || !metadata}
        />
      </header>

      {ratingVisible && metadata ? (
        <div className="content-container absolute inset-x-0 top-24 z-20 text-right max-[700px]:top-20" aria-label={`TMDB score ${ratingText} out of 10 from ${ratingCountText}`}>
          <p className="flex items-baseline justify-end gap-1 text-[22px] font-medium leading-none">
            <IconStarFilled aria-hidden="true" className="self-center" size={19} />
            <span>{ratingText}</span>
            <span className="text-[12px] text-inverse/80">/10</span>
          </p>
          <p className="mt-1 text-[12px] text-inverse/75 tabular-nums">{ratingCountText}</p>
        </div>
      ) : null}

      <div className="desktop-hero-viewport content-container flex min-h-dvh flex-col pb-10 pt-[clamp(7rem,12vh,9rem)] max-[700px]:pt-44">
        {snapshot.details.metadata.status === "error" ? (
          <div className="mt-auto max-w-lg bg-player-canvas/70 p-5" role="alert">
            <p className="type-body">Unable to load film details. Check your connection and try again.</p>
            <Button className="focus-ring mt-4 min-h-10 border-0 border-b border-current bg-transparent p-0 text-sm" type="button" onClick={() => void runtime?.retryFilmDetails()}>Retry details</Button>
          </div>
        ) : metadata ? (
          <div className="mt-auto">
            <div>
              <section className="flex min-w-0 flex-col" aria-labelledby="film-title">
                <h1 className={logo ? "sr-only" : "w-full text-[32px] font-medium leading-[0.9] text-balance uppercase"} id="film-title">
                  {metadata.name}
                </h1>
                {logo ? (
                  <img
                    className="block h-auto max-h-32 w-auto max-w-[310px] object-contain object-left"
                    src={logo}
                    alt=""
                    data-testid="film-title-logo"
                    onError={() => setFailedArtwork((current) => current.includes(logo) ? current : [...current, logo])}
                  />
                ) : null}
                {nativeTitle ? <p className={`${logo ? "mt-[10px]" : "mt-1"} text-[16px] font-medium leading-none text-inverse uppercase`}>{nativeTitle}</p> : null}
              </section>
              <div className="mt-5 grid grid-cols-[310px_minmax(0,1fr)] items-start gap-x-[70px] max-[900px]:mt-5 max-[900px]:flex max-[900px]:flex-col max-[900px]:gap-6">
                <div className="flex min-w-0 flex-col gap-3 text-[13px] text-inverse/85">
                  {metadata.director || metadata.originCountry || metadata.year ? (
                    <div className="flex w-0 min-w-full flex-col gap-0 text-[14px] leading-none text-inverse uppercase">
                      {metadata.director ? (
                        <p className="text-[14px] leading-none break-words">
                          <span className="font-normal">Directed by </span>
                          <strong className="font-bold">{metadata.director}</strong>
                        </p>
                      ) : null}
                      {metadata.originCountry || metadata.year ? (
                        <p className="mt-0.5 flex flex-wrap items-baseline gap-x-2 text-[14px] leading-none font-normal tabular-nums">
                          {metadata.originCountry ? <span>{metadata.originCountry}</span> : null}
                          {metadata.year ? <span>{metadata.year}</span> : null}
                        </p>
                      ) : null}
                    </div>
                  ) : null}
                  {metadata.genres.length > 0 ? <p>{metadata.genres.join(", ")}</p> : null}
                  {defaultSource?.quality || defaultSource?.audioChannels || metadata.runtime ? (
                    <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
                      {defaultSource?.quality ? (
                        <span className={metadataBadgeClass} aria-label={`Quality: ${defaultSource.quality === "4k" ? "4K" : "HD"}`}>{defaultSource.quality === "4k" ? "4K" : "HD"}</span>
                      ) : null}
                      {defaultSource?.audioChannels === "5.1" ? (
                        <span className={metadataBadgeClass} aria-label="Audio: 5.1">5.1</span>
                      ) : null}
                      {metadata.runtime ? <span className="tabular-nums">{metadata.runtime}</span> : null}
                    </div>
                  ) : null}
                  <div className="mt-2 flex items-center gap-3">
                    <Button
                      className="focus-ring inline-flex h-10 w-fit min-w-36 cursor-pointer items-center justify-center gap-2.5 rounded-full bg-inverse pl-7 pr-8 text-[17px] font-medium leading-none text-ink transition-[background-color,scale] duration-fast hover:bg-inverse/90 active:scale-[0.96] disabled:cursor-not-allowed disabled:opacity-55 disabled:active:scale-100"
                      type="button"
                      aria-label={primaryLabel}
                      disabled={snapshot.account.status === "signedIn" && (awaitingFirstSource || !defaultSource)}
                      onClick={primaryAction}
                    >
                      {awaitingFirstSource ? (
                        <IconLoader2 aria-hidden="true" className="animate-spin" size={17} stroke={2} />
                      ) : canPlay ? (
                        <IconPlayerPlayFilled aria-hidden="true" size={17} />
                      ) : null}
                      {resuming && timeLeft ? (
                        <>
                          <span className="relative h-1.5 w-16 overflow-hidden rounded-full bg-ink/20" aria-hidden="true">
                            <span className="absolute inset-y-0 left-0 min-w-1.5 rounded-full bg-ink" style={{ width: `${progress}%` }} />
                          </span>
                          <span aria-hidden="true">{timeLeft}</span>
                        </>
                      ) : (
                        <span aria-hidden="true">{resuming ? "Resume" : canPlay ? "Play" : primaryLabel}</span>
                      )}
                    </Button>
                    <Button
                      className="focus-ring grid size-10 shrink-0 cursor-pointer place-items-center rounded-full bg-inverse/15 text-inverse transition-[background-color,scale] duration-fast hover:bg-inverse/25 active:scale-[0.96] disabled:cursor-not-allowed disabled:opacity-55 disabled:active:scale-100"
                      type="button"
                      aria-label={watchlistLabel}
                      title={watchlistLabel}
                      disabled={snapshot.account.status === "signedIn" && !watchlist.available}
                      onClick={toggleWatchlist}
                    >
                      <AnimatePresence initial={false} mode="popLayout">
                        <m.span
                          key={watchlist.saved ? "saved" : "add"}
                          className="col-start-1 row-start-1 grid place-items-center"
                          initial={{ opacity: 0, scale: 0.25, filter: "blur(4px)" }}
                          animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }}
                          exit={{ opacity: 0, scale: 0.25, filter: "blur(4px)" }}
                          transition={{ type: "spring", duration: 0.3, bounce: 0 }}
                        >
                          {watchlist.saved
                            ? <IconCheck aria-hidden="true" size={20} stroke={2} />
                            : <IconPlus aria-hidden="true" size={20} stroke={2} />}
                        </m.span>
                      </AnimatePresence>
                    </Button>
                  </div>
                  <p className="sr-only" role="status" aria-live="polite">{primaryLabel}</p>
                  {snapshot.account.status === "signedIn" && !awaitingFirstSource && !defaultSource ? (
                    <div className="flex flex-wrap items-center gap-3" role="status">
                      <span>{snapshot.details.sources.error ?? "No playable source is available from your addons."}</span>
                      <Button className="focus-ring min-h-10 border-0 border-b border-current bg-transparent px-1" type="button" onClick={() => void runtime?.retryFilmDetails()}>Retry sources</Button>
                    </div>
                  ) : null}
                </div>
                <section className="min-w-0 max-w-[650px]" aria-labelledby="synopsis-title">
                  <h2 className="text-[14px] font-bold leading-none uppercase" id="synopsis-title">Synopsis</h2>
                  <p className="mt-2 text-[15px] leading-[1.48] text-inverse/95">{metadata.description ?? "No synopsis is available for this film."}</p>
                </section>
              </div>
            </div>
          </div>
        ) : (
          <div className="mt-auto w-full max-w-xl" aria-label="Loading film metadata" aria-busy="true">
            <div className="h-10 w-2/3 bg-inverse/20" />
            <div className="mt-4 h-4 w-1/3 bg-inverse/15" />
            <div className="mt-10 h-20 w-full bg-inverse/10" />
          </div>
        )}
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
