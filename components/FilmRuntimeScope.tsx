"use client";

import { useEffect, useRef } from "react";
import { usePanoramaRuntime } from "./RuntimeMount";

export function FilmRuntimeScope({ filmId, children }: { filmId: string; children: React.ReactNode }) {
  const runtime = usePanoramaRuntime();
  const scopeGeneration = useRef(0);

  useEffect(() => {
    if (!runtime) return;
    const generation = ++scopeGeneration.current;
    const isCurrentScope = () => scopeGeneration.current === generation;
    void runtime.openFilmDetails(filmId);
    return () => {
      queueMicrotask(() => {
        if (isCurrentScope()) void runtime.closeFilmDetails();
      });
    };
  }, [filmId, runtime]);

  return children;
}
