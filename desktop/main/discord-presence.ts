import { randomUUID } from "node:crypto";
import { mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { createConnection, type Socket } from "node:net";
import { dirname, posix } from "node:path";
import { parseDiscordPlayback, type DiscordPlayback, type DiscordSettings } from "../shared/discord-presence";

const applicationId = "1549772711264395274";

export function discordActivity(playback: DiscordPlayback, now: number) {
  const start = Math.floor(now / 1000 - playback.time);
  const details = playback.year ? `${playback.title.slice(0, 121)} (${playback.year})` : playback.title.slice(0, 128);
  const tmdbId = /^tmdb:([1-9]\d*)$/.exec(playback.filmId)?.[1];
  return {
    type: 3,
    name: "in Panorama",
    details,
    ...(tmdbId ? { details_url: `https://www.themoviedb.org/movie/${tmdbId}` } : {}),
    status_display_type: 2,
    ...(playback.director ? { state: `dir. ${playback.director}`.slice(0, 128), ...(playback.directorTmdbId ? { state_url: `https://www.themoviedb.org/person/${playback.directorTmdbId}` } : {}) } : {}),
    ...(playback.artwork ? { assets: { large_image: playback.artwork, large_text: details } } : {}),
    ...(playback.state === "playing" ? { timestamps: { start, ...(playback.duration > playback.time ? { end: start + Math.ceil(playback.duration) } : {}) } } : {}),
  };
}

export function discordFrame(opcode: number, value: unknown): Buffer {
  const body = Buffer.from(JSON.stringify(value));
  const header = Buffer.alloc(8);
  header.writeUInt32LE(opcode, 0);
  header.writeUInt32LE(body.length, 4);
  return Buffer.concat([header, body]);
}

export function discordPaths(platform = process.platform, env = process.env): string[] {
  const prefix = env.XDG_RUNTIME_DIR || env.TMPDIR || env.TMP || env.TEMP || "/tmp";
  return Array.from({ length: 10 }, (_, index) => platform === "win32" ? `\\\\?\\pipe\\discord-ipc-${index}` : posix.join(prefix, `discord-ipc-${index}`));
}

export class DiscordPresence {
  private enabled = false;
  private playback: DiscordPlayback | null = null;
  private receivedAt = 0;
  private socket: Socket | null = null;
  private ready = false;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private lastSent = -Infinity;

  constructor(private readonly file: string, private readonly paths = discordPaths()) {
    try { this.enabled = JSON.parse(readFileSync(file, "utf8")).enabled === true; } catch {}
  }

  getSettings(): DiscordSettings { return { enabled: this.enabled, available: /^\d{17,20}$/.test(applicationId) }; }

  setEnabled(value: unknown): DiscordSettings {
    if (typeof value !== "boolean" || (value && !this.getSettings().available)) throw new Error("Invalid Discord setting");
    if (!value) { this.enabled = false; this.disconnect(); }
    mkdirSync(dirname(this.file), { recursive: true });
    const temporary = `${this.file}.${randomUUID()}.tmp`;
    try {
      writeFileSync(temporary, JSON.stringify({ enabled: value }), { mode: 0o600 });
      renameSync(temporary, this.file);
    } finally { rmSync(temporary, { force: true }); }
    this.enabled = value;
    this.schedule();
    return this.getSettings();
  }

  update(value: unknown): void {
    this.playback = parseDiscordPlayback(value);
    this.receivedAt = Date.now();
    if (!this.playback) this.disconnect();
    else this.schedule();
  }

  clear(): void { this.playback = null; this.disconnect(); }

  private schedule(): void {
    if (!this.enabled || !this.playback || !this.getSettings().available || this.timer) return;
    if (!this.socket) { this.connect(0); return; }
    if (!this.ready) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      if (!this.playback || !this.ready) return;
      const now = Date.now();
      const playback = { ...this.playback, time: this.playback.time + (this.playback.state === "playing" ? (now - this.receivedAt) / 1000 : 0) };
      this.send(1, { cmd: "SET_ACTIVITY", args: { pid: process.pid, activity: discordActivity(playback, now) }, nonce: randomUUID() });
      this.lastSent = now;
    }, Math.max(0, this.lastSent + 5000 - Date.now()));
    this.timer.unref();
  }

  private connect(index: number): void {
    if (!this.enabled || !this.playback) return;
    if (index >= this.paths.length) {
      this.timer = setTimeout(() => { this.timer = null; this.schedule(); }, 15000);
      this.timer.unref();
      return;
    }
    const socket = createConnection(this.paths[index]);
    this.socket = socket;
    let buffer = Buffer.alloc(0);
    let connected = false;
    socket.unref();
    socket.setTimeout(5000);
    socket.on("timeout", () => socket.destroy());
    socket.on("error", () => socket.destroy());
    socket.on("connect", () => { if (this.socket === socket) this.send(0, { v: 1, client_id: applicationId }); });
    socket.on("data", (chunk: Buffer) => {
      if (this.socket !== socket) return;
      buffer = Buffer.concat([buffer, chunk]);
      try {
        while (buffer.length >= 8) {
          const opcode = buffer.readUInt32LE(0);
          const length = buffer.readUInt32LE(4);
          if (length > 65536) throw new Error("Oversized Discord frame");
          if (buffer.length < length + 8) return;
          const body = buffer.subarray(8, length + 8);
          buffer = buffer.subarray(length + 8);
          if (opcode === 3) {
            const header = Buffer.alloc(8);
            header.writeUInt32LE(4, 0);
            header.writeUInt32LE(body.length, 4);
            socket.write(Buffer.concat([header, body]));
          } else if (opcode === 2) { socket.destroy(); return; }
          else if (opcode === 1) {
            const message = JSON.parse(body.toString("utf8"));
            if (message.evt === "ERROR") { socket.destroy(); return; }
            if (message.evt === "READY") {
              connected = true;
              this.ready = true;
              socket.setTimeout(0);
              this.schedule();
            }
          }
        }
      } catch { socket.destroy(); }
    });
    socket.on("close", () => {
      if (this.socket !== socket) return;
      this.socket = null;
      this.ready = false;
      if (this.timer) clearTimeout(this.timer);
      this.timer = null;
      this.connect(connected ? this.paths.length : index + 1);
    });
  }

  private send(opcode: number, value: unknown): void {
    try { this.socket?.write(discordFrame(opcode, value)); } catch { this.socket?.destroy(); }
  }

  private disconnect(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    if (this.ready) this.send(1, { cmd: "SET_ACTIVITY", args: { pid: process.pid, activity: null }, nonce: randomUUID() });
    const socket = this.socket;
    this.socket = null;
    this.ready = false;
    if (socket) {
      socket.end();
      const timeout = setTimeout(() => socket.destroy(), 1000);
      timeout.unref();
      socket.once("close", () => clearTimeout(timeout));
    }
  }
}
