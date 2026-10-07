import { notFound } from "next/navigation";
import { FilmRuntimeScope } from "@/components/FilmRuntimeScope";

export default async function FilmLayout({ children, params }: LayoutProps<"/films/[tmdbId]">) {
  const { tmdbId } = await params;
  if (!/^\d+$/.test(tmdbId) || Number(tmdbId) <= 0) notFound();
  return <FilmRuntimeScope filmId={`tmdb:${tmdbId}`}>{children}</FilmRuntimeScope>;
}
