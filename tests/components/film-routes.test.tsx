import { StrictMode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FilmDetailsPage } from "@/components/FilmDetailsPage";
import { FilmRuntimeScope } from "@/components/FilmRuntimeScope";
import { FilmWatchPage } from "@/components/FilmWatchPage";
import PlayerDialog from "@/components/PlayerDialog";
import { RuntimeProvider } from "@/components/RuntimeMount";
import { FakeRuntime } from "@/runtime/fake-runtime";

const { pushMock, replaceMock, backMock } = vi.hoisted(() => ({
  pushMock: vi.fn(),
  replaceMock: vi.fn(),
  backMock: vi.fn(),
}));

vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: pushMock, replace: replaceMock, back: backMock }),
}));

async function readyFilm(id = "tmdb:100") {
  const runtime = new FakeRuntime();
  await runtime.initialize();
  await runtime.login("viewer@example.com", "password");
  await runtime.checkService();
  await runtime.openFilmDetails(id);
  return runtime;
}

function scrollWindowTo(top: number) {
  Object.defineProperty(window, "scrollY", { configurable: true, value: top });
  fireEvent.scroll(window);
}

beforeEach(() => {
  pushMock.mockClear();
  replaceMock.mockClear();
  backMock.mockClear();
  Object.defineProperty(window, "scrollY", { configurable: true, value: 0 });
});

