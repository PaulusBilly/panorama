import { randomBytes } from "node:crypto";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";
import { Readable } from "node:stream";
import { tmpdir } from "node:os";
import path from "node:path";
import type { MediaFetch, MediaFetchResult } from "./media-fetch";
import { mediaValidator, MediaRepresentationError, validateMediaRange } from "./media-range";
import { retryAfterDeadline } from "./media-retry";
import { createMediaCache, type MediaCache } from "./media-cache";
import { BufferingPolicy, type BufferingSample } from "./buffering-policy";

// Remote sources are read through this loopback proxy so that a single MPV
// stream is filled by several parallel range requests. Long, congested routes
// cap each TCP connection far below the line rate; parallel ranges add up.
const CHUNK_BYTES = 2 * 1_048_576;
// Extra connections are used only while the player is short of buffered media;
// with enough ahead, one connection keeps it topped up like a normal player.
// Pulling far faster than playback needs looks abusive to some servers.
const MAX_PARALLEL_REQUESTS = 6;
const URGENT_AHEAD_SECONDS = 20;
const LOW_AHEAD_SECONDS = 60;
const LOW_PARALLEL_REQUESTS = 3;
// After a stall every new request waits out a growing cooldown, so a server
// that is throttling is not met with a burst of reconnections.
const STALL_COOLDOWN_MS = 2_000;
const MAX_STALL_COOLDOWN_MS = 30_000;
const STALL_RECOVERY_MS = 60_000;
const WINDOW_CHUNKS = 48;
const CHUNK_RETRIES = 6;
const RATE_WINDOW_MS = 5_000;
// A request that delivers no bytes for this long is abandoned and retried;
// servers may silently hold connections beyond their per-file limit.
const STALL_TIMEOUT_MS = 8_000;
// Debrid resolvers may prepare a link for tens of seconds before answering.
const PROBE_TIMEOUT_MS = 90_000;
// Refresh an expired signed link at most this often per film, so failures do
// not turn into a burst of requests against the resolver.
const RERESOLVE_INTERVAL_MS = 30_000;
const EXPIRED_LINK_STATUSES = new Set([401, 403, 404, 410]);
// A warm-up resolves the link and fetches only enough of the start for MPV to
// open the file; an unused warm-up is discarded after this long.
const WARM_CHUNKS = 3;
const WARM_TTL_MS = 10 * 60_000;
// After a source's server refuses or drops the connection, requests for it fail
// at once for this long instead of waiting out another connection attempt.
const UNREACHABLE_HOLD_MS = 2 * 60_000;

type Fetch = typeof fetch | MediaFetch;

async function fetchMedia(fetchImpl: Fetch, url: string, init: RequestInit): Promise<MediaFetchResult> {
  const result = await (fetchImpl as MediaFetch)(url, init);
  return result instanceof Response ? { response: result, finalUrl: result.url || url } : result;
}

export type MediaProxyEvent = Record<string, string | number | boolean | null>;

class UpstreamStatusError extends Error {
  constructor(readonly status: number, readonly retryAfter: string | null = null) {
    super(`Unexpected media response ${status}`);
  }
}

// A range buffer that readers consume as bytes arrive rather than once complete.
type Chunk = {
  controller: AbortController;
  data: Buffer;
  received: number;
  waiters: Array<() => void>;
  promise: Promise<void>;
};

function describeError(error: unknown): string {
  if (!(error instanceof Error)) return "error";
  return error.name === "TimeoutError" ? "Media request timed out" : "Media transfer failed";
}

function wake(chunk: Chunk): void {
  const waiters = chunk.waiters;
  chunk.waiters = [];
  for (const waiter of waiters) waiter();
}

type Reader = { position: number; foreground: boolean };

export type MediaWarmResult = { status: "ready" | "failed"; error: string | null; unreachable: boolean };

export type MediaProxyStats = {
  downloadMbps: number | null;
  sizeBytes: number | null;
  unreachable: boolean;
  representationChanged?: boolean;
  transferDemanded?: boolean;
  parallel?: number;
  targetAheadSeconds?: number;
  resumeBufferSeconds?: number;
  memoryBytes?: number;
  diskBytes?: number;
  reservedBytes?: number;
};

