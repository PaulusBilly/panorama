"use client";

import { createContext, useContext, useState } from "react";
import { usePanoramaRuntime } from "./RuntimeMount";
import { PanoramaHome, type SearchCategory } from "./PanoramaHome";

type SearchRouteContextValue = {
  searchOpen: boolean;
  setSearchOpen(open: boolean): void;
};

const SearchRouteContext = createContext<SearchRouteContextValue | null>(null);

export function PanoramaSearchRouteProvider({ children }: { children: React.ReactNode }) {
  const [searchOpen, setSearchOpen] = useState(true);
  return (
    <SearchRouteContext.Provider value={{ searchOpen, setSearchOpen }}>
      {children}
    </SearchRouteContext.Provider>
  );
}

export function PanoramaSearchRoute({ category, query }: { category: SearchCategory; query: string }) {
  const runtime = usePanoramaRuntime();
  const routeState = useContext(SearchRouteContext);
  if (!routeState) throw new Error("PanoramaSearchRoute must be rendered inside PanoramaSearchRouteProvider.");
  return (
    <PanoramaHome
      key={query}
      runtime={runtime}
      searchRoute={{
        category,
        query,
        searchOpen: routeState.searchOpen,
        onSearchOpenChange: routeState.setSearchOpen,
      }}
    />
  );
}
