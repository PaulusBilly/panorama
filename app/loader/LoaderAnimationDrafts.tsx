"use client";

import { Button } from "@base-ui/react/button";

import Image from "next/image";
import { useState } from "react";

const drafts = [
  {
    number: "01",
    name: "Cascade",
    description: "Each bar shifts right in sequence, then rolls back into the mark.",
    animation: "loader-draft__mark--cascade",
  },
  {
    number: "02",
    name: "Frames",
    description: "The bars jump right in deliberate frame-by-frame steps.",
    animation: "loader-draft__mark--frames",
  },
  {
    number: "03",
    name: "Lockstep",
    description: "The bars collect in a shifted position one by one before resetting.",
    animation: "loader-draft__mark--lockstep",
  },
] as const;

export function LoaderAnimationDrafts() {
  const [paused, setPaused] = useState(false);

  return (
    <main className={`loader-drafts min-h-dvh bg-canvas text-ink ${paused ? "loader-drafts--paused" : ""}`}>
      <div className="mx-auto flex min-h-dvh w-full max-w-[96rem] flex-col px-5 py-8 sm:px-9 sm:py-12 lg:px-[5.125rem] lg:py-16">
        <header className="flex flex-col items-start justify-between gap-7 sm:flex-row sm:items-end">
          <div className="max-w-2xl">
            <p className="type-label text-ink-muted">Panorama loader study</p>
            <h1 className="type-display mt-3 text-balance">Three ways to move right</h1>
            <p className="type-body mt-4 max-w-xl text-pretty text-ink-muted">
              Each draft uses the same icon and changes only the rhythm of the shift.
            </p>
          </div>

          <div className="flex flex-col items-start gap-2 sm:items-end">
            <Button
              type="button"
              className="focus-ring min-h-10 cursor-pointer border border-rule bg-canvas px-4 py-2 type-label text-ink transition-colors duration-fast hover:bg-hover"
              aria-pressed={paused}
              onClick={() => setPaused((current) => !current)}
            >
              {paused ? "Play animations" : "Pause animations"}
            </Button>
            <span className="sr-only-stable" role="status">
              {paused ? "Animations paused" : "Animations playing"}
            </span>
          </div>
        </header>

        <section className="mt-12 grid flex-1 gap-4 lg:mt-16 lg:grid-cols-3" aria-label="Loader animation drafts">
          {drafts.map((draft) => (
            <article
              key={draft.number}
              className="flex min-h-[22rem] flex-col bg-player-canvas p-5 text-player-ink sm:p-7 lg:min-h-[28rem]"
            >
              <div className="flex items-baseline justify-between gap-4">
                <h2 className="type-title">{draft.name}</h2>
                <span className="type-caption text-player-ink/60">{draft.number}</span>
              </div>

              <div className="loader-draft__stage my-auto" aria-hidden="true">
                <div className={`loader-draft__mark ${draft.animation}`}>
                  {Array.from({ length: 4 }, (_, index) => (
                    <span className="loader-draft__bar" key={index}>
                      <Image src="/icon.svg" alt="" width={366} height={197} />
                    </span>
                  ))}
                </div>
              </div>

              <p className="type-body max-w-sm text-pretty text-player-ink/70">{draft.description}</p>
            </article>
          ))}
        </section>
      </div>
    </main>
  );
}
