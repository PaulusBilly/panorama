import { StrictMode, useState } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PanoramaHome } from "@/components/PanoramaHome";
import { FakeRuntime } from "@/runtime/fake-runtime";

const { pushMock, replaceMock } = vi.hoisted(() => ({ pushMock: vi.fn(), replaceMock: vi.fn() }));
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: pushMock, replace: replaceMock, back: vi.fn() }),
}));

async function renderHome() {
  const runtime = new FakeRuntime();
  render(<StrictMode><PanoramaHome runtime={runtime} /></StrictMode>);
  await act(async () => runtime.initialize());
  return runtime;
}

function RoutedSearchHarness({ runtime, query = "Godard", category = "films" }: {
  runtime: FakeRuntime;
  query?: string;
  category?: "films" | "people";
}) {
  const [searchOpen, setSearchOpen] = useState(true);
  const props = {
    runtime,
    searchRoute: { query, category, searchOpen, onSearchOpenChange: setSearchOpen },
  } as unknown as Parameters<typeof PanoramaHome>[0];
  return <PanoramaHome {...props} />;
}

async function renderSearch(query = "Godard", category: "films" | "people" = "films") {
  const runtime = new FakeRuntime();
  await runtime.initialize();
  const search = vi.spyOn(runtime, "searchMovies");
  const view = render(<StrictMode><RoutedSearchHarness runtime={runtime} query={query} category={category} /></StrictMode>);
  return { runtime, search, view };
}

function scrollWindowTo(top: number) {
  Object.defineProperty(window, "scrollY", { configurable: true, value: top });
  fireEvent.scroll(window);
}

