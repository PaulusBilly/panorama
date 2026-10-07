"use client";

import { Button } from "@base-ui/react/button";
import { Tabs } from "@base-ui/react/tabs";
import { Input } from "@base-ui/react/input";

import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { useRouter } from "next/navigation";
import { IconArrowRight, IconSearch, IconX } from "@tabler/icons-react";
import { initialRuntimeSnapshot } from "@/runtime/snapshot";
import type { PanoramaFilm, StremioRuntime } from "@/runtime/types";
import { parseTmdbPublicId } from "@/runtime/tmdb";
import { FilmCard } from "./FilmCard";
import { CastCrewCard } from "./CastCrewCard";
import { LoginDialog } from "./LoginDialog";
import { TrendingHero } from "./TrendingHero";
import { PanoramaHeaderRow, panoramaHeaderIconButtonClass } from "./PanoramaHeaderRow";
import { usePanoramaStickyHeader } from "./usePanoramaStickyHeader";

const noopSubscribe = () => () => undefined;
const headerIconProps = { size: 22, stroke: 1.6 } as const;
const searchMotionClass = "transition-[opacity,padding-top,background-color,grid-template-rows] duration-[320ms] ease-[cubic-bezier(0.22,1,0.36,1)]";

type Props = {
  runtime: StremioRuntime | null;
  searchRoute?: SearchRouteState;
};

const actionClass = "focus-ring min-h-10 cursor-pointer border-0 border-b border-current bg-transparent px-1 text-sm text-ink";
const catalogGridClass = "grid grid-cols-[repeat(4,23.25rem)] justify-center gap-x-1 gap-y-9 pt-8 max-[1640px]:grid-cols-[repeat(3,23.25rem)] max-[1220px]:grid-cols-[repeat(2,23.25rem)] max-[850px]:grid-cols-[repeat(2,20.9375rem)] max-[720px]:grid-cols-[20.9375rem] max-[480px]:grid-cols-[minmax(0,min(20.9375rem,100%))]";
const peopleGridClass = "grid grid-cols-6 gap-x-1 gap-y-12 pt-8 max-[1640px]:grid-cols-5 max-[1220px]:grid-cols-4 max-[850px]:grid-cols-3 max-[720px]:grid-cols-2";
export type SearchCategory = "films" | "people";

export type SearchRouteState = {
  query: string;
  category: SearchCategory;
  searchOpen: boolean;
  onSearchOpenChange(open: boolean): void;
};

