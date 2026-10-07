import { EventEmitter } from "node:events";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createConnection } from "node:net";
import { DiscordPresence, discordActivity, discordFrame, discordPaths } from "../../desktop/main/discord-presence";
import { discordArtwork, parseDiscordPlayback, type DiscordPlayback } from "../../desktop/shared/discord-presence";

vi.mock("node:net", () => {
  const mocked = { createConnection: vi.fn() };
  return { ...mocked, default: mocked };
});

const playback: DiscordPlayback = { filmId: "tmdb:157336", title: "Interstellar", director: "Christopher Nolan", artwork: "https://image.tmdb.org/t/p/w1280/test.jpg", time: 60, duration: 600, state: "playing" };
let directory: string;
let presence: DiscordPresence;
let sockets: Array<EventEmitter & { write: ReturnType<typeof vi.fn<(frame: Buffer) => void>>; end: ReturnType<typeof vi.fn<() => void>>; destroy: ReturnType<typeof vi.fn<() => void>> }>;

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(1_000_000);
  directory = mkdtempSync(join(tmpdir(), "panorama-discord-"));
  sockets = [];
  vi.mocked(createConnection).mockImplementation(() => {
    const socket = Object.assign(new EventEmitter(), {
      write: vi.fn<(frame: Buffer) => void>(), end: vi.fn<() => void>(), destroy: vi.fn<() => void>(), unref: vi.fn(), setTimeout: vi.fn(),
    });
    socket.destroy.mockImplementation(() => socket.emit("close"));
    sockets.push(socket);
    return socket as unknown as ReturnType<typeof createConnection>;
  });
  presence = new DiscordPresence(join(directory, "settings.json"), ["test-socket"]);
});

afterEach(() => { presence.clear(); vi.useRealTimers(); vi.clearAllMocks(); rmSync(directory, { recursive: true, force: true }); });

function ready() {
  const socket = sockets.at(-1)!;
  socket.emit("connect");
  const frame = discordFrame(1, { evt: "READY" });
  socket.emit("data", frame.subarray(0, 5));
  socket.emit("data", frame.subarray(5));
  vi.advanceTimersByTime(0);
  return socket;
}

function messages(socket = sockets.at(-1)!) {
  return socket.write.mock.calls.map(([frame]: [Buffer]) => JSON.parse(frame.subarray(8).toString()));
}

it("maps watching, artwork and seek timing without advancing paused or buffering activities", () => {
  const datedPlayback = parseDiscordPlayback({ ...playback, year: "2014" })!;
  expect(discordActivity(datedPlayback, 1_000_000).details_url).toBe("https://www.themoviedb.org/movie/157336");
  for (const filmId of ["tt0816692", "tmdb:123/other", "tmdb:0"]) expect(discordActivity({ ...playback, filmId }, 1_000_000)).not.toHaveProperty("details_url");
  expect(discordActivity(datedPlayback, 1_000_000)).toMatchObject({ name: "in Panorama", state: "dir. Christopher Nolan", details: "Interstellar (2014)", status_display_type: 2 });
  expect(discordActivity({ ...datedPlayback, title: "A".repeat(128) }, 1_000_000).details).toBe(`${"A".repeat(121)} (2014)`);
  expect(() => parseDiscordPlayback({ ...playback, year: 2014 })).toThrow();
  const linkedPlayback = parseDiscordPlayback({ ...playback, directorTmdbId: 525 })!;
  expect(discordActivity(linkedPlayback, 1_000_000).state_url).toBe("https://www.themoviedb.org/person/525");
  expect(discordActivity({ ...linkedPlayback, director: null }, 1_000_000)).not.toHaveProperty("state_url");
  expect(discordActivity(playback, 1_000_000)).not.toHaveProperty("state_url");
  for (const directorTmdbId of [-1, 0, 1.5, "525"]) expect(() => parseDiscordPlayback({ ...playback, directorTmdbId })).toThrow();
  for (const state of ["playing", "paused", "buffering"] as const) expect(discordActivity({ ...playback, state }, 1_000_000).state).toBe("dir. Christopher Nolan");
  expect(() => parseDiscordPlayback({ ...playback, director: 42 })).toThrow();
  expect(discordActivity(playback, 1_000_000)).toMatchObject({ type: 3, details: "Interstellar", status_display_type: 2, timestamps: { start: 940, end: 1540 }, assets: { large_image: playback.artwork } });
  expect(discordActivity({ ...playback, time: 120 }, 1_000_000).timestamps?.start).toBe(880);
  for (const state of ["paused", "buffering"] as const) expect(discordActivity({ ...playback, state }, 1_000_000)).not.toHaveProperty("timestamps");
  expect(discordActivity({ ...playback, artwork: null }, 1_000_000)).not.toHaveProperty("assets");
  expect(discordArtwork("http://image.tmdb.org/t/p/test.jpg", playback.artwork)).toBe(playback.artwork);
  expect(discordArtwork("https://image.tmdb.org.evil.test/t/p/a.jpg", "https://user@image.tmdb.org/t/p/a.jpg")).toBeNull();
  expect(parseDiscordPlayback(playback)).toEqual(playback);
  for (const input of [{ ...playback, time: NaN }, { ...playback, duration: -1 }, { ...playback, artwork: "https://evil.test/a" }, { ...playback, secret: "x" }, { ...playback, state: "ended" }]) expect(() => parseDiscordPlayback(input)).toThrow();
  expect(parseDiscordPlayback(null)).toBeNull();
});