function delay(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) { reject(signal.reason); return; }
    const timer = setTimeout(() => { signal.removeEventListener("abort", abort); resolve(); }, Math.min(ms, 2_147_483_647));
    const abort = () => { clearTimeout(timer); reject(signal.reason); };
    signal.addEventListener("abort", abort, { once: true });
  });
}

class RangeSession {
  private readonly chunks = new Map<number, Chunk>();
  private readonly readers = new Set<Reader>();
  private everServed = false;
  private readonly samples: Array<[number, number]> = [];
  private readonly closeController = new AbortController();
  private prepared: Promise<"ranged" | "passthrough" | "failed"> | null = null;
  private reresolving: Promise<void> | null = null;
  private lastResolveAt = -Infinity;
  private readonly host: string | null;
  private resolvedUrl: string;
  private inFlight = 0;
  private activeTransfers = 0;
  private validator: string | null = null;
  private representationChanged = false;
  private readonly sessionId = randomBytes(16).toString("hex") + ":";
  private scheduleTimer: ReturnType<typeof setTimeout> | null = null;
  private idleTimer: ReturnType<typeof setTimeout> | null = null;
  private readonly policy = new BufferingPolicy();
  private targetAheadSeconds = 60;
  private windowChunks = WINDOW_CHUNKS;
  private prefetchAllowed = true;
  private readonly adaptive = process.env.PANORAMA_ADAPTIVE_BUFFERING === "1";
  // Startup has nothing buffered, so it begins at full parallelism.
  private demandParallel = this.adaptive ? LOW_PARALLEL_REQUESTS : MAX_PARALLEL_REQUESTS;
  private stallCap = MAX_PARALLEL_REQUESTS;
  private readonly baseCooldownMs: number;
  private readonly maxCooldownMs: number;
  private cooldownMs: number;
  private pausedUntil = 0;
  private lastStallAt = 0;
  private contentType = "application/octet-stream";
  private lastProbeError: string | null = null;
  private unreachableAt: number | null = null;
  size: number | null = null;

  constructor(
    private readonly sourceUrl: string,
    private readonly fetchImpl: Fetch,
    private readonly now: () => number,
    private readonly stallTimeoutMs: number,
    private readonly log: (event: MediaProxyEvent) => void,
    private readonly cache: MediaCache,
  ) {
    this.resolvedUrl = sourceUrl;
    let host: string | null = null;
    try { host = new URL(sourceUrl).host; } catch {}
    this.host = host;
    const scale = stallTimeoutMs / STALL_TIMEOUT_MS;
    this.baseCooldownMs = STALL_COOLDOWN_MS * scale;
    this.maxCooldownMs = MAX_STALL_COOLDOWN_MS * scale;
    this.cooldownMs = this.baseCooldownMs;
  }

  close(): void {
    this.closeController.abort();
    if (this.scheduleTimer) clearTimeout(this.scheduleTimer);
    this.scheduleTimer = null;
    if (this.idleTimer) clearTimeout(this.idleTimer);
    this.idleTimer = null;
    for (const [index, chunk] of this.chunks) { chunk.controller.abort(); wake(chunk); this.cache.release(this.key(index)); }
    this.chunks.clear();
    void this.cache.removeSession(this.sessionId);
  }

  // Sets how many parallel ranges the player's buffer currently warrants.
  setReadAhead(aheadSeconds: number | null, buffering: boolean, context?: Partial<BufferingSample>): void {
    this.demandParallel = buffering || aheadSeconds === null || aheadSeconds < URGENT_AHEAD_SECONDS
      ? MAX_PARALLEL_REQUESTS
      : aheadSeconds < LOW_AHEAD_SECONDS ? LOW_PARALLEL_REQUESTS : 1;
    if (this.stallCap < MAX_PARALLEL_REQUESTS && this.now() - this.lastStallAt > STALL_RECOVERY_MS) {
      this.stallCap += 1;
      this.lastStallAt = this.now();
    }
    if (this.adaptive) {
      const decision = this.policy.update({ nowMs: this.now(), bufferedSeconds: aheadSeconds, sourceMbps: context?.sourceMbps ?? null, downloadMbps: this.downloadMbps(), transferDemanded: this.activeTransfers > 0 && this.readers.size > 0, buffering, paused: context?.paused ?? false, playbackSpeed: context?.playbackSpeed ?? 1, throttled: this.now() < this.pausedUntil });
      this.demandParallel = decision.parallel;
      this.targetAheadSeconds = decision.targetAheadSeconds;
      const demand = context?.sourceMbps;
      this.windowChunks = demand && demand > 0 ? Math.max(3, Math.min(48, Math.ceil(demand * 125000 * this.targetAheadSeconds / CHUNK_BYTES))) : 30;
    }
    this.schedule();
  }

