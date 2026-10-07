import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { PlayerSettingsPopover } from "../../components/PlayerSettingsPopover";
import { RuntimeProvider } from "../../components/RuntimeMount";
import { PlayerPanelHost } from "./player-panel-host";
import { initialRuntimeSnapshot } from "../../runtime/snapshot";
import type { RuntimeSnapshot, StremioRuntime } from "../../runtime/types";

afterEach(() => { delete window.panoramaDesktop; });

it("offers an opt-in checkbox and saves Discord independently of playback settings", async () => {
  const setDiscordEnabled = vi.fn(async (enabled: boolean) => ({ enabled, available: true }));
  window.panoramaDesktop = {
    getCapabilities: vi.fn(), openExternal: vi.fn(),
    getDiscordSettings: vi.fn(async () => ({ enabled: false, available: true })),
    setDiscordEnabled,
  };
  render(<PlayerPanelHost><PlayerSettingsPopover onOpenChange={vi.fn()} onWillOpen={vi.fn()} /></PlayerPanelHost>);
  fireEvent.click(screen.getByRole("button", { name: "Playback settings" }));
  const checkbox = await screen.findByRole("checkbox", { name: "Share watching activity on Discord" });
  expect(checkbox).not.toBeChecked();
  fireEvent.click(checkbox);
  await waitFor(() => expect(checkbox).toBeChecked());
  expect(setDiscordEnabled).toHaveBeenCalledWith(true);
  fireEvent.click(checkbox);
  await waitFor(() => expect(checkbox).not.toBeChecked());
  expect(setDiscordEnabled).toHaveBeenLastCalledWith(false);
});

it("publishes playback only, excludes stale artwork, and clears on end and unmount", () => {
  const updateDiscordPlayback = vi.fn();
  window.panoramaDesktop = { getCapabilities: vi.fn(), openExternal: vi.fn(), updateDiscordPlayback };
  let snapshot: RuntimeSnapshot = { ...initialRuntimeSnapshot, player: { ...initialRuntimeSnapshot.player, status: "ready", filmId: "tmdb:1", title: "Film", time: 10, duration: 100, paused: false } };
  let notify = () => {};
  const unsubscribe = vi.fn();
  const runtime = { getSnapshot: () => snapshot, subscribe: (listener: () => void) => { notify = listener; return unsubscribe; } } as unknown as StremioRuntime;
  const view = render(<RuntimeProvider runtime={runtime}><span>Player</span></RuntimeProvider>);
  expect(updateDiscordPlayback).toHaveBeenLastCalledWith({ filmId: "tmdb:1", title: "Film", year: null, directorTmdbId: null, director: null, artwork: null, time: 10, duration: 100, state: "playing" });
  snapshot = { ...snapshot, details: { ...snapshot.details, metadata: { status: "ready", error: null, item: { id: "tmdb:1", type: "movie", name: "Film", year: "2014", director: null, originCountry: null, rating: null, ratingCount: null, posterUrl: null, landscapeUrl: null, logoUrl: null, description: null, originalTitle: null, alternativeTitle: null, runtime: null, genres: [] } } } };
  act(notify);
  expect(updateDiscordPlayback.mock.calls.at(-1)?.[0].year).toBe("2014");
  snapshot = { ...snapshot, player: { ...snapshot.player, filmId: "tmdb:2" } };
  act(notify);
  expect(updateDiscordPlayback.mock.calls.at(-1)?.[0].year).toBeNull();
  snapshot = { ...snapshot, player: { ...snapshot.player, paused: true } };
  act(notify);
  expect(updateDiscordPlayback.mock.calls.at(-1)?.[0].state).toBe("paused");
  snapshot = { ...snapshot, player: { ...snapshot.player, status: "ended" } };
  act(notify);
  expect(updateDiscordPlayback).toHaveBeenLastCalledWith(null);
  view.unmount();
  expect(unsubscribe).toHaveBeenCalled();
  expect(updateDiscordPlayback).toHaveBeenLastCalledWith(null);
});