describe("film detail route", () => {
  it("uses the homepage hero surface color while initial metadata is loading", async () => {
    const runtime = new FakeRuntime();
    await runtime.initialize();
    const details = runtime.getSnapshot().details;
    runtime.setStateForTest({
      details: {
        ...details,
        metadata: { status: "loading", item: null, error: null },
      },
    });
    render(<RuntimeProvider runtime={runtime}><FilmDetailsPage /></RuntimeProvider>);

    const page = screen.getByRole("main");
    expect(page).toHaveClass("desktop-hero-viewport", "min-h-dvh", "overflow-x-hidden", "bg-surface");
    expect(page).not.toHaveClass("h-dvh", "overflow-hidden", "bg-artwork-empty");
    expect(page.querySelector(".content-container.flex-col")).toHaveClass("desktop-hero-viewport", "min-h-dvh", "pb-10");
  });

  it("starts at the top of the desktop page viewport", async () => {
    const runtime = new FakeRuntime();
    await runtime.initialize();
    const viewport = document.createElement("div");
    viewport.className = "app-viewport";
    viewport.scrollTop = 480;
    document.body.append(viewport);

    const view = render(<RuntimeProvider runtime={runtime}><FilmDetailsPage /></RuntimeProvider>, { container: viewport });

    expect(viewport.scrollTop).toBe(0);
    view.unmount();
    viewport.remove();
  });

  it("does not clear a newly opened film during Strict Mode effect replay", async () => {
    const runtime = await readyFilm();
    const open = vi.spyOn(runtime, "openFilmDetails");
    const close = vi.spyOn(runtime, "closeFilmDetails");
    const view = render(
      <StrictMode>
        <RuntimeProvider runtime={runtime}>
          <FilmRuntimeScope filmId="tmdb:100"><FilmDetailsPage /></FilmRuntimeScope>
        </RuntimeProvider>
      </StrictMode>,
    );

    await waitFor(() => expect(open).toHaveBeenCalled());
    expect(close).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Aftersun" })).toBeVisible();

    view.unmount();
    await waitFor(() => expect(close).toHaveBeenCalledTimes(1));
  });

  it("renders TMDB score, count, source quality, and the restrained detail fields", async () => {
    const runtime = await readyFilm();
    render(<RuntimeProvider runtime={runtime}><FilmDetailsPage /></RuntimeProvider>);

    const header = screen.getByRole("banner");
    expect(header).toHaveClass("z-40");
    expect(header.firstElementChild).toHaveClass("content-container", "grid-cols-[1fr_auto_1fr]", "pt-5");
    expect(within(header).getByRole("button", { name: "Back to films" })).toHaveClass("cursor-pointer");
    expect(within(header).getByRole("link", { name: "Panorama home" })).toHaveAttribute("href", "/");
    expect(within(header).getByRole("link", { name: "Panorama home" })).toHaveClass("cursor-pointer");
    expect(screen.getByRole("main")).toHaveClass("bg-artwork-empty");
    expect(screen.getByRole("heading", { name: "Aftersun" })).toHaveClass("text-[32px]", "leading-[0.9]", "uppercase");
    const directorLabel = screen.getByText("Directed by", { selector: "span" });
    expect(directorLabel.parentElement?.parentElement).toHaveClass("uppercase");
    expect(screen.getByLabelText("TMDB score 7.6 out of 10 from 2,485 ratings")).toBeVisible();
    expect(screen.getByText("2,485 ratings")).toBeVisible();
    const quality = screen.getByLabelText("Quality: HD");
    const audioChannels = screen.getByLabelText("Audio: 5.1");
    const genre = screen.getByText("Drama");
    const duration = screen.getByText("102 min", { selector: "span" });
    expect(quality).toBeVisible();
    expect(audioChannels).toBeVisible();
    expect(genre).toBeVisible();
    expect(genre.parentElement).not.toBe(quality.parentElement);
    expect(quality.parentElement).toContainElement(duration);
    expect(quality).toHaveTextContent("HD");
    expect(quality).toHaveClass("border", "border-inverse/65", "px-1.5", "py-0.5", "text-[11px]", "font-bold", "uppercase");
    expect(quality.querySelector("svg")).not.toBeInTheDocument();
    expect(audioChannels).toHaveTextContent("5.1");
    expect(audioChannels).toHaveClass("border", "border-inverse/65", "px-1.5", "py-0.5", "text-[11px]", "font-bold", "uppercase");
    expect(duration.querySelector("svg")).not.toBeInTheDocument();
    const synopsisHeading = screen.getByRole("heading", { name: "Synopsis" });
    const informationRow = synopsisHeading.parentElement?.parentElement;
    expect(synopsisHeading).toBeVisible();
    expect(informationRow).toHaveClass("grid-cols-[310px_minmax(0,1fr)]");
    expect(informationRow?.firstElementChild).not.toHaveClass("pe-[70px]");
    expect(informationRow?.firstElementChild).toContainElement(directorLabel);
    expect(informationRow?.firstElementChild).toContainElement(genre);
    expect(screen.queryByText("Our Take")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /trailer|share|favorite/i })).not.toBeInTheDocument();
  });

  it("uses the shared directional header while remaining transparent over its hero", async () => {
    const runtime = await readyFilm();
    render(<RuntimeProvider runtime={runtime}><FilmDetailsPage /></RuntimeProvider>);
    const header = screen.getByRole("banner");
    const hero = screen.getByRole("main");
    Object.defineProperty(header, "offsetHeight", { configurable: true, value: 60 });
    vi.spyOn(hero, "getBoundingClientRect").mockReturnValue({ top: -100, bottom: 700 } as DOMRect);

    scrollWindowTo(100);
    expect(header).toHaveAttribute("data-hidden", "true");

    scrollWindowTo(99);
    expect(header).toHaveAttribute("data-hidden", "false");
    expect(header).toHaveClass("bg-transparent");
    expect(header).not.toHaveClass("bg-canvas");
  });

  it("shows foreign origin and native title while suppressing an incomplete score", async () => {
    const runtime = await readyFilm("tmdb:101");
    const details = runtime.getSnapshot().details;
    runtime.setStateForTest({
      details: {
        ...details,
        metadata: {
          ...details.metadata,
          item: details.metadata.item ? {
            ...details.metadata.item,
            rating: null,
            logoUrl: "https://image.tmdb.org/t/p/w500/perfect-days-logo.png",
          } : null,
        },
      },
    });
    render(<RuntimeProvider runtime={runtime}><FilmDetailsPage /></RuntimeProvider>);

    const englishTitle = screen.getByRole("heading", { name: "Perfect Days" });
    const logo = screen.getByTestId("film-title-logo");
    const nativeTitle = screen.getByText("パーフェクト・デイズ");
    const synopsisHeading = screen.getByRole("heading", { name: "Synopsis" });
    const informationRow = synopsisHeading.parentElement?.parentElement;
    const directorLabel = screen.getByText("Directed by", { selector: "span" });
    expect(nativeTitle).toHaveClass("mt-[10px]", "text-[16px]", "text-inverse");
    expect(englishTitle).toHaveClass("sr-only");
    expect(logo).toHaveClass("max-h-32", "max-w-[310px]", "object-contain", "object-left");
    expect(englishTitle.parentElement).toContainElement(nativeTitle);
    expect(informationRow?.firstElementChild).toContainElement(directorLabel);
    expect(informationRow?.firstElementChild).not.toContainElement(nativeTitle);
    expect(screen.getByText("Japan")).toBeVisible();
    expect(screen.queryByLabelText(/TMDB score/)).not.toBeInTheDocument();
  });

  it("prefers a foreign full onscreen alternative title over the original title", async () => {
    const runtime = await readyFilm("tmdb:101");
    const details = runtime.getSnapshot().details;
    runtime.setStateForTest({
      details: {
        ...details,
        metadata: {
          ...details.metadata,
          item: details.metadata.item ? {
            ...details.metadata.item,
            name: "Masculin Féminin",
            originalTitle: "Masculin féminin",
            alternativeTitle: "Masculin féminin: 15 faits précis",
            originCountry: "France",
          } : null,
        },
      },
    });
    render(<RuntimeProvider runtime={runtime}><FilmDetailsPage /></RuntimeProvider>);

    expect(screen.getByText("Masculin féminin: 15 faits précis")).toBeVisible();
    expect(screen.queryByText("Masculin féminin", { exact: true })).not.toBeInTheDocument();
  });

  it("prepares the default source and navigates to watch without saved progress", async () => {
    const runtime = await readyFilm();
    const prepare = vi.spyOn(runtime, "preparePlayback");
    const start = vi.spyOn(runtime, "startPlayback");
    const details = runtime.getSnapshot().details;
    runtime.setStateForTest({ details: { ...details, resume: { available: false, offset: 0, duration: null } } });
    render(<RuntimeProvider runtime={runtime}><FilmDetailsPage /></RuntimeProvider>);

    await waitFor(() => expect(prepare).toHaveBeenCalledWith("source-1-1"));
    const playButton = screen.getByRole("button", { name: "Play" });
    expect(playButton).toHaveClass("cursor-pointer", "disabled:cursor-not-allowed");
    fireEvent.click(playButton);
    expect(start).not.toHaveBeenCalled();
    expect(pushMock).toHaveBeenCalledWith("/films/100/watch");
  });

  it("prepares the default source and navigates to resume with time left", async () => {
    const runtime = await readyFilm();
    const prepare = vi.spyOn(runtime, "preparePlayback");
    const start = vi.spyOn(runtime, "startPlayback");
    const details = runtime.getSnapshot().details;
    runtime.setStateForTest({ details: { ...details, resume: { available: true, offset: 120, duration: 6120 } } });
    render(<RuntimeProvider runtime={runtime}><FilmDetailsPage /></RuntimeProvider>);

    await waitFor(() => expect(prepare).toHaveBeenCalledWith("source-1-1"));
    fireEvent.click(screen.getByRole("button", { name: "Resume, 1h 40m left" }));

    expect(start).not.toHaveBeenCalled();
    expect(pushMock).toHaveBeenCalledWith("/films/100/watch?resume=1");
  });

  it("offers sign in on logged-out film details", async () => {
    const runtime = new FakeRuntime();
    await runtime.initialize();
    await runtime.openFilmDetails("tmdb:100");
    render(<RuntimeProvider runtime={runtime}><FilmDetailsPage /></RuntimeProvider>);

    fireEvent.click(screen.getByRole("button", { name: "Sign in to watch" }));
    expect(screen.getByRole("dialog", { name: "Sign in to sync addons" })).toBeVisible();
  });

  it("adds and removes the film from the watchlist beside the play pill", async () => {
    const runtime = await readyFilm();
    render(<RuntimeProvider runtime={runtime}><FilmDetailsPage /></RuntimeProvider>);

    fireEvent.click(screen.getByRole("button", { name: "Add to Watchlist" }));
    expect(runtime.getSnapshot().details.watchlist.saved).toBe(true);

    fireEvent.click(await screen.findByRole("button", { name: "Remove from Watchlist" }));
    expect(runtime.getSnapshot().details.watchlist.saved).toBe(false);
    expect(await screen.findByRole("button", { name: "Add to Watchlist" })).toBeEnabled();
  });

  it("asks a signed-out viewer to sign in before adding to the watchlist", async () => {
    const runtime = new FakeRuntime();
    await runtime.initialize();
    await runtime.openFilmDetails("tmdb:100");
    const setWatchlisted = vi.spyOn(runtime, "setWatchlisted");
    render(<RuntimeProvider runtime={runtime}><FilmDetailsPage /></RuntimeProvider>);

    fireEvent.click(screen.getByRole("button", { name: "Add to Watchlist" }));
    expect(setWatchlisted).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Sign in to sync addons" })).toBeVisible();
  });

  it("keeps the first playable source stable while other addons are still loading", async () => {
    const runtime = await readyFilm();
    const prepare = vi.spyOn(runtime, "preparePlayback");
    const details = runtime.getSnapshot().details;
    runtime.setStateForTest({
      details: {
        ...details,
        sources: { ...details.sources, status: "loading" },
      },
    });
    render(<RuntimeProvider runtime={runtime}><FilmDetailsPage /></RuntimeProvider>);

    await waitFor(() => expect(prepare).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("button", { name: "Play" })).toBeEnabled();
    expect(screen.getByLabelText("Quality: HD")).toBeVisible();

    act(() => runtime.setStateForTest({
      details: {
        ...runtime.getSnapshot().details,
        sources: {
          ...runtime.getSnapshot().details.sources,
          groups: runtime.getSnapshot().details.sources.groups.map((group) => ({
            ...group,
            items: group.items.map((source) => ({ ...source })),
          })),
        },
      },
    }));
    expect(prepare).toHaveBeenCalledTimes(1);
  });

  it("keeps alternate sources out of details and prepares only the first playable source", async () => {
    const runtime = await readyFilm();
    const details = runtime.getSnapshot().details;
    const firstGroup = details.sources.groups[0];
    if (!firstGroup) throw new Error("Missing source fixture");
    runtime.setStateForTest({
      details: {
        ...details,
        sources: {
          ...details.sources,
          groups: [{
            ...firstGroup,
            addonId: "com.stremio.aiostreams",
            addonName: "AIOStreams",
            items: [
              { ...firstGroup.items[0], addonId: "com.stremio.aiostreams", addonName: "AIOStreams" },
              {
                ...firstGroup.items[0],
                id: "source-1-2",
                addonId: "com.stremio.aiostreams",
                addonName: "AIOStreams",
                name: "1080p alternate",
                description: "20 GB · H.264",
              },
            ],
          }],
        },
      },
    });
    const prepare = vi.spyOn(runtime, "preparePlayback");
    const start = vi.spyOn(runtime, "startPlayback");
    render(<RuntimeProvider runtime={runtime}><FilmDetailsPage /></RuntimeProvider>);

    expect(screen.queryByRole("heading", { name: "Sources" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /1080p alternate/i })).not.toBeInTheDocument();
    await waitFor(() => expect(prepare).toHaveBeenCalledWith("source-1-1"));
    fireEvent.click(screen.getByRole("button", { name: "Play" }));

    expect(start).not.toHaveBeenCalled();
    expect(pushMock).toHaveBeenCalledWith("/films/100/watch");
  });
});

