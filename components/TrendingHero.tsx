"use client";

import { Button } from "@base-ui/react/button";

import { useEffect, useState } from "react";
import { IconPlayerPlayFilled } from "@tabler/icons-react";
import type { PanoramaFilm } from "@/runtime/types";

type Props = {
  film: PanoramaFilm | null;
  loading: boolean;
  onWatch(film: PanoramaFilm): void;
  onReadyChange?(ready: boolean): void;
};

export function TrendingHero({ film, loading, onWatch, onReadyChange }: Props) {
  const [failedArtwork, setFailedArtwork] = useState<string[]>([]);
  const [loadedArtwork, setLoadedArtwork] = useState<string | null>(null);
  const landscape = film?.landscapeUrl && !failedArtwork.includes(film.landscapeUrl)
    ? film.landscapeUrl
    : null;
  const poster = film?.posterUrl && !failedArtwork.includes(film.posterUrl)
    ? film.posterUrl
    : null;
  const artwork = landscape ?? poster;
  const logo = film?.logoUrl && !failedArtwork.includes(film.logoUrl) ? film.logoUrl : null;
  const artworkReady = !artwork || loadedArtwork === artwork;
  const contentReady = Boolean(film) && !loading && artworkReady;

  useEffect(() => {
    onReadyChange?.(contentReady);
  }, [contentReady, onReadyChange]);

  return (
    <section
      className={`desktop-hero-viewport relative isolate min-h-dvh overflow-hidden text-inverse ${artwork ? "bg-player-canvas" : "bg-artwork-empty"}`}
      aria-label="Featured film"
      aria-busy={!contentReady}
    >
      {artwork ? (
        <img
          className={`absolute inset-0 -z-30 size-full ${landscape ? "object-cover" : "object-cover object-[50%_28%]"}`}
          src={artwork}
          alt=""
          fetchPriority="high"
          onLoad={() => setLoadedArtwork(artwork)}
          onError={() => {
            setFailedArtwork((current) => current.includes(artwork) ? current : [...current, artwork]);
            setLoadedArtwork(artwork);
          }}
        />
      ) : null}
      <div
        className="pointer-events-none absolute inset-0 -z-20 bg-[radial-gradient(circle_at_18%_100%,var(--theme-hero-copy-fade),transparent_58%),linear-gradient(180deg,var(--theme-hero-top)_0%,var(--theme-hero-mid)_44%,var(--theme-hero-bottom)_100%)]"
        data-testid="hero-fade"
        aria-hidden="true"
      />

      <div className="desktop-hero-viewport content-container flex min-h-dvh flex-col pb-10 pt-[clamp(7rem,12vh,9rem)] max-[700px]:pt-44">
        {film ? (
          <div className="mt-auto grid grid-cols-[max-content_minmax(0,1fr)] gap-x-[70px] max-[900px]:flex max-[900px]:flex-col max-[900px]:gap-6">
            <div className="flex w-max max-w-full flex-col">
              <h1 className={logo ? "sr-only" : `${film.director ? "w-0 min-w-full" : "w-max max-w-full"} text-[32px] font-medium leading-[0.9] text-balance uppercase`}>
                {film.name}
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
              {film.director || film.originCountry || film.year ? (
                <div className="mt-4 flex w-max max-w-full flex-col gap-0 text-[14px] leading-none uppercase text-inverse/85">
                  {film.director ? (
                    <p className="whitespace-nowrap text-[14px] leading-none">
                      <span className="font-normal">Directed by </span>
                      <strong className="font-bold">{film.director}</strong>
                    </p>
                  ) : null}
                  {film.originCountry || film.year ? (
                    <p className="mt-0.5 flex flex-wrap items-baseline gap-x-2 text-[14px] leading-none font-normal tabular-nums">
                      {film.originCountry ? <span>{film.originCountry}</span> : null}
                      {film.year ? <span>{film.year}</span> : null}
                    </p>
                  ) : null}
                </div>
              ) : null}
            </div>
            <div className="col-span-2 col-start-1 mt-6 grid grid-cols-subgrid items-center max-[900px]:mt-5 max-[900px]:flex max-[900px]:flex-col max-[900px]:items-start max-[900px]:gap-6">
              <Button
                className="focus-ring inline-flex w-fit min-h-11 cursor-pointer items-center gap-2 border border-inverse bg-inverse px-4 text-[16px] font-medium uppercase text-ink transition-colors duration-fast hover:bg-[transparent] hover:text-[var(--theme-inverse)]"
                type="button"
                aria-label={`WATCH ${film.name}`}
                onClick={() => onWatch(film)}
              >
                <IconPlayerPlayFilled aria-hidden="true" size={16} />
                WATCH
              </Button>
              {film.description ? (
                <p className="min-w-0 max-w-[650px] text-[14px] leading-[1.55] text-inverse/85 max-[900px]:max-w-none">
                  {film.description}
                </p>
              ) : null}
            </div>
          </div>
        ) : null}
      </div>

      <div
        className={`absolute inset-0 z-10 bg-surface transition-opacity duration-[320ms] ease-[cubic-bezier(0.22,1,0.36,1)] ${contentReady ? "pointer-events-none opacity-0" : "opacity-100"}`}
        data-testid="hero-skeleton"
        aria-hidden="true"
      />
    </section>
  );
}