  diagnostics() { return { representationChanged: this.representationChanged, transferDemanded: this.activeTransfers > 0 && this.readers.size > 0, parallel: this.parallel, targetAheadSeconds: this.targetAheadSeconds, resumeBufferSeconds: 5 }; }

  private key(index: number): string { return `${this.sessionId}${index}`; }

  private get parallel(): number {
    return Math.min(this.demandParallel, this.stallCap);
  }

  private needed(index: number): boolean {
    return [...this.readers].some((reader) => Math.floor(reader.position / CHUNK_BYTES) === index);
  }

  downloadMbps(): number | null {
    const since = this.now() - RATE_WINDOW_MS;
    while (this.samples.length > 0 && this.samples[0][0] < since) this.samples.shift();
    if (this.activeTransfers === 0) return null;
    if (this.samples.length === 0) return 0;
    const bytes = this.samples.reduce((total, [, value]) => total + value, 0);
    return bytes * 8 / (RATE_WINDOW_MS / 1_000) / 1_000_000;
  }

  async serve(request: IncomingMessage, response: ServerResponse): Promise<void> {
    if (this.unreachable()) { response.writeHead(502); response.end(); return; }
    if (this.readers.size >= 8) { response.writeHead(503); response.end(); return; }
    const mode = await this.prepare();
    if (this.closeController.signal.aborted) { response.writeHead(410); response.end(); return; }
    // An unanswered source fails this request quickly; the player's reload
    // starts a fresh attempt rather than reusing a failed probe.
    if (mode === "failed") { response.writeHead(502); response.end(); return; }
    if (mode === "passthrough" || this.size === null) { await this.passthrough(request, response); return; }
    const size = this.size;
    const range = /^bytes=(\d*)-(\d*)$/.exec(request.headers.range ?? "");
    let start = 0;
    let end = size - 1;
    if (range?.[1]) {
      start = Number(range[1]);
      if (range[2]) end = Math.min(Number(range[2]), size - 1);
    } else if (range?.[2]) {
      start = Math.max(0, size - Number(range[2]));
    }
    if (![start, end].every(Number.isSafeInteger) || start < 0 || start >= size || start > end) {
      response.writeHead(416, { "Content-Range": `bytes */${size}` });
      response.end();
      return;
    }
    const headers = {
      "Accept-Ranges": "bytes",
      "Content-Length": String(end - start + 1),
      "Content-Type": this.contentType,
    };
    if (range) response.writeHead(206, { ...headers, "Content-Range": `bytes ${start}-${end}/${size}` });
    else response.writeHead(200, headers);
    if (request.method === "HEAD") { response.end(); return; }

    const reader: Reader = { position: start, foreground: this.readers.size === 0 };
    let pinned: string | null = null;
    let open = true;
    response.on("close", () => { open = false; this.readers.delete(reader); if (reader.foreground) { const next = this.readers.values().next().value; if (next) next.foreground = true; } for (const chunk of this.chunks.values()) wake(chunk); this.schedule(); });
    this.readers.add(reader);
    this.everServed = true;
    try {
      while (open && reader.position <= end) {
        this.schedule();
        const index = Math.floor(reader.position / CHUNK_BYTES);
        const key = this.key(index);
        if (pinned !== key) { if (pinned) this.cache.unpin(pinned); this.cache.pin(key); pinned = key; }
        const chunk = this.chunk(index);
        const chunkStart = index * CHUNK_BYTES;
        const offset = reader.position - chunkStart;
        while (open && !this.closeController.signal.aborted && chunk.received <= offset) {
          await Promise.race([chunk.promise, new Promise<void>((resolve) => chunk.waiters.push(resolve))]);
        }
        if (!open || this.closeController.signal.aborted) break;
        const slice = chunk.data.subarray(offset, Math.min(chunk.received, end - chunkStart + 1));
        reader.position += slice.length;
        if (reader.position >= chunkStart + chunk.data.length) await chunk.promise;
        if (!response.write(slice)) {
          await new Promise<void>((resolve) => {
            const done = () => { response.off("drain", done); response.off("close", done); resolve(); };
            response.on("drain", done);
            response.on("close", done);
          });
        }
      }
      if (open) response.end();
    } catch {
      response.destroy();
    } finally {
      if (pinned) this.cache.unpin(pinned);
      this.readers.delete(reader);
      this.schedule();
    }
  }

