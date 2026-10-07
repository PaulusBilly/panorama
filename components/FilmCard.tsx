"use client";

import { Button } from "@base-ui/react/button";

import { useState } from "react";
import type { PanoramaFilm } from "@/runtime/types";

type Props = {
  film: PanoramaFilm;
  onOpen(): void;
};

export function FilmCard({ film, onOpen }: Props) {
  const [failedArtwork, setFailedArtwork] = useState<string[]>([]);
  const [loadedArtwork, setLoadedArtwork] = useState<string | null>(null);
  const landscape = film.landscapeUrl && !failedArtwork.includes(film.landscapeUrl) ? film.landscapeUrl : null;
  const poster = film.posterUrl && !failedArtwork.includes(film.posterUrl) ? film.posterUrl : null;
  const artwork = landscape ?? poster;
  const artworkLoaded = Boolean(artwork && loadedArtwork === artwork);
  const artworkUnavailable = !artwork;
  const hasCredits = Boolean(film.director || film.originCountry || film.year);

  return (
    <article className="group min-w-0">
      <div
        className={`relative aspect-[404/245] overflow-hidden shadow-[inset_0_0_0_1px_var(--theme-image-outline)] ${
          artworkUnavailable ? "bg-artwork-empty" : "bg-surface"
        }`}
      >
        {artwork && !artworkLoaded ? (
          <div className="artwork-shimmer absolute inset-0" aria-hidden="true" data-testid="artwork-shimmer" />
        ) : null}
        {artwork ? (
          <img
            src={artwork}
            alt=""
            loading="lazy"
            onLoad={() => setLoadedArtwork(artwork)}
            onError={() => {
              setLoadedArtwork(null);
              setFailedArtwork((current) => current.includes(artwork) ? current : [...current, artwork]);
            }}
            className={`block size-full transition-[transform,filter,opacity] duration-[220ms] ease-editorial group-hover:scale-[1.012] ${
              artworkLoaded ? "opacity-100" : "opacity-0"
            } ${
              landscape
                ? "object-cover group-hover:saturate-100"
                : "object-cover object-[50%_28%] saturate-[0.86] group-hover:saturate-100"
            }`}
          />
        ) : null}
        <div className="pointer-events-none absolute inset-x-0 bottom-0 z-10 h-[46%] bg-gradient-to-b from-transparent to-[var(--theme-artwork-fade)]" />
        <div className="pointer-events-none absolute inset-x-0 bottom-0 z-20 flex flex-col justify-end gap-0.5 px-[18px] pb-3.5 pt-7 text-inverse uppercase">
          <h2
            className="line-clamp-2 max-h-[2.36em] text-[clamp(1rem,1.8vw,var(--text-xl))] font-medium leading-[1.18] text-balance break-words"
            title={film.name}
          >
            {film.name}
          </h2>
          {hasCredits ? (
            <p className="flex min-w-0 shrink-0 items-baseline gap-1 overflow-hidden text-[clamp(calc(var(--text-xs)-2px),0.9vw,calc(var(--text-sm)-2px))] leading-[1.25] whitespace-nowrap">
              {film.director ? (
                <span className="min-w-0 overflow-hidden text-ellipsis font-bold" title={film.director}>
                  {film.director}
                </span>
              ) : null}
              {film.originCountry ? (
                <span className="shrink-0 font-normal text-inverse/90 tabular-nums">{film.originCountry}</span>
              ) : null}
              {film.year ? (
                <span className="shrink-0 font-normal text-inverse/90 tabular-nums">{film.year}</span>
              ) : null}
            </p>
          ) : null}
        </div>
        <Button
          className="focus-ring absolute inset-0 z-30 cursor-pointer border-0 bg-transparent p-0"
          type="button"
          aria-label={`Open details for ${film.name}`}
          onClick={onOpen}
        />
      </div>
    </article>
  );
}
