import { describe, expect, it } from "vitest";
import {
  applyDirectorNames,
  clampSubtitleOffset,
  clampSubtitlePaddingX,
  clampSubtitlePaddingY,
  clampSubtitleSize,
  clampSubtitleVerticalPosition,
  directorNamesFromCatalog,
  embeddedSubtitleSourceLabel,
  extraSubtitleSourceLabel,
  extraSubtitleTracksFromCore,
  extraSubtitleTracksFromStream,
  uniqueExtraSubtitleTracks,
  subtitleLanguageId,
  subtitleLanguageName,
  isCurrentFilmRequest,
  normalizeFilm,
  normalizeFilmDetails,
  normalizeFilms,
  normalizePlaybackSupport,
  normalizeResumeState,
  normalizeWatchlisted,
  normalizeSourceGroups,
  preferredEnglishAudioTrack,
  preferredEnglishSubtitleTrack,
  firstPlayableSource,
  isAioStreamsAddon,
  isLocalFilesAddon,
  onlyAioStreamsSourceGroups,
  parseFilmDirector,
  parseFilmRating,
  parseFilmYear,
  parseSourceAudioChannels,
  parseSourceQuality,
} from "@/runtime/normalize";

describe("film metadata normalization", () => {
  it("prefers the release info year and keeps both artwork shapes", () => {
    expect(normalizeFilm({
      id: "tt123",
      type: "movie",
      name: "A Film",
      releaseInfo: "2024",
      released: "2023-01-01",
      director: ["Greta Gerwig"],
      imdbRating: "8.2",
      poster: "https://example.com/poster.jpg",
      background: "https://example.com/background.jpg",
      description: "A description",
    })).toEqual({
      id: "tt123",
      type: "movie",
      name: "A Film",
      year: "2024",
      director: "Greta Gerwig",
      originCountry: null,
      rating: 8.2,
      ratingCount: null,
      posterUrl: "https://example.com/poster.jpg",
      landscapeUrl: "https://example.com/background.jpg",
      logoUrl: null,
      description: "A description",
    });
  });

  it("falls back to the released date and rejects incomplete items", () => {
    expect(parseFilmYear(null, "1999-08-12T00:00:00.000Z")).toBe("1999");
    expect(normalizeFilm({ id: "tt123", type: "series", name: "Series" })).toBeNull();
    expect(normalizeFilm({ id: "tt123", type: "movie" })).toBeNull();
  });

  it("falls back to the IMDb catalog link for ratings stripped from core previews", () => {
    expect(parseFilmRating(null, [
      { category: "share", name: "A Film" },
      { category: "imdb", name: "7.9" },
    ])).toBe(7.9);
  });

  it("joins multiple directors and falls back to catalog director links", () => {
    expect(parseFilmDirector(["Charlotte Wells", "Someone Else"], null)).toBe("Charlotte Wells, Someone Else");
    expect(parseFilmDirector(null, [
      { category: "Cast", name: "Paul Mescal" },
      { category: "Directors", name: "Charlotte Wells" },
    ])).toBe("Charlotte Wells");
    expect(parseFilmDirector(null, [
      { category: "director", name: "Celine Song" },
    ])).toBe("Celine Song");
  });

  it("leaves lite search-style metas without a director until extras are applied", () => {
    expect(normalizeFilm({
      id: "tt0074991",
      type: "movie",
      name: "Obsession",
      releaseInfo: "1976",
      poster: "https://example.com/poster.jpg",
      background: "https://example.com/background.jpg",
    })?.director).toBeNull();
  });

  it("fills directors missing from core previews using a catalog lookup", () => {
    const films = normalizeFilms([
      { id: "tt1", type: "movie", name: "One", releaseInfo: "2022" },
    ]);
    expect(films[0]?.director).toBeNull();
    expect(
      applyDirectorNames(films, directorNamesFromCatalog([
        { id: "tt1", type: "movie", name: "One", director: ["Charlotte Wells"] },
      ]))[0]?.director,
    ).toBe("Charlotte Wells");
  });

  it("deduplicates catalog items", () => {
    const items = [
      { id: "tt1", type: "movie", name: "One" },
      { id: "tt1", type: "movie", name: "One again" },
      { id: "tt2", type: "movie", name: "Two" },
    ];

    expect(normalizeFilms(items).map((film) => film.id)).toEqual(["tt1", "tt2"]);
  });

  it("keeps same-title films that have different ids", () => {
    expect(normalizeFilms([
      { id: "tt37287335", type: "movie", name: "Obsession", releaseInfo: "2025" },
      { id: "tt0074991", type: "movie", name: "Obsession", releaseInfo: "1976" },
    ]).map((film) => `${film.name} ${film.year}`)).toEqual(["Obsession 2025", "Obsession 1976"]);
  });

  it("normalizes full metadata without losing the catalog fallback", () => {
    const fallback = normalizeFilm({ id: "tt1", type: "movie", name: "One", releaseInfo: "2022" });
    expect(normalizeFilmDetails({
      id: "tt1",
      type: "movie",
      name: "One",
      releaseInfo: "2022",
      runtime: "102 min",
      genres: ["Drama", "Coming of age"],
      logo: "https://example.com/logo.png",
    }, fallback ?? undefined)).toMatchObject({
      runtime: "102 min",
      genres: ["Drama", "Coming of age"],
      logoUrl: "https://example.com/logo.png",
    });
  });

  it("groups sources by addon without exposing playback targets", () => {
    const groups = normalizeSourceGroups([
      {
        addon: { manifest: { id: "addon.one", name: "Addon One" } },
        content: {
          type: "Ready",
          content: [{ name: "1080p", description: "English · 5.1", url: "https://private.example/video" }],
        },
      },
      {
        addon: { manifest: { id: "addon.two", name: "Addon Two" } },
        content: { type: "Err" },
      },
    ]);

    expect(groups[0]).toMatchObject({ addonName: "Addon One", status: "ready" });
    expect(groups[0]?.items[0]).toEqual({
      id: "source-1-1",
      addonId: "addon.one",
      addonName: "Addon One",
      name: "1080p",
      description: "English · 5.1",
      quality: "hd",
      audioChannels: "5.1",
      playbackSupport: "internal",
      unavailableReason: null,
    });
    expect(groups[1]?.error).toBe("Unable to load sources from Addon Two.");
    expect(JSON.stringify(groups)).not.toContain("private.example");
  });

  it("parses source quality without promoting unknown sources", () => {
    expect(parseSourceQuality("2160p HDR")).toBe("4k");
    expect(parseSourceQuality("UHD remux")).toBe("4k");
    expect(parseSourceQuality("1080p WEB-DL")).toBe("hd");
    expect(parseSourceQuality("720p")).toBe("hd");
    expect(parseSourceQuality("CAM")).toBeNull();
  });

  it("detects 5.1 audio channels without promoting unrelated decimals", () => {
    expect(parseSourceAudioChannels("English · 5.1")).toBe("5.1");
    expect(parseSourceAudioChannels("DDP5.1 Atmos")).toBe("5.1");
    expect(parseSourceAudioChannels("Version 5.1.2")).toBeNull();
    expect(parseSourceAudioChannels("Stereo")).toBeNull();
  });

  it("chooses the first internal source in addon and item order", () => {
    const groups = normalizeSourceGroups([
      {
        addon: { manifest: { id: "one", name: "One" } },
        content: { type: "Ready", content: [
          { name: "Website", externalUrl: "https://example.com" },
          { name: "First playable", url: "https://private.example/first" },
        ] },
      },
      {
        addon: { manifest: { id: "two", name: "Two" } },
        content: { type: "Ready", content: [
          { name: "Later playable", url: "https://private.example/later" },
        ] },
      },
    ]);
    expect(firstPlayableSource(groups)?.name).toBe("First playable");

    groups[0].items[1].unavailableReason = "The server for this source is not responding right now.";
    expect(firstPlayableSource(groups)?.name).toBe("Later playable");
    groups[1].items[0].unavailableReason = "The server for this source is not responding right now.";
    expect(firstPlayableSource(groups)?.name).toBe("First playable");
  });

  it("drops Stremio local-files groups before UI source tabs", () => {
    expect(isLocalFilesAddon("org.stremio.local", "Local Files (without catalog support)")).toBe(true);
    expect(isLocalFilesAddon("com.example.streams", "AIOStreams")).toBe(false);

    const groups = normalizeSourceGroups([
      {
        addon: { manifest: { id: "org.stremio.local", name: "Local Files (without catalog support)" } },
        content: { type: "Ready", content: [{ name: "movie.mkv", url: "file:///private" }] },
      },
      {
        addon: { manifest: { id: "com.example.streams", name: "AIOStreams" } },
        content: { type: "Ready", content: [{ name: "1080p", url: "https://private.example/video" }] },
      },
    ]);

    expect(groups).toHaveLength(1);
    expect(groups[0]).toMatchObject({ addonId: "com.example.streams", addonName: "AIOStreams" });
    expect(groups[0]?.items[0]?.id).toBe("source-1-1");
  });

  it("keeps only AIOStreams groups for Panorama playback", () => {
    expect(isAioStreamsAddon("com.stremio.aiostreams", "AIOStreams")).toBe(true);
    expect(isAioStreamsAddon("community.aio-streams", "Custom name")).toBe(true);
    expect(isAioStreamsAddon("org.peario", "Peario")).toBe(false);
    expect(isAioStreamsAddon("org.stremio.local", "Local Files")).toBe(false);

    const groups = onlyAioStreamsSourceGroups([
      {
        addon: { manifest: { id: "org.peario", name: "Peario" } },
        content: { type: "Ready", content: [{ name: "Peario source", url: "https://private.example/peario" }] },
      },
      {
        addon: { manifest: { id: "com.stremio.aiostreams", name: "AIOStreams" } },
        content: { type: "Ready", content: [{ name: "AIO source", url: "https://private.example/aio" }] },
      },
      {
        addon: { manifest: { id: "org.stremio.local", name: "Local Files (without catalog support)" } },
        content: { type: "Ready", content: [{ name: "Local source", url: "file:///private" }] },
      },
    ]);

    expect(groups).toHaveLength(1);
    expect(normalizeSourceGroups(groups)[0]).toMatchObject({
      addonId: "com.stremio.aiostreams",
      addonName: "AIOStreams",
    });
  });

  it("rejects stale or mismatched detail responses", () => {
    expect(isCurrentFilmRequest(3, 3, "tt-current", "tt-current")).toBe(true);
    expect(isCurrentFilmRequest(2, 3, "tt-current", "tt-current")).toBe(false);
    expect(isCurrentFilmRequest(3, 3, "tt-current", "tt-previous")).toBe(false);
  });

  it("classifies source playback without exposing its target", () => {
    expect(normalizePlaybackSupport({ url: "https://example.com/video" })).toBe("internal");
    expect(normalizePlaybackSupport({ infoHash: "private-hash" })).toBe("internal");
    expect(normalizePlaybackSupport({ externalUrl: "https://external.example" })).toBe("external");
    expect(normalizePlaybackSupport({})).toBe("unsupported");
  });

  it("bounds session subtitle size and timing", () => {
    expect(clampSubtitleSize(40)).toBe(50);
    expect(clampSubtitleSize(138.4)).toBe(138);
    expect(clampSubtitleSize(340)).toBe(300);
    expect(clampSubtitleOffset(-12)).toBe(-10);
    expect(clampSubtitleOffset(1.24)).toBe(1);
    expect(clampSubtitleOffset(12)).toBe(10);
    expect(clampSubtitleVerticalPosition(-4)).toBe(0);
    expect(clampSubtitleVerticalPosition(12.4)).toBe(12);
    expect(clampSubtitleVerticalPosition(103)).toBe(100);
    expect(clampSubtitlePaddingX(40)).toBe(40);
    expect(clampSubtitlePaddingY(-2)).toBe(0);
  });

  it("keeps only English and Indonesian in the subtitle language column", () => {
    expect(subtitleLanguageId("en", "English")).toBe("en");
    expect(subtitleLanguageId("eng")).toBe("en");
    expect(subtitleLanguageId("id", "Bahasa Indonesia")).toBe("id");
    expect(subtitleLanguageId("ind")).toBe("id");
    expect(subtitleLanguageId("bg", "Bulgarian")).toBeNull();
    expect(subtitleLanguageName("id")).toBe("Indonesian");
  });

  it("chooses the topmost English subtitle even when it is forced", () => {
    expect(preferredEnglishSubtitleTrack([
      { id: "id-embedded", label: "Indonesian", language: "id", origin: "embedded", sourceLabel: "Embedded 1" },
      { id: "en-forced", label: "English", language: "eng", origin: "embedded", sourceLabel: "Embedded 1 · Forced" },
      { id: "en-embedded", label: "English", language: "eng", origin: "embedded", sourceLabel: "Embedded 2 · eng" },
      { id: "en-addon", label: "English SDH", language: "en", origin: "addon", sourceLabel: "OpenSubtitles" },
    ])?.id).toBe("en-forced");
  });

  it("chooses the topmost English audio track by code or label", () => {
    expect(preferredEnglishAudioTrack([
      { id: "ja", label: "Japanese", language: "jpn", description: null },
      { id: "en", label: "English", language: "eng", description: "5.1" },
      { id: "en-alt", label: "English commentary", language: "en", description: null },
    ])?.id).toBe("en");
    expect(preferredEnglishAudioTrack([
      { id: "unknown", label: "English", language: null, description: null },
    ])?.id).toBe("unknown");
  });

  it("falls back to forced English when it is the only English track", () => {
    expect(preferredEnglishSubtitleTrack([
      { id: "id-embedded", label: "Indonesian", language: "id", origin: "embedded", sourceLabel: "Embedded 1" },
      { id: "en-forced", label: "English", language: "eng", origin: "embedded", sourceLabel: "Embedded 2 · Forced" },
    ])?.id).toBe("en-forced");
  });

  it("numbers embedded subtitle sources and keeps their description", () => {
    expect(embeddedSubtitleSourceLabel(0, null)).toBe("Embedded 1");
    expect(embeddedSubtitleSourceLabel(1, "English SDH")).toBe("Embedded 2 · English SDH");
  });

  it("maps stream-attached extra subtitles into the engine track shape", () => {
    expect(extraSubtitleTracksFromStream({
      url: "https://example.com/video.mkv",
      subtitles: [
        { id: "aio-id", url: "https://example.com/id.srt", lang: "id" },
        { url: "https://example.com/en.vtt", lang: "en", label: "English SDH" },
        { lang: "fr" },
      ],
    }, "AIOStreams")).toEqual([
      {
        id: "aio-id",
        url: "https://example.com/id.srt",
        lang: "id",
        label: "id",
        origin: "AIOStreams",
      },
      {
        id: "AIOStreams:https://example.com/en.vtt",
        url: "https://example.com/en.vtt",
        lang: "en",
        label: "English SDH",
        origin: "AIOStreams",
      },
    ]);
    expect(extraSubtitleTracksFromStream([
      { name: "resolved" },
      { subtitles: [{ id: "aio-id", url: "https://example.com/id.srt", lang: "id" }] },
    ], "AIOStreams")).toEqual([
      {
        id: "aio-id",
        url: "https://example.com/id.srt",
        lang: "id",
        label: "id",
        origin: "AIOStreams",
      },
    ]);
  });

  it("flattens core subtitle addon groups and remaps exclusive origins", () => {
    const originByTransport = new Map([
      ["https://opensubtitles.example/manifest.json", "OpenSubtitles"],
    ]);
    expect(extraSubtitleTracksFromCore([
      {
        request: { base: "https://opensubtitles.example/manifest.json" },
        content: {
          type: "Ready",
          content: [{ id: "os-en", url: "https://example.com/os-en.srt", lang: "eng" }],
        },
      },
      {
        addon: { manifest: { name: "Community Subs" } },
        content: { type: "Loading", content: [] },
      },
    ], originByTransport)).toEqual([
      {
        id: "os-en",
        url: "https://example.com/os-en.srt",
        lang: "eng",
        label: "eng",
        origin: "OpenSubtitles",
      },
    ]);
    expect(extraSubtitleSourceLabel("EXCLUSIVE", "AIOStreams")).toBe("AIOStreams");
    expect(extraSubtitleSourceLabel("OpenSubtitles", "AIOStreams")).toBe("OpenSubtitles");
  });

  it("flattens serialized player subtitle tracks from core-web", () => {
    const originByTransport = new Map([
      ["https://opensubtitles.example/manifest.json", "OpenSubtitles"],
    ]);
    expect(extraSubtitleTracksFromCore([
      {
        id: "https://opensubtitles.example/manifest.json_0",
        addonSubtitleId: "os-en",
        url: "https://example.com/os-en.srt",
        lang: "eng",
        origin: "OpenSubtitles",
      },
      {
        id: 17,
        url: { href: "https://example.com/id.srt" },
        language: "id",
        origin: "https://opensubtitles.example/manifest.json",
      },
      {
        request: { base: "https://community.example/manifest.json" },
        content: { type: "Loading", content: [] },
      },
    ], originByTransport)).toEqual([
      {
        id: "https://opensubtitles.example/manifest.json_0",
        url: "https://example.com/os-en.srt",
        lang: "eng",
        label: "eng",
        origin: "OpenSubtitles",
      },
      {
        id: "17",
        url: "https://example.com/id.srt",
        lang: "id",
        label: "id",
        origin: "OpenSubtitles",
      },
    ]);
  });

  it("reads stream-attached subtitle urls from href-shaped values", () => {
    expect(extraSubtitleTracksFromStream({
      selected: true,
      stream: {
        url: "https://example.com/video.mkv",
        subtitles: [{ id: "aio-id", url: { href: "https://example.com/id.srt" }, lang: "id" }],
      },
    }, "AIOStreams")).toEqual([
      {
        id: "aio-id",
        url: "https://example.com/id.srt",
        lang: "id",
        label: "id",
        origin: "AIOStreams",
      },
    ]);
  });

  it("reads addon-response wrapped core subtitle groups and colliding ids", () => {
    expect(extraSubtitleTracksFromCore([
      {
        request: { base: "https://opensubtitles.example/manifest.json" },
        content: {
          type: "Ready",
          content: { subtitles: [{ id: "os-en", url: "https://example.com/os-en.srt", lang: "eng" }] },
        },
      },
    ])).toEqual([
      {
        id: "os-en",
        url: "https://example.com/os-en.srt",
        lang: "eng",
        label: "eng",
        origin: "Addon",
      },
    ]);
    expect(uniqueExtraSubtitleTracks([
      { id: "en", url: "https://example.com/a.srt", lang: "en", label: "en", origin: "OpenSubtitles" },
      { id: "en", url: "https://example.com/b.srt", lang: "en", label: "en", origin: "OpenSubtitles" },
    ]).map((track) => track.id)).toEqual(["en", "OpenSubtitles:https://example.com/b.srt"]);
  });

  it("normalizes eligible Stremio resume state without exposing library data", () => {
    expect(normalizeResumeState({
      _id: "private-library-id",
      state: { timeOffset: 2_520_000, duration: 6_120_000 },
    })).toEqual({ available: true, offset: 2520, duration: 6120 });
    expect(normalizeResumeState({ state: { timeOffset: 29_000, duration: 6_120_000 } }).available).toBe(false);
    expect(normalizeResumeState({ state: { timeOffset: 6_070_000, duration: 6_120_000 } }).available).toBe(false);
  });
});


describe("watchlist normalization", () => {
  it.each([
    [{ content: { type: "Ready", content: { inLibrary: true } } }, null, true],
    [{ content: { type: "Ready", content: { inLibrary: false } } }, { removed: false, temp: false }, false],
    [null, { removed: false, temp: false }, true],
    [null, { removed: true, temp: false }, false],
    [null, { removed: false, temp: true }, false],
    [null, null, false],
    [{ content: { type: "Loading" } }, { removed: false, temp: false }, true],
  ])("normalizes meta %j and library item %j to %s", (meta, libraryItem, saved) => {
    expect(normalizeWatchlisted(meta, libraryItem)).toBe(saved);
  });
});