  // True while the source's server recently refused or dropped the connection.
  unreachable(): boolean {
    return this.unreachableAt !== null && this.now() - this.unreachableAt < UNREACHABLE_HOLD_MS;
  }

  // Resolves the link and fetches the opening chunks without a reader.
  async warm(): Promise<MediaWarmResult> {
    if (this.unreachable()) return { status: "failed", error: this.lastProbeError, unreachable: true };
    const mode = await this.prepare();
    if (mode === "failed") return { status: "failed", error: this.lastProbeError, unreachable: this.unreachable() };
    if (mode === "ranged" && this.size !== null) {
      const last = Math.ceil(this.size / CHUNK_BYTES) - 1;
      if (!this.closeController.signal.aborted) {
        for (let index = 1; index < Math.min(WARM_CHUNKS, last + 1); index += 1) this.chunk(index);
        if (last >= WARM_CHUNKS) {
          if (this.inFlight < this.parallel) this.chunk(last);
          else void Promise.race([...this.chunks.values()].map((chunk) => chunk.promise)).then(() => {
            if (!this.closeController.signal.aborted) this.chunk(last);
          }).catch(() => undefined);
        }
      }
    }
    return { status: "ready", error: null, unreachable: false };
  }

  private prepare(): Promise<"ranged" | "passthrough" | "failed"> {
    this.prepared ??= (async () => {
      const started = this.now();
      try {
        // The probe asks for the whole first range, so its body becomes the
        // first chunk and startup saves a round trip to the source.
        let owner = new AbortController();
        let result: MediaFetchResult;
        for (let attempt = 0; ; attempt += 1) {
          while (this.now() < this.pausedUntil) await delay(this.pausedUntil - this.now(), this.closeController.signal);
          owner = new AbortController();
          const timeout = setTimeout(() => owner.abort(new Error("Media probe timed out")), PROBE_TIMEOUT_MS);
          try {
            result = await fetchMedia(this.fetchImpl, this.sourceUrl, { headers: { Range: `bytes=0-${CHUNK_BYTES - 1}`, "Accept-Encoding": "identity" }, cache: "no-store", signal: AbortSignal.any([this.closeController.signal, owner.signal]) });
          } catch (error) { owner.abort(); throw error; } finally { clearTimeout(timeout); }
          if (result.response.status !== 429 && result.response.status !== 503) break;
          owner.abort();
          await result.response.body?.cancel().catch(() => undefined);
          this.stallCap = Math.max(1, this.stallCap - 1);
          this.lastStallAt = this.now();
          this.pausedUntil = Math.max(this.pausedUntil, retryAfterDeadline(result.response.headers.get("retry-after"), this.now()) ?? 0, this.now() + this.cooldownMs);
          this.cooldownMs = Math.min(this.maxCooldownMs, this.cooldownMs * 2);
          if (attempt >= CHUNK_RETRIES) throw new UpstreamStatusError(result.response.status);
        }
        const { response, finalUrl } = result;
        let total: number | null = null;
        if (response.status === 206) {
          try { total = validateMediaRange(response.headers, 0, CHUNK_BYTES - 1, null).total; }
          catch (error) { owner.abort(); await response.body?.cancel().catch(() => undefined); throw error; }
        }
        this.contentType = response.headers.get("content-type") ?? this.contentType;
        const ms = this.now() - started;
        if (response.status === 206 && total !== null && Number.isSafeInteger(total) && total > 0) {
          this.size = total;
          this.resolvedUrl = finalUrl;
          this.unreachableAt = null;
          this.validator = mediaValidator(response.headers);
          this.chunk(0, { response, owner });
          let target: string | null = null;
          try { target = new URL(this.resolvedUrl).host; } catch {}
          this.log({ event: "probe", result: "ranged", status: 206, ms, host: this.host, target, sizeMb: Math.round(total / 1_048_576) });
          return "ranged";
        }
        owner.abort();
        await response.body?.cancel().catch(() => undefined);
        if (response.ok) {
          this.log({ event: "probe", result: "passthrough", status: response.status, ms, host: this.host });
          return "passthrough";
        }
        this.lastProbeError = `HTTP ${response.status}`;
        this.log({ event: "probe", result: "failed", status: response.status, ms, host: this.host });
      } catch (error) {
        this.lastProbeError = describeError(error);
        // A network failure (not a timeout or our own cancellation) means the
        // server refused or dropped the connection.
        const name = error instanceof Error ? error.name : "";
        if (!this.closeController.signal.aborted && name !== "TimeoutError" && name !== "AbortError") {
          this.unreachableAt = this.now();
        }
        this.log({ event: "probe", result: "failed", error: this.lastProbeError, ms: this.now() - started, host: this.host });
      }
      this.prepared = null;
      return "failed";
    })();
    return this.prepared;
  }

