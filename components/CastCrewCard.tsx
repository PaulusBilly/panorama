"use client";

import { useState } from "react";
import type { PanoramaPerson } from "@/runtime/types";

type Props = {
  person: PanoramaPerson;
};

export function CastCrewCard({ person }: Props) {
  const [failed, setFailed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const portrait = person.profileUrl && !failed ? person.profileUrl : null;

  return (
    <article className="min-w-0 bg-canvas">
      <div
        className={`relative aspect-[4/5] overflow-hidden shadow-[inset_0_0_0_1px_var(--theme-image-outline)] ${portrait ? "bg-surface" : "bg-artwork-empty"}`}
      >
        {portrait && !loaded ? (
          <div className="artwork-shimmer absolute inset-0" aria-hidden="true" data-testid="person-artwork-shimmer" />
        ) : null}
        {portrait ? (
          <img
            className={`block size-full object-cover grayscale transition-opacity duration-[220ms] ease-editorial ${loaded ? "opacity-100" : "opacity-0"}`}
            src={portrait}
            alt=""
            loading="lazy"
            onLoad={() => setLoaded(true)}
            onError={() => {
              setFailed(true);
              setLoaded(false);
            }}
          />
        ) : null}
      </div>
      <div className="min-h-[92px] px-4 py-4 uppercase">
        <h2 className="type-heading text-balance break-words" title={person.name}>{person.name}</h2>
        {person.department ? (
          <p className="type-label mt-1 text-ink-muted" title={person.department}>{person.department}</p>
        ) : null}
      </div>
    </article>
  );
}
