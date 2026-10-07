import { notFound, redirect } from "next/navigation";
import { PanoramaSearchRoute } from "@/components/PanoramaSearchRoute";
import type { SearchCategory } from "@/components/PanoramaHome";

type Props = {
  params: Promise<{ category: string }>;
  searchParams: Promise<{ query?: string | string[] }>;
};

function isSearchCategory(value: string): value is SearchCategory {
  return value === "films" || value === "people";
}

export default async function SearchPage({ params, searchParams }: Props) {
  const [{ category }, resolvedSearchParams] = await Promise.all([params, searchParams]);
  if (!isSearchCategory(category)) notFound();

  const rawQuery = Array.isArray(resolvedSearchParams.query)
    ? resolvedSearchParams.query[0]
    : resolvedSearchParams.query;
  const query = rawQuery?.trim() ?? "";
  if (!query) redirect("/");
  if (rawQuery !== query || Array.isArray(resolvedSearchParams.query)) {
    redirect(`/search/${category}?${new URLSearchParams({ query }).toString()}`);
  }

  return <PanoramaSearchRoute category={category} query={query} />;
}