  // Fetches a fresh redirect target for an expired signed link. Shared by all
  // ranges and rate-limited per film.
  private reresolve(): Promise<void> {
    if (this.now() - this.lastResolveAt >= RERESOLVE_INTERVAL_MS) { this.reresolving = null; this.lastResolveAt = this.now(); }
    this.reresolving ??= (async () => {
      const owner = new AbortController();
      const timeout = setTimeout(() => owner.abort(), PROBE_TIMEOUT_MS);
      try {
        const { response, finalUrl } = await fetchMedia(this.fetchImpl, this.sourceUrl, { headers: { Range: "bytes=0-0", "Accept-Encoding": "identity" }, cache: "no-store", signal: AbortSignal.any([this.closeController.signal, owner.signal]) });
        try {
          if (response.status !== 206) throw new UpstreamStatusError(response.status, response.headers.get("retry-after"));
          validateMediaRange(response.headers, 0, 0, this.size);
          if (!this.validator || mediaValidator(response.headers) !== this.validator) throw new MediaRepresentationError("Media identity changed");
          this.resolvedUrl = finalUrl;
        } finally { owner.abort(); await response.body?.cancel().catch(() => undefined); }
      } catch (error) {
        if (error instanceof MediaRepresentationError) { this.representationChanged = true; this.close(); }
        throw error;
      } finally { clearTimeout(timeout); }
    })();
    return this.reresolving;
  }

  private async passthrough(request: IncomingMessage, response: ServerResponse): Promise<void> {
    const controller = new AbortController();
    response.on("close", () => controller.abort());
    try {
      const { response: upstream } = await fetchMedia(this.fetchImpl, this.sourceUrl, {
        method: request.method === "HEAD" ? "HEAD" : "GET",
        headers: request.headers.range ? { Range: request.headers.range } : {},
        cache: "no-store",
        signal: AbortSignal.any([controller.signal, this.closeController.signal]),
      });
      const headers: Record<string, string> = {};
      for (const name of ["content-type", "content-length", "content-range", "accept-ranges"]) {
        const value = upstream.headers.get(name);
        if (value) headers[name] = value;
      }
      response.writeHead(upstream.status, headers);
      if (!upstream.body || request.method === "HEAD") { response.end(); return; }
      Readable.fromWeb(upstream.body as never).on("error", () => response.destroy()).pipe(response);
    } catch {
      response.destroy();
    }
  }

