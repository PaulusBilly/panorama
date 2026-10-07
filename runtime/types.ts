export type RuntimeStatus = "idle" | "initializing" | "ready" | "error";
export type AccountStatus = "loggedOut" | "authenticating" | "signedIn" | "error";
export type SyncStatus = "idle" | "syncing" | "ready" | "error";
export type ServiceStatus = "checking" | "online" | "offline";
export type CatalogStatus = "idle" | "loading" | "ready" | "error";
export type DetailStatus = "idle" | "loading" | "ready" | "error";
export type SourceGroupStatus = "loading" | "ready" | "error";
export type PlaybackStatus = "idle" | "preparing" | "ready" | "buffering" | "ended" | "error";
export type PlaybackStage = "checkingService" | "resolvingSource" | "loadingVideo" | null;
export type PlaybackSupport = "internal" | "external" | "unsupported";
export type CatalogMode = "popular" | "search";
export type PlaybackStartMode = "resume" | "restart";
export type SourceQuality = "4k" | "hd";
export type SourceAudioChannels = "5.1";

export type PanoramaFilm = {
  id: string;
  type: "movie";
  name: string;
  year: string | null;
  director: string | null;
  originCountry: string | null;
  rating: number | null;
  ratingCount: number | null;
  posterUrl: string | null;
  landscapeUrl: string | null;
  logoUrl: string | null;
  description: string | null;
};

export type PanoramaPerson = {
  id: string;
  name: string;
  department: string | null;
  profileUrl: string | null;
};

export type PanoramaFilmDetails = PanoramaFilm & {
  directorTmdbId?: number | null;
  originalTitle: string | null;
  alternativeTitle: string | null;
  runtime: string | null;
  genres: string[];
};

export type PanoramaSource = {
  id: string;
  addonId: string;
  addonName: string;
  name: string;
  description: string | null;
  quality: SourceQuality | null;
  audioChannels: SourceAudioChannels | null;
  playbackSupport: PlaybackSupport;
  unavailableReason: string | null;
};

export type PanoramaSubtitleTrack = {
  id: string;
  label: string;
  language: string | null;
  origin: "embedded" | "addon";
  sourceLabel: string;
};

export type PanoramaAudioTrack = {
  id: string;
  label: string;
  language: string | null;
  description: string | null;
};

export type PanoramaSubtitleStyle = {
  fontSizePx: number;
  lineHeight: number;
  backgroundOpacity: number;
  borderRadius: number;
  size: number;
  textColor: string;
  textOpacity: number;
  backgroundColor: string;
  outlineColor: string;
  paddingX: number;
  paddingY: number;
  fontWeight: "regular" | "medium" | "semibold" | "bold";
};

export type PanoramaSourceGroup = {
  id: string;
  addonId: string;
  addonName: string;
  status: SourceGroupStatus;
  items: PanoramaSource[];
  error: string | null;
};

export type CatalogPage = {
  items: PanoramaFilm[];
  nextSkip: number;
  hasMore: boolean;
};

export type PeoplePage = {
  items: PanoramaPerson[];
  nextSkip: number;
  hasMore: boolean;
};

export type PanoramaResumeState = {
  available: boolean;
  offset: number;
  duration: number | null;
};

export type PanoramaWatchlistState = {
  available: boolean;
  saved: boolean;
  pending: boolean;
};

export type RuntimeSnapshot = {
  runtime: {
    status: RuntimeStatus;
    error: string | null;
  };
  account: {
    status: AccountStatus;
    email: string | null;
    error: string | null;
  };
  addons: {
    status: SyncStatus;
    count: number;
    error: string | null;
  };
  service: {
    status: ServiceStatus;
    endpoint: string;
  };
  catalog: {
    mode: CatalogMode;
    query: string | null;
    requestId: number;
    status: CatalogStatus;
    page: CatalogPage;
    loadingMore: boolean;
    error: string | null;
  };
  people: {
    query: string | null;
    requestId: number;
    status: CatalogStatus;
    page: PeoplePage;
    loadingMore: boolean;
    error: string | null;
  };
  details: {
    filmId: string | null;
    resume: PanoramaResumeState;
    watchlist: PanoramaWatchlistState;
    metadata: {
      status: DetailStatus;
      item: PanoramaFilmDetails | null;
      error: string | null;
    };
    sources: {
      status: DetailStatus;
      groups: PanoramaSourceGroup[];
      error: string | null;
    };
  };
  player: {
    status: PlaybackStatus;
    stage: PlaybackStage;
    filmId: string | null;
    sourceId: string | null;
    title: string | null;
    paused: boolean;
    time: number;
    duration: number;
    buffering: boolean;
    volume: number;
    muted: boolean;
    fullscreen: boolean;
    trackDiscoveryReady: boolean;
    error: string | null;
    audio: {
      status: "idle" | "loading" | "ready" | "error";
      tracks: PanoramaAudioTrack[];
      selectedId: string | null;
      error: string | null;
    };
    subtitles: {
      status: "idle" | "loading" | "ready" | "error";
      tracks: PanoramaSubtitleTrack[];
      selectedId: string | null;
      pendingId: string | null;
      offset: number;
      verticalPosition: number;
      style: PanoramaSubtitleStyle;
      renderingMode?: "custom" | "native" | "unknown";
      appearanceLimitation?: string | null;
      error: string | null;
    };
  };
};

export type RuntimeListener = () => void;

export interface StremioRuntime {
  getSnapshot(): RuntimeSnapshot;
  subscribe(listener: RuntimeListener): () => void;
  initialize(): Promise<void>;
  login(email: string, password: string): Promise<void>;
  logout(): Promise<void>;
  loadPopularMovies(): Promise<void>;
  searchMovies(query: string): Promise<void>;
  retryMovieSearch(): Promise<void>;
  retryPeopleSearch(): Promise<void>;
  clearSearch(): Promise<void>;
  loadNextPage(): Promise<void>;
  loadNextPeoplePage(): Promise<void>;
  openFilmDetails(filmId: string): Promise<void>;
  retryFilmDetails(): Promise<void>;
  setWatchlisted(saved: boolean): Promise<void>;
  closeFilmDetails(): Promise<void>;
  syncAddons(): Promise<void>;
  checkService(): Promise<ServiceStatus>;
  preparePlayback(sourceId: string): Promise<void>;
  startPlayback(sourceId: string, mode?: PlaybackStartMode): Promise<void>;
  switchPlaybackSource(sourceId: string): Promise<void>;
  retryPlayback(): Promise<void>;
  stopPlayback(): Promise<void>;
  attachPlayer(container: HTMLElement): Promise<void>;
  detachPlayer(): void;
  setPlaybackPaused(paused: boolean): void;
  seekPlayback(time: number): void;
  setPlaybackVolume(volume: number): void;
  setPlaybackMuted(muted: boolean): void;
  setPlaybackFullscreen(fullscreen: boolean, target: HTMLElement): void;
  selectAudioTrack(trackId: string): void;
  selectSubtitle(trackId: string | null): void;
  setSubtitleStyle(style: Partial<PanoramaSubtitleStyle>): void;
  setSubtitleOffset(offset: number): void;
  setSubtitleVerticalPosition(position: number): void;
  flushPlaybackProgress(): void;
  destroy(): void;
}