describe("Panorama homepage", () => {
  beforeEach(() => {
    pushMock.mockClear();
    replaceMock.mockClear();
    Object.defineProperty(window, "scrollY", { configurable: true, value: 0 });
  });

  it("starts the hero as a light grey skeleton", () => {
    render(<StrictMode><PanoramaHome runtime={new FakeRuntime()} /></StrictMode>);
    const hero = screen.getByRole("region", { name: "Featured film" });
    expect(hero).toHaveClass("desktop-hero-viewport", "min-h-dvh", "overflow-hidden");
    expect(hero).not.toHaveClass("h-dvh");
    expect(hero.querySelector(".content-container")).toHaveClass("desktop-hero-viewport", "min-h-dvh", "pb-10");
    expect(screen.getByTestId("hero-skeleton")).toHaveClass("bg-surface", "opacity-100");
  });

  it("renders the responsive catalog and shared header alignment", async () => {
    await renderHome();
    expect(await screen.findByRole("region", { name: "Popular films" })).toBeVisible();
    expect(screen.getAllByRole("article")).toHaveLength(9);
    expect(screen.getByTestId("film-grid")).toHaveClass("grid-cols-[repeat(4,23.25rem)]");
    const header = screen.getByRole("banner");
    expect(header).toHaveClass("z-40");
    expect(header.firstElementChild).toHaveClass("content-container");
    expect(within(header).getByRole("button", { name: "Search" })).toHaveClass("cursor-pointer");
    expect(within(header).getByRole("link", { name: "Panorama home" })).toHaveAttribute("href", "/");
    expect(within(header).getByRole("link", { name: "Panorama home" })).toHaveClass("cursor-pointer");
    const accountMenu = within(header).getByRole("button", { name: "Account menu" });
    expect(accountMenu.querySelector("svg")).toHaveAttribute("width", "22");
    expect(accountMenu.querySelector("svg")).toHaveAttribute("height", "22");
  });

  it("hides after scrolling down by its height and reveals on upward scroll", async () => {
    await renderHome();
    const header = screen.getByRole("banner");
    Object.defineProperty(header, "offsetHeight", { configurable: true, value: 60 });

    scrollWindowTo(59);
    expect(header).toHaveAttribute("data-hidden", "false");

    scrollWindowTo(61);
    expect(header).toHaveAttribute("data-hidden", "true");
    expect(header).toHaveAttribute("inert");

    scrollWindowTo(60);
    expect(header).toHaveAttribute("data-hidden", "false");
    expect(header).not.toHaveAttribute("inert");
  });

  it("keeps the revealed header transparent over the hero and white beyond it", async () => {
    await renderHome();
    const header = screen.getByRole("banner");
    const hero = screen.getByRole("region", { name: "Featured film" });
    Object.defineProperty(header, "offsetHeight", { configurable: true, value: 60 });
    const heroRect = vi.spyOn(hero.parentElement as HTMLElement, "getBoundingClientRect");

    heroRect.mockReturnValue({ top: -100, bottom: 700 } as DOMRect);
    scrollWindowTo(100);
    scrollWindowTo(99);
    expect(header).toHaveClass("bg-transparent");
    expect(header).not.toHaveClass("bg-canvas");

    heroRect.mockReturnValue({ top: -800, bottom: 40 } as DOMRect);
    scrollWindowTo(800);
    scrollWindowTo(799);
    expect(header).toHaveClass("bg-canvas", "text-ink");
    expect(header).not.toHaveClass("bg-transparent");
  });

  it("hides and reveals the expanded black search header while scrolling", async () => {
    await renderSearch();
    const header = screen.getByRole("banner");
    Object.defineProperty(header, "offsetHeight", { configurable: true, value: 60 });

    scrollWindowTo(100);

    expect(header).toHaveAttribute("data-hidden", "true");
    expect(header).toHaveClass("bg-player-canvas", "text-inverse");

    scrollWindowTo(99);

    expect(header).toHaveAttribute("data-hidden", "false");
    expect(header).toHaveClass("bg-player-canvas", "text-inverse");
  });

  it("keeps search visible through residual scroll from the opening click", async () => {
    await renderHome();
    const header = screen.getByRole("banner");
    Object.defineProperty(header, "offsetHeight", { configurable: true, value: 60 });

    scrollWindowTo(100);
    scrollWindowTo(99);
    fireEvent.click(within(header).getByRole("button", { name: "Search" }));
    scrollWindowTo(100);

    expect(header).toHaveAttribute("data-hidden", "false");
    expect(within(header).getByRole("button", { name: "Close search" })).toBeVisible();
  });

  it("reserves the measured fixed-header height on search routes", async () => {
    await renderSearch();
    const header = screen.getByRole("banner");
    Object.defineProperty(header, "offsetHeight", { configurable: true, value: 180 });

    scrollWindowTo(1);

    expect(header.parentElement).toHaveStyle({ paddingTop: "180px" });
  });

  it("tracks the desktop app viewport as its scroll container", async () => {
    document.body.classList.add("panorama-desktop");
    const viewport = document.createElement("div");
    viewport.className = "app-viewport";
    document.body.append(viewport);
    const runtime = new FakeRuntime();
    const view = render(<PanoramaHome runtime={runtime} />, { container: viewport });
    await act(async () => runtime.initialize());
    const header = screen.getByRole("banner");
    Object.defineProperty(header, "offsetHeight", { configurable: true, value: 60 });

    viewport.scrollTop = 61;
    fireEvent.scroll(viewport);

    expect(header).toHaveAttribute("data-hidden", "true");
    view.unmount();
    viewport.remove();
    document.body.classList.remove("panorama-desktop");
  });

  it("opens and completes the accessible sign-in flow", async () => {
    await renderHome();
    fireEvent.click(screen.getByRole("button", { name: "Account menu" }));
    fireEvent.click(screen.getByRole("button", { name: "Log In", hidden: true }));
    const dialog = screen.getByRole("dialog", { name: "Sign in to sync addons" });
    fireEvent.change(screen.getByLabelText("Email"), { target: { value: "viewer@example.com" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "password" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Sign in" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Account menu" })).toBeVisible());
  });

  it("keeps the signed-in account until Log out", async () => {
    const runtime = await renderHome();
    await act(async () => runtime.login("viewer@example.com", "password"));
    fireEvent.click(screen.getByRole("button", { name: "Account menu" }));
    fireEvent.click(screen.getByRole("button", { name: "Log Out", hidden: true }));
    fireEvent.click(screen.getByRole("button", { name: "Account menu" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Log In", hidden: true })).toBeInTheDocument());
  });

  it("closes the account popover when the page or a nested container scrolls", async () => {
    await renderHome();
    const accountMenu = screen.getByRole("button", { name: "Account menu" });
    const openPanel = () => fireEvent.click(accountMenu);
    openPanel();
    await waitFor(() => expect(accountMenu).toHaveAttribute("aria-expanded", "true"));
    fireEvent.scroll(window);
    await waitFor(() => expect(accountMenu).toHaveAttribute("aria-expanded", "false"));
    expect(screen.queryByRole("dialog", { name: "Account actions" })).not.toBeInTheDocument();

    openPanel();
    await waitFor(() => expect(accountMenu).toHaveAttribute("aria-expanded", "true"));
    const scroller = document.createElement("div");
    document.body.append(scroller);
    fireEvent.scroll(scroller);
    await waitFor(() => expect(accountMenu).toHaveAttribute("aria-expanded", "false"));
    expect(screen.queryByRole("dialog", { name: "Account actions" })).not.toBeInTheDocument();
    scroller.remove();
  });

  it("loads the next catalog page", async () => {
    await renderHome();
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    await waitFor(() => expect(screen.getAllByRole("article")).toHaveLength(18));
  });

  it("routes trimmed form submissions to encoded film search URLs", async () => {
    await renderHome();
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    const search = screen.getByRole("searchbox", { name: "Search movies" });
    fireEvent.change(search, { target: { value: "  A&B  " } });
    fireEvent.submit(screen.getByRole("search"));

    expect(pushMock).toHaveBeenCalledWith("/search/films?query=A%26B");
  });

  it("restores a routed query and its selected result category", async () => {
    const { search } = await renderSearch("Godard", "people");

    await waitFor(() => expect(screen.getByRole("region", { name: "Search results for Godard" })).toBeVisible());
    expect(search).toHaveBeenCalledTimes(1);
    expect(search).toHaveBeenCalledWith("Godard");
    expect(screen.getByRole("searchbox", { name: "Search movies" })).toHaveValue("Godard");
    expect(screen.getByRole("tab", { name: "Cast & Crew" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("people-grid")).toBeVisible();
  });

  it("removes the hero and routes result category tabs without repeating the search", async () => {
    const { runtime, search, view } = await renderSearch("Godard", "films");
    await waitFor(() => expect(screen.queryByRole("region", { name: "Featured film" })).not.toBeInTheDocument());
    const filmsTab = screen.getByRole("tab", { name: "Films" });
    const peopleTab = screen.getByRole("tab", { name: "Cast & Crew" });
    expect(filmsTab).toHaveAttribute("aria-selected", "true");
    expect(peopleTab).toHaveAttribute("aria-selected", "false");

    fireEvent.click(peopleTab);
    expect(pushMock).toHaveBeenCalledWith("/search/people?query=Godard");
    view.rerender(<StrictMode><RoutedSearchHarness runtime={runtime} query="Godard" category="people" /></StrictMode>);
    expect(search).toHaveBeenCalledTimes(1);
    expect(peopleTab).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("people-grid")).toHaveClass("grid-cols-6", "max-[1640px]:grid-cols-5");
    expect(screen.getAllByRole("article")).toHaveLength(6);
    expect(screen.getByRole("heading", { name: "Jean-Luc Godard" })).toBeVisible();
    expect(screen.getByText("Directing")).toBeVisible();
    expect(screen.queryByRole("button", { name: /Jean-Luc Godard/i })).not.toBeInTheDocument();
  });

  it("supports roving keyboard focus across search result tabs", async () => {
    await renderSearch("Godard", "films");
    const filmsTab = await screen.findByRole("tab", { name: "Films" });
    const peopleTab = screen.getByRole("tab", { name: "Cast & Crew" });

    fireEvent.keyDown(filmsTab, { key: "ArrowRight" });
    await waitFor(() => expect(peopleTab).toHaveFocus());
    expect(pushMock).toHaveBeenCalledWith("/search/people?query=Godard");
    fireEvent.keyDown(peopleTab, { key: "Home" });
    await waitFor(() => expect(filmsTab).toHaveFocus());
    fireEvent.keyDown(filmsTab, { key: "End" });
    await waitFor(() => expect(peopleTab).toHaveFocus());
  });

  it("closes the search band without clearing its query or results", async () => {
    const { runtime } = await renderSearch("Aftersun");
    await waitFor(() => expect(screen.getByRole("region", { name: "Search results for Aftersun" })).toBeVisible());

    fireEvent.click(screen.getByRole("button", { name: "Close search" }));

    expect(screen.getByRole("region", { name: "Search results for Aftersun" })).toBeVisible();
    expect(runtime.getSnapshot().catalog.query).toBe("Aftersun");
    expect(replaceMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(screen.getByRole("searchbox", { name: "Search movies" })).toHaveValue("Aftersun");
  });

  it("clears routed search with Escape and replaces the URL with home", async () => {
    const { runtime } = await renderSearch("Aftersun");
    const search = await screen.findByRole("searchbox", { name: "Search movies" });

    fireEvent.keyDown(search, { key: "Escape" });

    expect(replaceMock).toHaveBeenCalledWith("/");
    await waitFor(() => expect(runtime.getSnapshot().catalog.mode).toBe("popular"));
  });

  it("returns home from the explicit empty-results Clear search action", async () => {
    const { runtime } = await renderSearch("NoMatch");
    const clear = await screen.findByRole("button", { name: "Clear search" });

    fireEvent.click(clear);

    expect(replaceMock).toHaveBeenCalledWith("/");
    await waitFor(() => expect(runtime.getSnapshot().catalog.mode).toBe("popular"));
  });

  it("returns home when an empty routed query is submitted", async () => {
    await renderSearch("Aftersun");
    const search = await screen.findByRole("searchbox", { name: "Search movies" });
    fireEvent.change(search, { target: { value: "   " } });

    fireEvent.submit(screen.getByRole("search"));

    expect(replaceMock).toHaveBeenCalledWith("/");
  });

  it("opens the expanded search band with the previous display treatment", async () => {
    await renderHome();
    const header = screen.getByRole("banner");

    fireEvent.click(within(header).getByRole("button", { name: "Search" }));

    const close = within(header).getByRole("button", { name: "Close search" });
    const form = within(header).getByRole("search");
    const search = within(form).getByRole("searchbox", { name: "Search movies" });
    const submit = within(form).getByRole("button", { name: "Submit search" });
    expect(close.parentElement).not.toBe(form.parentElement);
    expect(form).toHaveClass("content-container", "py-10", "max-[700px]:py-8");
    expect(search).toHaveClass("w-full", "min-h-14", "border-b", "type-display", "pr-20");
    expect(submit).toHaveClass("absolute", "right-0", "bottom-3");
    expect(submit.querySelector("svg")).toHaveAttribute("width", "52");
    expect(submit.querySelector("svg")).toHaveAttribute("height", "52");
    expect(within(header).queryByRole("link", { name: "Panorama home" })).not.toBeInTheDocument();
    expect(within(header).queryByRole("button", { name: "Account menu" })).not.toBeInTheDocument();
    expect(search).toHaveFocus();

    fireEvent.click(close);
    expect(search).toHaveAttribute("tabindex", "-1");
    expect(submit).toHaveAttribute("tabindex", "-1");
  });

  it("keeps the hero background non-interactive and navigates from its Watch CTA and film cards", async () => {
    await renderHome();
    const hero = screen.getByRole("region", { name: "Featured film" });
    const watch = within(hero).getByRole("button", { name: "WATCH Aftersun" });
    expect(watch).toHaveClass("cursor-pointer", "transition-colors", "hover:bg-[transparent]", "hover:text-[var(--theme-inverse)]");
    expect(within(hero).getAllByRole("button")).toHaveLength(1);
    fireEvent.click(watch);
    expect(pushMock).toHaveBeenCalledWith("/films/100/watch");
    pushMock.mockClear();
    fireEvent.click(screen.getAllByRole("button", { name: "Open details for Perfect Days" })[0]);
    expect(pushMock).toHaveBeenCalledWith("/films/101");
  });

  it("uses a TMDB logo within the hero title column", async () => {
    const runtime = await renderHome();
    const catalog = runtime.getSnapshot().catalog;
    act(() => runtime.setStateForTest({
      catalog: {
        ...catalog,
        page: {
          ...catalog.page,
          items: catalog.page.items.map((film, index) => index === 0
            ? { ...film, logoUrl: "https://image.tmdb.org/t/p/w500/aftersun-logo.png" }
            : film),
        },
      },
    }));

    const hero = screen.getByRole("region", { name: "Featured film" });
    expect(within(hero).getByRole("heading", { name: "Aftersun" })).toHaveClass("sr-only");
    expect(within(hero).getByTestId("film-title-logo")).toHaveClass(
      "max-h-32",
      "max-w-[310px]",
      "object-contain",
      "object-left",
    );
  });
});