  private schedule(): void {
    if (this.closeController.signal.aborted || this.size === null) return;
    if (this.readers.size === 0) {
      if (!this.everServed) return;
      if (!this.idleTimer) {
        this.idleTimer = setTimeout(() => {
          this.idleTimer = null;
          if (this.readers.size > 0) return;
          for (const [index, chunk] of this.chunks) { chunk.controller.abort(); this.chunks.delete(index); this.cache.release(this.key(index)); }
        }, 250);
        this.idleTimer.unref?.();
      }
      return;
    }
    if (this.idleTimer) clearTimeout(this.idleTimer);
    this.idleTimer = null;
    const last = Math.ceil(this.size / CHUNK_BYTES) - 1;
    const ordered = [...this.readers].sort((left, right) => Number(right.foreground) - Number(left.foreground));
    const firsts = ordered.map((reader) => Math.floor(reader.position / CHUNK_BYTES));
    const wanted = (index: number) => ordered.some((reader) => { const first = Math.floor(reader.position / CHUNK_BYTES); return index >= first - 1 && index <= first + (reader.foreground ? this.windowChunks : 0); });
    for (const [index, chunk] of this.chunks) {
      if (wanted(index) && !(this.cache.has(this.key(index)) && chunk.received === chunk.data.length && !this.needed(index))) continue;
      chunk.controller.abort();
      this.chunks.delete(index);
      this.cache.release(this.key(index));
    }
    if (this.now() < this.pausedUntil) {
      if (!this.scheduleTimer) {
        this.scheduleTimer = setTimeout(() => { this.scheduleTimer = null; this.schedule(); }, Math.min(2_147_483_647, this.pausedUntil - this.now()));
        this.scheduleTimer.unref?.();
      }
      return;
    }
    for (let offset = 0; offset <= this.windowChunks && this.inFlight < this.parallel; offset += 1) {
      for (let readerIndex = 0; readerIndex < firsts.length; readerIndex += 1) {
        if (offset > 0 && (!ordered[readerIndex].foreground || !this.prefetchAllowed)) continue;
        const first = firsts[readerIndex];
        const index = first + offset;
        if (index <= last && !this.chunks.has(index) && !this.cache.has(this.key(index)) && this.inFlight < this.parallel) this.chunk(index);
      }
    }
  }

  private chunk(index: number, initial?: { response: Response; owner: AbortController }): Chunk {
    const existing = this.chunks.get(index);
    if (existing) return existing;
    const start = index * CHUNK_BYTES;
    const length = Math.min(this.size ?? 0, start + CHUNK_BYTES) - start;
    const chunk: Chunk = {
      controller: new AbortController(),
      data: Buffer.alloc(0),
      received: 0,
      waiters: [],
      promise: Promise.resolve(),
    };
    if (this.inFlight >= this.parallel && this.needed(index)) {
      for (const [otherIndex, other] of this.chunks) {
        if (this.needed(otherIndex) || other.received === other.data.length && other.data.length > 0) continue;
        other.controller.abort();
        break;
      }
    }
    this.inFlight += 1;
    chunk.promise = (async () => {
      try {
      while (!this.cache.reserve(this.key(index), length)) {
        this.prefetchAllowed = false;
        if (!initial && !this.needed(index)) throw new Error("Media cache admission failed");
        await delay(25, AbortSignal.any([chunk.controller.signal, this.closeController.signal]));
      }
      const cached = await this.cache.get(this.key(index));
      if (chunk.controller.signal.aborted || this.closeController.signal.aborted) { initial?.owner.abort(); this.cache.release(this.key(index)); throw new Error("Media request aborted"); }
      chunk.data = cached ?? Buffer.allocUnsafe(length);
      if (cached) { chunk.received = cached.length; wake(chunk); initial?.owner.abort(); return; }
      await this.download(index, chunk, initial);
      if (!chunk.controller.signal.aborted && !this.closeController.signal.aborted) void this.cache.put(this.key(index), chunk.data).then(() => {
        if (!this.needed(index) && this.chunks.get(index) === chunk) { this.chunks.delete(index); this.cache.release(this.key(index)); queueMicrotask(() => this.schedule()); }
      }).catch(() => { this.prefetchAllowed = false; this.demandParallel = 1; });
      } finally { this.inFlight -= 1; queueMicrotask(() => this.schedule()); }
    })();
    chunk.promise.then(() => wake(chunk), () => {
      if (this.chunks.get(index) === chunk) { this.chunks.delete(index); this.cache.release(this.key(index)); }
      wake(chunk);
    });
    this.chunks.set(index, chunk);
    return chunk;
  }