export function PanoramaHome({ runtime, searchRoute }: Props) {
  const [loginOpen, setLoginOpen] = useState(false);
  const router = useRouter();
  const [homeSearchOpen, setHomeSearchOpen] = useState(false);
  const [searchValue, setSearchValue] = useState(searchRoute?.query ?? "");
  const [openHeaderOffset, setOpenHeaderOffset] = useState(0);
  const [heroReady, setHeroReady] = useState(false);
  const [retainedFeaturedFilm, setRetainedFeaturedFilm] = useState<PanoramaFilm | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const headerRef = useRef<HTMLElement>(null);
  const heroRef = useRef<HTMLDivElement>(null);
  const headerRowRef = useRef<HTMLDivElement>(null);
  const searchBandContentRef = useRef<HTMLFormElement>(null);
  const requestedSearchRef = useRef<{ runtime: StremioRuntime; query: string } | null>(null);
  const snapshot = useSyncExternalStore(
    runtime?.subscribe ?? noopSubscribe,
    runtime?.getSnapshot ?? (() => initialRuntimeSnapshot),
    () => initialRuntimeSnapshot,
  );
  const films = snapshot.catalog.page.items;
  const people = snapshot.people.page.items;
  const featuredFilm = snapshot.catalog.mode === "popular" ? films[0] ?? retainedFeaturedFilm : retainedFeaturedFilm;
  const showSkeletons = snapshot.catalog.status === "idle" || snapshot.catalog.status === "loading";
  const signedInEmail = snapshot.account.status === "signedIn" ? snapshot.account.email : null;
  const searching = searchRoute !== undefined;
  const routedQuery = searchRoute?.query;
  const searchOpen = searchRoute?.searchOpen ?? homeSearchOpen;
  const activeSearchTab = searchRoute?.category ?? "films";
  const searchQuery = searchRoute?.query ?? snapshot.catalog.query;
  const catalogLabel = searching && searchQuery
    ? `Search results for ${searchQuery}`
    : "Popular films";
  const { height: headerHeight, hidden: headerHidden, overHero: headerOverHero } = usePanoramaStickyHeader(
    headerRef,
    {
      heroRef: searching ? undefined : heroRef,
      overHeroInitially: !searching,
      revealOn: searchOpen,
    },
  );

  useLayoutEffect(() => {
    const row = headerRowRef.current;
    const band = searchBandContentRef.current;
    if (!row || !band) return;

    const syncOffset = () => {
      setOpenHeaderOffset(row.getBoundingClientRect().height + band.scrollHeight);
    };
    syncOffset();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(syncOffset);
    observer.observe(row);
    observer.observe(band);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (searchOpen) searchRef.current?.focus();
  }, [searchOpen]);

  useEffect(() => {
    if (!routedQuery) return;
    if (!runtime || snapshot.catalog.mode === "search" && snapshot.catalog.query === routedQuery) return;
    const requested = requestedSearchRef.current;
    if (requested?.runtime === runtime && requested.query === routedQuery) return;
    requestedSearchRef.current = { runtime, query: routedQuery };
    void runtime.searchMovies(routedQuery);
  }, [runtime, routedQuery, snapshot.catalog.mode, snapshot.catalog.query]);

  useEffect(() => {
    if (searching || snapshot.catalog.mode !== "search") return;
    void runtime?.clearSearch();
  }, [runtime, searching, snapshot.catalog.mode]);

  const setSearchOpen = (open: boolean) => {
    if (searchRoute) searchRoute.onSearchOpenChange(open);
    else setHomeSearchOpen(open);
  };

  const hideSearch = () => {
    setSearchOpen(false);
  };

  const clearSearch = () => {
    setSearchOpen(false);
    setSearchValue("");
    if (snapshot.catalog.mode === "search") {
      void runtime?.clearSearch();
    }
    router.replace("/");
  };

  const openSearch = () => {
    if (snapshot.catalog.mode === "popular" && films[0]) setRetainedFeaturedFilm(films[0]);
    if (!searchValue && searchQuery) {
      setSearchValue(searchQuery);
    }
    setSearchOpen(true);
  };

  const openFilm = (film: PanoramaFilm) => {
    const tmdbId = parseTmdbPublicId(film.id);
    if (tmdbId) router.push(`/films/${tmdbId}`);
  };

  const watchFilm = (film: PanoramaFilm) => {
    const tmdbId = parseTmdbPublicId(film.id);
    if (tmdbId) router.push(`/films/${tmdbId}/watch`);
  };

  const selectSearchTab = (tab: SearchCategory) => {
    if (searchQuery) {
      const params = new URLSearchParams({ query: searchQuery });
      router.push(`/search/${tab}?${params.toString()}`);
    }
  };

  const header = (
    <header
      ref={headerRef}
      data-hidden={headerHidden}
      className={`panorama-site-header fixed inset-x-0 z-40 ${searching ? "pb-5" : ""} ${searchOpen ? "bg-player-canvas text-inverse" : headerOverHero ? heroReady ? "bg-transparent text-inverse" : "bg-transparent text-ink" : "bg-canvas text-ink"}`}
      inert={headerHidden || undefined}
    >
      <PanoramaHeaderRow
        rowRef={headerRowRef}
        leadingControl={(
          <Button
            className={panoramaHeaderIconButtonClass}
            type="button"
            aria-label={searchOpen ? "Close search" : "Search"}
            aria-expanded={searchOpen}
            aria-controls="movie-search-band"
            title={searchOpen ? "Close search" : "Search"}
            onClick={searchOpen ? hideSearch : openSearch}
          >
            {searchOpen
              ? <IconX aria-hidden="true" {...headerIconProps} />
              : <IconSearch aria-hidden="true" {...headerIconProps} />}
          </Button>
        )}
        signedInEmail={signedInEmail}
        onSignIn={() => setLoginOpen(true)}
        onLogout={() => void runtime?.logout()}
        chromeHidden={searchOpen}
        logoInverted={!searchOpen && (!headerOverHero || !heroReady)}
      />
      <div
        className={`grid ${searchMotionClass} ${searchOpen ? "grid-rows-[1fr]" : "grid-rows-[0fr]"}`}
        id="movie-search-band"
      >
        <div className="min-h-0 overflow-hidden">
          <form
            ref={searchBandContentRef}
            className="content-container py-10 max-[700px]:py-8"
            role="search"
            aria-hidden={!searchOpen}
            inert={!searchOpen || undefined}
            onSubmit={(event) => {
              event.preventDefault();
              const query = searchValue.trim();
              if (query) {
                const params = new URLSearchParams({ query });
                router.push(`/search/films?${params.toString()}`);
              } else {
                clearSearch();
              }
            }}
          >
            <label className="sr-only-stable" htmlFor="movie-search">Search movies</label>
            <div className="relative">
              <Input
                ref={searchRef}
                className="movie-search-input w-full min-h-14 appearance-none border-0 border-b border-inverse bg-transparent pb-3 pr-20 type-display text-[clamp(3rem,calc(5vw+0.75rem),4rem)]! leading-[1.2]! text-inverse outline-none focus:border-inverse max-[700px]:min-h-12 max-[700px]:pb-2 max-[700px]:pr-16 max-[700px]:text-[clamp(2.5rem,calc(8vw+0.75rem),3rem)]!"
                id="movie-search"
                name="search"
                type="search"
                value={searchValue}
                autoComplete="off"
                tabIndex={searchOpen ? 0 : -1}
                onChange={(event) => setSearchValue(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Escape") {
                    event.preventDefault();
                    clearSearch();
                  }
                }}
              />
              <Button
                className={`${panoramaHeaderIconButtonClass} absolute right-0 bottom-3 max-[700px]:bottom-2`}
                type="submit"
                aria-label="Submit search"
                title="Submit search"
                tabIndex={searchOpen ? 0 : -1}
              >
                <IconArrowRight aria-hidden="true" size={52} stroke={1.4} />
              </Button>
            </div>
          </form>
        </div>
      </div>
    </header>
  );

  return (
    <main className="min-h-screen overflow-hidden bg-canvas">
      <div
        className={searching ? "relative bg-canvas" : "relative min-h-dvh bg-player-canvas"}
        style={{ paddingTop: searching ? headerHeight : undefined }}
      >
        {header}
        {!searching ? (
          <div
            ref={heroRef}
            className={searchMotionClass}
            style={{ paddingTop: searchOpen ? openHeaderOffset : 0 }}
          >
            <TrendingHero
              film={featuredFilm}
              loading={showSkeletons && featuredFilm === null}
              onWatch={watchFilm}
              onReadyChange={setHeroReady}
            />
          </div>
        ) : null}
      </div>

      <Tabs.Root render={<section />} value={activeSearchTab} onValueChange={(value) => selectSearchTab(value as SearchCategory)} className="content-container pb-28" aria-label={catalogLabel}>
        {snapshot.runtime.status === "error" ? (
          <div className="mt-8 flex items-center justify-between gap-8 border-y border-rule py-[22px] max-[700px]:flex-col max-[700px]:items-start" role="alert">
            <p className="type-body">{snapshot.runtime.error}</p>
            <Button className={actionClass} type="button" onClick={() => window.location.reload()}>Reload Panorama</Button>
          </div>
        ) : null}

        {searching ? (
          <Tabs.List activateOnFocus className="flex gap-8 border-b border-rule pt-8" aria-label="Search result categories">
            <Tabs.Tab value="films"
              className={`focus-ring -mb-px min-h-10 cursor-pointer border-0 border-b-2 bg-transparent px-1 type-label uppercase transition-colors duration-[160ms] ${activeSearchTab === "films" ? "border-ink text-ink" : "border-transparent text-ink-muted hover:text-ink"}`}
              id="films-tab"
              type="button"
              aria-controls="films-panel"
            >
              Films
            </Tabs.Tab>
            <Tabs.Tab value="people"
              className={`focus-ring -mb-px min-h-10 cursor-pointer border-0 border-b-2 bg-transparent px-1 type-label uppercase transition-colors duration-[160ms] ${activeSearchTab === "people" ? "border-ink text-ink" : "border-transparent text-ink-muted hover:text-ink"}`}
              id="people-tab"
              type="button"
              aria-controls="people-panel"
            >
              Cast &amp; Crew
            </Tabs.Tab>
          </Tabs.List>
        ) : null}

        <Tabs.Panel value="films" keepMounted
          id={searching ? "films-panel" : undefined}
          role={searching ? "tabpanel" : undefined}
          aria-labelledby={searching ? "films-tab" : undefined}
          tabIndex={searching ? 0 : undefined}
          hidden={searching && activeSearchTab !== "films"}
        >

        {snapshot.catalog.status === "error" ? (
          <div className="mt-8 flex items-center justify-between gap-8 border-y border-rule py-[22px] max-[700px]:flex-col max-[700px]:items-start" role="alert">
            <p className="type-body">{snapshot.catalog.error}</p>
            <Button
              className={actionClass}
              type="button"
              onClick={() => searching
                ? void runtime?.retryMovieSearch()
                : void runtime?.loadPopularMovies()}
            >
              Try again
            </Button>
          </div>
        ) : null}

        {showSkeletons ? (
          <div
            className={catalogGridClass}
            aria-label={searching ? "Loading search results" : "Loading popular films"}
            aria-busy="true"
          >
            {Array.from({ length: 9 }, (_, index) => (
              <div className="relative aspect-[404/245] overflow-hidden bg-[linear-gradient(90deg,var(--theme-surface)_0%,var(--theme-surface-strong)_50%,var(--theme-surface)_100%)] bg-[length:200%_100%] animate-shimmer" key={index}>
                <div className="absolute inset-x-0 bottom-0 h-[46%] bg-gradient-to-b from-transparent to-[var(--theme-skeleton-fade)]" />
                <div className="absolute inset-x-[18px] bottom-3.5 z-10">
                  <div className="h-[11px] w-[58%] bg-inverse/40" />
                  <div className="mt-2 h-[11px] w-[28%] bg-inverse/40" />
                </div>
              </div>
            ))}
          </div>
        ) : null}

        {snapshot.catalog.status === "ready" && films.length === 0 ? (
          <div className="mt-8 flex items-center justify-between gap-8 border-y border-rule py-[22px] max-[700px]:flex-col max-[700px]:items-start">
            <p className="type-body">
              {searching && snapshot.catalog.query
                ? `No films found for “${snapshot.catalog.query}”.`
                : "No popular films are available right now."}
            </p>
            <Button className={actionClass} type="button" onClick={searching ? clearSearch : () => void runtime?.loadPopularMovies()}>
              {searching ? "Clear search" : "Refresh catalog"}
            </Button>
          </div>
        ) : null}

        {films.length > 0 ? (
          <div
            className={catalogGridClass}
            data-testid="film-grid"
          >
            {films.map((film) => (
              <FilmCard film={film} key={film.id} onOpen={() => openFilm(film)} />
            ))}
          </div>
        ) : null}

        {snapshot.catalog.status === "ready" && snapshot.catalog.page.hasMore ? (
          <div className="flex justify-center pt-16">
            <Button
              className={`${actionClass} min-w-[82px] disabled:cursor-wait disabled:text-disabled`}
              type="button"
              disabled={snapshot.catalog.loadingMore}
              onClick={() => void runtime?.loadNextPage()}
            >
              {snapshot.catalog.loadingMore ? "Loading" : "Load more"}
            </Button>
          </div>
        ) : null}
        </Tabs.Panel>

        {searching ? (
          <Tabs.Panel value="people" keepMounted
            id="people-panel"
            role="tabpanel"
            aria-labelledby="people-tab"
            tabIndex={0}
            hidden={activeSearchTab !== "people"}
          >
            {snapshot.people.status === "error" ? (
              <div className="mt-8 flex items-center justify-between gap-8 border-y border-rule py-[22px] max-[700px]:flex-col max-[700px]:items-start" role="alert">
                <p className="type-body">{snapshot.people.error}</p>
                <Button className={actionClass} type="button" onClick={() => void runtime?.retryPeopleSearch()}>Try again</Button>
              </div>
            ) : null}

            {snapshot.people.status === "loading" ? (
              <div className={peopleGridClass} aria-label="Loading cast and crew results" aria-busy="true">
                {Array.from({ length: 12 }, (_, index) => (
                  <div className="bg-canvas" key={index}>
                    <div className="aspect-[4/5] bg-[linear-gradient(90deg,var(--theme-surface)_0%,var(--theme-surface-strong)_50%,var(--theme-surface)_100%)] bg-[length:200%_100%] animate-shimmer" />
                    <div className="min-h-[92px] px-4 py-4">
                      <div className="h-4 w-[72%] bg-surface-strong" />
                      <div className="mt-2 h-3 w-[44%] bg-surface" />
                    </div>
                  </div>
                ))}
              </div>
            ) : null}

            {snapshot.people.status === "ready" && people.length === 0 ? (
              <div className="mt-8 flex items-center justify-between gap-8 border-y border-rule py-[22px] max-[700px]:flex-col max-[700px]:items-start">
                <p className="type-body">No cast or crew found for “{snapshot.people.query}”.</p>
                <Button className={actionClass} type="button" onClick={clearSearch}>Clear search</Button>
              </div>
            ) : null}

            {people.length > 0 ? (
              <div className={peopleGridClass} data-testid="people-grid">
                {people.map((person) => <CastCrewCard person={person} key={person.id} />)}
              </div>
            ) : null}

            {snapshot.people.status === "ready" && snapshot.people.page.hasMore ? (
              <div className="flex justify-center pt-16">
                <Button
                  className={`${actionClass} min-w-[82px] disabled:cursor-wait disabled:text-disabled`}
                  type="button"
                  disabled={snapshot.people.loadingMore}
                  onClick={() => void runtime?.loadNextPeoplePage()}
                >
                  {snapshot.people.loadingMore ? "Loading" : "Load more"}
                </Button>
              </div>
            ) : null}
          </Tabs.Panel>
        ) : null}
      </Tabs.Root>

      <footer className="w-full bg-surface">
        <div className="content-container flex items-end pt-[calc(200px+clamp(4.5rem,12vw,8rem))] pb-0">
          <span
            role="img"
            aria-label="Panorama"
            className="block w-full aspect-[1223/207] bg-[#FFFFFF] [mask-image:url('/panorama.svg')] [mask-size:100%_100%] [mask-repeat:no-repeat] [mask-position:center] [-webkit-mask-image:url('/panorama.svg')] [-webkit-mask-size:100%_100%] [-webkit-mask-repeat:no-repeat] [-webkit-mask-position:center]"
          />
        </div>
      </footer>

      <div className="sr-only-stable" role="status" aria-live="polite">
        {snapshot.account.status === "signedIn"
          ? `Signed in as ${snapshot.account.email}.`
          : null}
      </div>
      <div className="sr-only-stable" role="status" aria-live="polite">
        {snapshot.catalog.status === "loading"
          ? searching
            ? `Searching for ${snapshot.catalog.query}.`
            : "Loading popular films."
          : snapshot.catalog.status === "ready" && searching
            ? `${films.length} films found for ${snapshot.catalog.query}.`
            : null}
      </div>
      <div className="sr-only-stable" role="status" aria-live="polite">
        {searching && snapshot.people.status === "loading"
          ? `Searching cast and crew for ${snapshot.people.query}.`
          : searching && snapshot.people.status === "ready"
            ? `${people.length} cast and crew results found for ${snapshot.people.query}.`
            : null}
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