it("defaults off, persists opt-in, handles split frames, coalesces updates and clears immediately", () => {
  presence.update(playback);
  expect(sockets).toHaveLength(0);
  expect(presence.getSettings()).toEqual({ enabled: false, available: true });
  presence.setEnabled(true);
  expect(JSON.parse(readFileSync(join(directory, "settings.json"), "utf8"))).toEqual({ enabled: true });
  expect(new DiscordPresence(join(directory, "settings.json")).getSettings().enabled).toBe(true);
  const socket = ready();
  expect(messages(socket)[0]).toEqual({ v: 1, client_id: "1549772711264395274" });
  expect(messages(socket)[1].args.activity.details).toBe("Interstellar");
  presence.update({ ...playback, time: 200 });
  presence.update({ ...playback, state: "paused", time: 210 });
  vi.advanceTimersByTime(4999);
  expect(messages(socket)).toHaveLength(2);
  vi.advanceTimersByTime(1);
  expect(messages(socket)[2].args.activity.state).toBe("dir. Christopher Nolan");
  expect(messages(socket)[2].args.activity).not.toHaveProperty("timestamps");
  presence.setEnabled(false);
  expect(messages(socket).at(-1).args.activity).toBeNull();
  expect(socket.end).toHaveBeenCalled();
  vi.advanceTimersByTime(30000);
  expect(sockets).toHaveLength(1);
});

it("retries disconnected clients after 15 seconds and cancels retries on clear", () => {
  presence.setEnabled(true);
  presence.update(playback);
  sockets[0].destroy();
  vi.advanceTimersByTime(14999);
  expect(sockets).toHaveLength(1);
  vi.advanceTimersByTime(1);
  const socket = ready();
  expect(sockets).toHaveLength(2);
  socket.emit("data", Buffer.concat([discordFrame(3, { ping: 1 }), discordFrame(3, { ping: 2 })]));
  expect(socket.write.mock.calls.at(-1)![0].readUInt32LE(0)).toBe(4);
  socket.destroy();
  presence.update(null);
  vi.advanceTimersByTime(30000);
  expect(sockets).toHaveLength(2);
});

it("rejects oversized frames and discovers platform socket paths", () => {
  presence.setEnabled(true);
  presence.update(playback);
  const socket = ready();
  const header = Buffer.alloc(8);
  header.writeUInt32LE(65537, 4);
  socket.emit("data", header);
  expect(socket.destroy).toHaveBeenCalled();
  expect(discordPaths("win32")[0]).toBe("\\\\?\\pipe\\discord-ipc-0");
  expect(discordPaths("darwin", { TMPDIR: "/test", NODE_ENV: "test" })[9]).toBe("/test/discord-ipc-9");
});