  private async download(index: number, chunk: Chunk, initial?: { response: Response; owner: AbortController }): Promise<void> {
    const signal = AbortSignal.any([chunk.controller.signal, this.closeController.signal]);
    const start = index * CHUNK_BYTES;
    const end = start + chunk.data.length - 1;
    while (this.activeTransfers >= this.parallel) await delay(25, signal);
    this.activeTransfers += 1;
    try {
    for (let attempt = 0; ; attempt += 1) {
        const attemptController = attempt === 0 && initial ? initial.owner : new AbortController();
        let stalled = false;
        let watchdog: ReturnType<typeof setTimeout> | null = null;
        let reader: ReadableStreamDefaultReader<Uint8Array> | null = null;
        const cancel = () => { attemptController.abort(); void reader?.cancel().catch(() => undefined); };
        signal.addEventListener("abort", cancel, { once: true });
        const arm = () => {
          if (watchdog) clearTimeout(watchdog);
          watchdog = setTimeout(() => { stalled = true; cancel(); }, this.stallTimeoutMs);
        };
        try {
          if (signal.aborted) throw new Error("Media request aborted");
          while (this.now() < this.pausedUntil) await delay(this.pausedUntil - this.now(), signal);
          if (attempt > 0 && chunk.received === chunk.data.length) chunk.received = 0;
          arm();
          const expectedStart = start + chunk.received;
          const response = attempt === 0 && initial ? initial.response : (await fetchMedia(this.fetchImpl, this.resolvedUrl, {
            headers: { Range: `bytes=${expectedStart}-${end}`, "Accept-Encoding": "identity", ...(this.validator ? { "If-Range": this.validator } : {}) }, cache: "no-store", signal: AbortSignal.any([signal, attemptController.signal]),
          })).response;
          if (response.status !== 206 || !response.body) {
            await response.body?.cancel().catch(() => undefined);
            if (response.status === 200) throw new MediaRepresentationError("Media representation changed");
            throw new UpstreamStatusError(response.status, response.headers.get("retry-after"));
          }
          validateMediaRange(response.headers, expectedStart, end, this.size);
          if (this.validator && mediaValidator(response.headers) !== this.validator) throw new MediaRepresentationError("Media validator changed");
          if (attempt > 0 && !this.validator) throw new MediaRepresentationError("Cannot resume unverified media");
          reader = response.body.getReader();
          while (true) {
            const part = await reader.read();
            if (signal.aborted || attemptController.signal.aborted) throw new Error("Media request aborted");
            if (part.done) break;
            arm();
            if (part.value.length > chunk.data.length - chunk.received) throw new MediaRepresentationError("Overlong media range");
            chunk.data.set(part.value, chunk.received);
            chunk.received += part.value.length;
            this.samples.push([this.now(), part.value.length]);
            wake(chunk);
          }
          if (chunk.received !== chunk.data.length) throw new Error("Incomplete media range");
          this.cooldownMs = this.baseCooldownMs;
          return;
        } catch (error) {
          if (watchdog) clearTimeout(watchdog);
          watchdog = null;
          cancel();
          await reader?.cancel().catch(() => undefined);
          reader?.releaseLock();
          reader = null;
          if (signal.aborted) throw error;
          if (error instanceof MediaRepresentationError) { this.representationChanged = true; this.close(); throw error; }
          const status = error instanceof UpstreamStatusError ? error.status : null;
          const throttled = stalled || status === 429 || status === 503;
          if (throttled) {
            this.stallCap = Math.max(1, this.stallCap - 1);
            this.lastStallAt = this.now();
            const serverDeadline = error instanceof UpstreamStatusError ? retryAfterDeadline(error.retryAfter, this.now()) : null;
            this.pausedUntil = Math.max(this.pausedUntil, serverDeadline ?? 0, this.now() + this.cooldownMs);
            this.cooldownMs = Math.min(this.maxCooldownMs, this.cooldownMs * 2);
            let surplus = this.activeTransfers - this.parallel;
            for (const [otherIndex, other] of this.chunks) {
              if (surplus <= 0) break;
              if (otherIndex === index || this.needed(otherIndex) || other.received === other.data.length && other.data.length > 0) continue;
              other.controller.abort(); surplus -= 1;
            }
          }
          this.log({ event: "range-retry", index, attempt, status, stalled, parallel: this.parallel, host: this.host });
          if (attempt >= CHUNK_RETRIES || (throttled && !this.needed(index) && this.inFlight > this.parallel)) throw error;
          if (status !== null && EXPIRED_LINK_STATUSES.has(status)) await this.reresolve();
          while (this.now() < this.pausedUntil) await delay(this.pausedUntil - this.now(), signal);
          if (!throttled) await delay(500 * 2 ** Math.min(attempt, 3), signal);
        } finally {
          if (watchdog) clearTimeout(watchdog);
          signal.removeEventListener("abort", cancel);
          await reader?.cancel().catch(() => undefined);
          reader?.releaseLock();
          attemptController.abort();
        }
      }
    } finally { this.activeTransfers -= 1; }
  }

}

