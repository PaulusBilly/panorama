"use client";

import { PanoramaHome } from "@/components/PanoramaHome";
import { usePanoramaRuntime } from "@/components/RuntimeMount";

export default function HomePage() {
  const runtime = usePanoramaRuntime();
  return <PanoramaHome runtime={runtime} />;
}