describe("film watch route", () => {
  it("shows the Cascade loader while a signed-in direct watch visit waits for film loading to begin", async () => {
    const runtime = new FakeRuntime();
    await runtime.initialize();
    await runtime.login("viewer@example.com", "password");
    await runtime.checkService();

    render(<RuntimeProvider runtime={runtime}><FilmWatchPage /></RuntimeProvider>);

    expect(screen.getByTestId("cascade-loader")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Back to film details" })).not.toBeInTheDocument();
  });

  it("keeps flushed resume state available when returning to film details", async () => {
    const runtime = await readyFilm();
    await runtime.startPlayback("source-1-1", "restart");
    await runtime.attachPlayer();
    runtime.seekPlayback(2520);

    runtime.flushPlaybackProgress();

    expect(runtime.getSnapshot().details.resume).toEqual({
      available: true,
      offset: 2520,
      duration: 6120,
    });
  });

  it("shows only the centered Cascade loader while eligible playback startup is pending", async () => {
    const runtime = await readyFilm();
    vi.spyOn(runtime, "startPlayback").mockResolvedValue();
    render(<RuntimeProvider runtime={runtime}><FilmWatchPage /></RuntimeProvider>);

    expect(screen.getByTestId("cascade-loader")).toBeVisible();
    expect(screen.getByRole("status", { name: "Preparing playback" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Back to film details" })).not.toBeInTheDocument();
    expect(screen.queryByText("Aftersun", { selector: "h1" })).not.toBeInTheDocument();
  });

  it("resumes a previously played film when the watch URL requests resume", async () => {
    const originalUrl = window.location.href;
    window.history.replaceState(null, "", "/films/100/watch?resume=1");
    try {
      const runtime = await readyFilm();
      const details = runtime.getSnapshot().details;
      runtime.setStateForTest({ details: { ...details, resume: { available: true, offset: 120, duration: 6120 } } });
      const start = vi.spyOn(runtime, "startPlayback");
      render(<RuntimeProvider runtime={runtime}><FilmWatchPage /></RuntimeProvider>);

      await waitFor(() => expect(start).toHaveBeenCalledWith("source-1-1", "resume"));
    } finally {
      window.history.replaceState(null, "", originalUrl);
    }
  });

  it("starts playback when a ready film mounts under Strict Mode", async () => {
    const runtime = await readyFilm();
    render(<StrictMode><RuntimeProvider runtime={runtime}><FilmWatchPage /></RuntimeProvider></StrictMode>);

    await waitFor(() => expect(runtime.getSnapshot().player.status).not.toBe("idle"));
  });

  it("starts a previously played film from the beginning", async () => {
    const runtime = await readyFilm();
    const details = runtime.getSnapshot().details;
    runtime.setStateForTest({ details: { ...details, resume: { available: true, offset: 120, duration: 6120 } } });
    const start = vi.spyOn(runtime, "startPlayback");
    render(<RuntimeProvider runtime={runtime}><FilmWatchPage /></RuntimeProvider>);

    await waitFor(() => expect(start).toHaveBeenCalledWith("source-1-1", "restart"));
    expect(screen.queryByText("Start playback")).not.toBeInTheDocument();
  });

  it("offers sign in on a direct logged-out watch visit", async () => {
    const runtime = new FakeRuntime();
    await runtime.initialize();
    await runtime.openFilmDetails("tmdb:100");
    render(<RuntimeProvider runtime={runtime}><FilmWatchPage /></RuntimeProvider>);

    fireEvent.click(screen.getByRole("button", { name: "Sign in to watch" }));
    expect(screen.getByRole("dialog", { name: "Sign in to sync addons" })).toBeVisible();
  });

  it("opens the sources popover and switches immediately while preserving position", async () => {
    const runtime = await readyFilm();
    const snapshot = runtime.getSnapshot();
    const firstGroup = snapshot.details.sources.groups[0];
    runtime.setStateForTest({
      details: {
        ...snapshot.details,
        sources: {
          ...snapshot.details.sources,
          groups: firstGroup ? [{
            ...firstGroup,
            items: [...firstGroup.items, {
              ...firstGroup.items[0],
              id: "source-1-3",
              name: "720p alternate",
            }],
          }] : [],
        },
      },
    });
    await runtime.startPlayback("source-1-1", "restart");
    const attach = runtime.attachPlayer.bind(runtime);
    const attachSpy = vi.spyOn(runtime, "attachPlayer").mockResolvedValue();
    const switchSource = vi.spyOn(runtime, "switchPlaybackSource");
    render(<RuntimeProvider runtime={runtime}><FilmWatchPage /></RuntimeProvider>);

    expect(screen.getByTestId("cascade-loader")).toBeVisible();
    expect(screen.queryByTestId("player-controls-overlay")).not.toBeInTheDocument();
    const waitingForTracks = runtime.getSnapshot();
    act(() => runtime.setStateForTest({
      player: {
        ...waitingForTracks.player,
        status: "ready",
        stage: null,
        trackDiscoveryReady: false,
        audio: { ...waitingForTracks.player.audio, status: "ready" },
        subtitles: { ...waitingForTracks.player.subtitles, status: "ready" },
      },
    }));
    expect(screen.getByTestId("player-controls-overlay")).toBeVisible();
    attachSpy.mockImplementation(attach);
    await act(async () => {
      await runtime.attachPlayer();
    });
    await waitFor(() => expect(runtime.getSnapshot().player.status).toBe("ready"));
    await waitFor(() => expect(screen.getByTestId("player-controls-overlay")).toBeVisible());
    act(() => runtime.seekPlayback(145));
    fireEvent.click(await screen.findByRole("button", { name: "Sources" }));
    expect(screen.getByRole("dialog", { name: "Sources" })).toHaveAttribute("id", "player-sources");
    expect(screen.getByRole("button", { name: "Sources" })).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(screen.getByRole("radio", { name: /720p alternate/i }));
    expect(switchSource).toHaveBeenCalledWith("source-1-3");
    await waitFor(() => expect(runtime.getSnapshot().player.time).toBe(145));
  });

  it("hides the controls and pointer after three seconds of pointer inactivity", async () => {
    const runtime = await readyFilm();
    await runtime.startPlayback("source-1-1", "restart");
    render(<RuntimeProvider runtime={runtime}><FilmWatchPage /></RuntimeProvider>);

    await waitFor(() => expect(runtime.getSnapshot().player.status).toBe("ready"));
    const surface = screen.getByTestId("player-surface");
    vi.useFakeTimers();
    try {
      fireEvent.pointerMove(surface);
      expect(surface).not.toHaveClass("cursor-none");
      expect(screen.getByTestId("player-controls-overlay")).toBeVisible();

      act(() => vi.advanceTimersByTime(2_999));
      expect(surface).not.toHaveClass("cursor-none");
      expect(screen.getByTestId("player-controls-overlay")).toBeVisible();

      act(() => vi.advanceTimersByTime(1));
      expect(surface).toHaveClass("cursor-none");
      expect(screen.queryByTestId("player-controls-overlay")).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows only the Cascade loader during initial player preparation", async () => {
    const runtime = await readyFilm();
    await runtime.startPlayback("source-1-1", "restart");
    const snapshot = runtime.getSnapshot();
    const preparingSnapshot = {
      ...snapshot,
      player: {
        ...snapshot.player,
        status: "preparing",
        stage: "loadingVideo",
        trackDiscoveryReady: false,
      },
    } as const;
    render(<PlayerDialog runtime={runtime} snapshot={preparingSnapshot} onClose={() => undefined} />);

    expect(screen.getByTestId("cascade-loader")).toBeVisible();
    expect(screen.getByRole("status", { name: "Preparing playback" })).toBeVisible();
    expect(screen.queryByText("Aftersun", { selector: "p" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Close" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("player-controls-overlay")).not.toBeInTheDocument();
    expect(screen.getByTestId("player-surface")).not.toHaveClass("cursor-none");
  });

  it("keeps the existing preparation feedback after playback was already ready", async () => {
    const runtime = await readyFilm();
    await runtime.startPlayback("source-1-1", "restart");
    const snapshot = runtime.getSnapshot();
    const readySnapshot = {
      ...snapshot,
      player: {
        ...snapshot.player,
        status: "ready",
        stage: null,
        trackDiscoveryReady: true,
      },
    } as const;
    const { rerender } = render(<PlayerDialog runtime={runtime} snapshot={readySnapshot} onClose={() => undefined} />);
    const preparingSnapshot = {
      ...readySnapshot,
      player: {
        ...readySnapshot.player,
        status: "preparing",
        stage: "loadingVideo",
      },
    } as const;

    rerender(<PlayerDialog runtime={runtime} snapshot={preparingSnapshot} onClose={() => undefined} />);

    expect(screen.queryByTestId("cascade-loader")).not.toBeInTheDocument();
    expect(screen.getByText("Loading video")).toBeVisible();
    expect(screen.getByRole("button", { name: "Close" })).toBeVisible();
  });
});