export class MediaProxy {
  private server: Server | null = null;
  private origin: string | null = null;
  private cache: MediaCache | null = null;
  private readonly sessions = new Map<string, RangeSession>();
  private warmed: { url: string; session: RangeSession; timer: ReturnType<typeof setTimeout> } | null = null;

  constructor(
    private readonly fetchImpl: Fetch = fetch,
    private readonly now: () => number = Date.now,
    private readonly stallTimeoutMs = STALL_TIMEOUT_MS,
    private readonly log: (event: MediaProxyEvent) => void = () => undefined,
    private readonly cacheDirectory = path.join(tmpdir(), "panorama-media-cache"),
  ) {}

  async start(): Promise<void> {
    if (this.server) return;
    this.cache = await createMediaCache({ directory: this.cacheDirectory, maxMemoryBytes: 96 * 1_048_576, maxDiskBytes: 2 * 1_073_741_824, reserveFreeBytes: 1_073_741_824 });
    const server = createServer((request, response) => {
      const token = /^\/media\/([0-9a-f]{32})$/.exec(request.url ?? "")?.[1];
      const session = token ? this.sessions.get(token) : undefined;
      if (!session || (request.method !== "GET" && request.method !== "HEAD")) {
        response.writeHead(404);
        response.end();
        return;
      }
      void session.serve(request, response);
    });
    await new Promise<void>((resolve, reject) => {
      server.once("error", reject);
      server.listen(0, "127.0.0.1", () => resolve());
    });
    this.server = server;
    this.origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  }

  // Replaces any previous session; only one film plays at a time. A warm-up
  // for the same source is adopted so playback starts from fetched data.
  open(url: string): string {
    if (!this.origin) throw new Error("Media proxy is not started");
    const warmed = this.warmed?.url === url ? this.warmed : null;
    if (warmed) {
      clearTimeout(warmed.timer);
      this.warmed = null;
    }
    this.close();
    const token = randomBytes(16).toString("hex");
    this.sessions.set(token, warmed?.session ?? this.createSession(url));
    return `${this.origin}/media/${token}`;
  }

  // Prepares one source ahead of playback. Only one warm-up is kept.
  async warm(url: string): Promise<MediaWarmResult> {
    if (this.warmed?.url !== url) {
      this.discardWarm();
      const session = this.createSession(url);
      const timer = setTimeout(() => this.discardWarm(), WARM_TTL_MS);
      timer.unref?.();
      this.warmed = { url, session, timer };
    }
    const result = await this.warmed!.session.warm();
    this.log({ event: "warm", result: result.status, error: result.error });
    return result;
  }

  close(): void {
    for (const session of this.sessions.values()) session.close();
    this.sessions.clear();
  }

  private discardWarm(): void {
    if (!this.warmed) return;
    clearTimeout(this.warmed.timer);
    this.warmed.session.close();
    this.warmed = null;
  }

  private createSession(url: string): RangeSession {
    if (!this.cache) throw new Error("Media cache is not initialized");
    return new RangeSession(url, this.fetchImpl, this.now, this.stallTimeoutMs, this.log, this.cache);
  }

  setReadAhead(aheadSeconds: number | null, buffering: boolean, context?: Partial<BufferingSample>): void {
    for (const session of this.sessions.values()) session.setReadAhead(aheadSeconds, buffering, context);
  }

  stats(): MediaProxyStats {
    const session = [...this.sessions.values()].at(-1);
    return {
      downloadMbps: session?.downloadMbps() ?? null,
      sizeBytes: session?.size ?? null,
      unreachable: session?.unreachable() ?? false,
      ...session?.diagnostics(),
      ...this.cache?.stats(),
    };
  }

  destroy(): void {
    this.discardWarm();
    this.close();
    void this.cache?.close().catch(() => undefined);
    this.cache = null;
    this.server?.close();
    this.server = null;
    this.origin = null;
  }
}
