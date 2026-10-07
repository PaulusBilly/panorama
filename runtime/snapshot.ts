import type { RuntimeSnapshot } from "./types";

export const DEFAULT_SERVICE_ENDPOINT = "http://127.0.0.1:11470";
export const DISCOVERABLE_SERVICE_ENDPOINTS = [11470, 11471, 11472, 11473, 11474]
  .map((port) => `http://127.0.0.1:${port}`);

export function serviceEndpointCandidates(configuredEndpoint?: string | null): string[] {
  const preferred = configuredEndpoint && isAllowedServiceEndpoint(configuredEndpoint)
    ? configuredEndpoint
    : DEFAULT_SERVICE_ENDPOINT;
  return [...new Set([preferred, ...DISCOVERABLE_SERVICE_ENDPOINTS])];
}

export const initialRuntimeSnapshot: RuntimeSnapshot = {
  runtime: {
    status: "idle",
    error: null,
  },
  account: {
    status: "loggedOut",
    email: null,
    error: null,
  },
  addons: {
    status: "idle",
    count: 0,
    error: null,
  },
  service: {
    status: "checking",
    endpoint: DEFAULT_SERVICE_ENDPOINT,
  },
  catalog: {
    mode: "popular",
    query: null,
    requestId: 0,
    status: "idle",
    page: {
      items: [],
      nextSkip: 0,
      hasMore: false,
    },
    loadingMore: false,
    error: null,
  },
  people: {
    query: null,
    requestId: 0,
    status: "idle",
    page: {
      items: [],
      nextSkip: 0,
      hasMore: false,
    },
    loadingMore: false,
    error: null,
  },
  details: {
    filmId: null,
    resume: { available: false, offset: 0, duration: null },
    watchlist: { available: false, saved: false, pending: false },
    metadata: { status: "idle", item: null, error: null },
    sources: { status: "idle", groups: [], error: null },
  },
  player: {
    status: "idle",
    stage: null,
    filmId: null,
    sourceId: null,
    title: null,
    paused: true,
    time: 0,
    duration: 0,
    buffering: false,
    volume: 1,
    muted: false,
    fullscreen: false,
    trackDiscoveryReady: false,
    error: null,
    audio: {
      status: "idle",
      tracks: [],
      selectedId: null,
      error: null,
    },
    subtitles: {
      status: "idle",
      tracks: [],
      selectedId: null,
      pendingId: null,
      offset: 0,
      verticalPosition: 15,
      style: {
        fontSizePx: 38,
        lineHeight: 1.24,
        backgroundOpacity: 68,
        borderRadius: 0,
        size: 100,
        textColor: "#ffffff",
        textOpacity: 100,
        backgroundColor: "rgba(0, 0, 0, 0.68)",
        outlineColor: "rgba(0, 0, 0, 0.78)",
        paddingX: 20,
        paddingY: 0,
        fontWeight: "regular",
      },
      error: null,
    },
  },
};

export function isAllowedServiceEndpoint(value: string): boolean {
  try {
    const url = new URL(value);
    const isLoopback =
      url.hostname === "127.0.0.1" ||
      url.hostname === "localhost" ||
      url.hostname === "[::1]";

    return url.protocol === "http:" && isLoopback;
  } catch {
    return false;
  }
}
