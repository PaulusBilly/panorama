import { describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { MpvController } from "../../desktop/main/mpv-controller";
import { MpvHost } from "../../desktop/native/mpv-host/mpv-host";
import type { MpvHostCommand, MpvHostDiagnostics, NativeMpvBinding } from "../../desktop/native/mpv-host/types";

class DiagnosticBinding implements NativeMpvBinding {
  diagnostics: MpvHostDiagnostics = {
    mpvVersion: "0.41.0",
    videoCodec: "h264",
    hardwareDecoder: "videotoolbox",
    buffering: false,
    cacheSeconds: 42,
    cacheEndSeconds: 162,
    cacheBufferingPercent: 73,
    cacheForwardBytes: 1048576,
    inputBytesPerSecond: 12500000,
    audioSampleRate: 48000,
    audioOutputSampleRate: 48000,
    audioCodec: "truehd",
    audioChannels: "7.1",
    audioOutputChannels: "stereo",
    videoPixelFormat: "p010",
    videoColorPrimaries: "bt.2020",
    videoTransferFunction: "pq",
    timeSeconds: 120,
    durationSeconds: 4800,
    path: "http://127.0.0.1:11471/sensitive-token",
    sourceFps: 24,
    displayFps: 60,
    frameDropCount: 2,
    decoderFrameDropCount: 0,
    mistimedFrameCount: 1,
    delayedFrameCount: 3,
    renderUpdates: 3000,
    renderedFrames: 2998,
    reportedSwaps: 2998,
    renderReady: true,
    tracks: [],
  };
  commands: MpvHostCommand[] = [];
  dispatch(command: MpvHostCommand): void {
    this.commands.push(command);
    if (command.type === "shellCommand" && command.args[0] === "loadfile") {
      Object.assign(this.diagnostics, { loadGeneration: (this.diagnostics.loadGeneration ?? 0) + 1,
        ownedEntryId: (this.diagnostics.ownedEntryId ?? 0) + 1, fileStarted: false, fileLoaded: false,
        decodedReady: false, presented: false, moving: false });
    }
    if (command.type === "seek") Object.assign(this.diagnostics, {
      seekGeneration: (this.diagnostics.seekGeneration ?? 0) + 1, decodedReady: false, presented: false, moving: false,
    });
  }
  restart(presented = true, moving = false): void {
    Object.assign(this.diagnostics, { fileStarted: true, fileLoaded: true, decodedReady: true, presented, moving,
      restartedSeekGeneration: this.diagnostics.seekGeneration ?? 0,
      presentedSeekGeneration: presented ? this.diagnostics.seekGeneration ?? 0 : 0 });
  }
  destroy(): void {}
  getDiagnostics(): MpvHostDiagnostics {
    return this.diagnostics;
  }
}

describe("native playback diagnostics", () => {
  it.skipIf(process.platform === "win32")("rejects stale entry events and stale seek generations, and rolls back rejected seeks, in the shared native state", () => {
    const directory = mkdtempSync(path.join(tmpdir(), "panorama-playback-state-"));
    try {
      mkdirSync(path.join(directory, "mpv"));
      writeFileSync(path.join(directory, "mpv/client.h"), `#pragma once
#include <stdint.h>
enum mpv_event_id { MPV_EVENT_START_FILE, MPV_EVENT_END_FILE, MPV_EVENT_FILE_LOADED, MPV_EVENT_COMMAND_REPLY, MPV_EVENT_SEEK, MPV_EVENT_PLAYBACK_RESTART };
struct mpv_event_start_file { int64_t playlist_entry_id; };
struct mpv_event_end_file { int64_t playlist_entry_id; };
struct mpv_event { mpv_event_id event_id; int error; uint64_t reply_userdata; void *data; };
`);
      writeFileSync(path.join(directory, "check.cc"), `#include "playback-state.h"
#include <cassert>
int main() {
  PlaybackState state;
  auto event = [&](mpv_event_id id, void *data = nullptr, uint64_t token = 0) {
    mpv_event value{id, 0, token, data}; return state.Event(value);
  };
  mpv_event_start_file old{10}, current{11};
  state.Replace([] { return 10; });
  event(MPV_EVENT_START_FILE, &old);
  event(MPV_EVENT_FILE_LOADED);
  assert(event(MPV_EVENT_PLAYBACK_RESTART));
  const auto oldFrame = state.FrameToken();
  state.Replace([] { return 11; });
  event(MPV_EVENT_START_FILE, &old);
  event(MPV_EVENT_FILE_LOADED);
  assert(!event(MPV_EVENT_PLAYBACK_RESTART));
  state.Frame(oldFrame);
  assert(!state.Snapshot().file_started && !state.Snapshot().presented);
  event(MPV_EVENT_START_FILE, &current);
  event(MPV_EVENT_FILE_LOADED);
  state.Frame(state.FrameToken());
  assert(!state.Snapshot().presented);
  assert(event(MPV_EVENT_PLAYBACK_RESTART));
  assert(state.Snapshot().decoded_ready && !state.Snapshot().presented);
  state.Frame(state.FrameToken());
  assert(state.Snapshot().presented);
  assert(!state.Sample(0, false, true, false).moving);
  assert(!state.Sample(1, false, false, false).moving);
  assert(state.Sample(1.1, false, false, false).moving);
  uint64_t first, second;
  const auto priorFrame = state.FrameToken();
  state.Seek([&](uint64_t token) { first = token; return 0; });
  event(MPV_EVENT_COMMAND_REPLY, nullptr, first);
  event(MPV_EVENT_SEEK);
  state.Seek([&](uint64_t token) { second = token; return 0; });
  assert(!event(MPV_EVENT_PLAYBACK_RESTART));
  event(MPV_EVENT_COMMAND_REPLY, nullptr, first);
  event(MPV_EVENT_SEEK);
  assert(!event(MPV_EVENT_PLAYBACK_RESTART));
  event(MPV_EVENT_COMMAND_REPLY, nullptr, second);
  assert(!event(MPV_EVENT_PLAYBACK_RESTART));
  event(MPV_EVENT_SEEK);
  assert(event(MPV_EVENT_PLAYBACK_RESTART));
  state.Frame(priorFrame);
  assert(!state.Snapshot().presented);
  state.Frame(state.FrameToken());
  assert(state.Snapshot().presented_seek_generation == 2);
  assert(state.Sample(40, true, false, false).presented);
  assert(!state.Sample(41, true, false, false).moving);
  state.Replace([] { return 12; });
  mpv_event_start_file next{12};
  event(MPV_EVENT_START_FILE, &next);
  event(MPV_EVENT_FILE_LOADED);
  assert(event(MPV_EVENT_PLAYBACK_RESTART));
  state.Replace([] { return 13; });
  mpv_event_start_file early{13};
  uint64_t rejected;
  state.Seek([&](uint64_t token) { rejected = token; return 0; });
  mpv_event failure{MPV_EVENT_COMMAND_REPLY, -12, rejected, nullptr};
  state.Event(failure);
  event(MPV_EVENT_START_FILE, &early);
  event(MPV_EVENT_FILE_LOADED);
  assert(event(MPV_EVENT_PLAYBACK_RESTART));
  state.Frame(state.FrameToken());
  state.Seek([&](uint64_t token) { rejected = token; return 0; });
  assert(!state.Snapshot().presented);
  failure.reply_userdata = rejected;
  state.Event(failure);
  assert(state.Snapshot().presented && state.Snapshot().presented_seek_generation == state.Snapshot().seek_generation);
  state.Seek([](uint64_t) { return -1; });
  assert(state.Snapshot().decoded_ready && state.Snapshot().presented);
  state.Seek([&](uint64_t token) { rejected = token; return 0; });
  state.Replace([] { return 14; });
  failure.reply_userdata = rejected;
  state.Event(failure);
  assert(!state.Snapshot().decoded_ready && !state.Snapshot().presented);
  mpv_event_start_file replacement{14};
  event(MPV_EVENT_START_FILE, &replacement);
  event(MPV_EVENT_FILE_LOADED);
  assert(!state.Snapshot().decoded_ready && !state.Snapshot().presented);
  assert(event(MPV_EVENT_PLAYBACK_RESTART));
  state.Frame(state.FrameToken());
  assert(state.Snapshot().decoded_ready && state.Snapshot().presented);
  for (int drained = 0; drained <= 3; drained++) {
    mpv_event_start_file incoming{20 + drained};
    assert(state.Replace([&] {
      assert(state.Snapshot().owned_entry == -1);
      if (drained >= 1) event(MPV_EVENT_START_FILE, &incoming);
      if (drained >= 2) event(MPV_EVENT_FILE_LOADED);
      if (drained >= 3) assert(!event(MPV_EVENT_PLAYBACK_RESTART));
      return incoming.playlist_entry_id;
    }));
    auto snapshot = state.Snapshot();
    assert(snapshot.file_started == (drained >= 1));
    assert(snapshot.file_loaded == (drained >= 2));
    assert(snapshot.decoded_ready == (drained >= 3));
    if (drained < 1) event(MPV_EVENT_START_FILE, &incoming);
    if (drained < 2) event(MPV_EVENT_FILE_LOADED);
    if (drained < 3) assert(event(MPV_EVENT_PLAYBACK_RESTART));
    snapshot = state.Snapshot();
    assert(snapshot.restart_sequence == 1 && snapshot.restarted_seek_generation == snapshot.seek_generation);
    state.Frame(state.FrameToken());
    assert(state.Snapshot().presented);
  }
  mpv_event_start_file newest{31};
  assert(!state.Replace([&] {
    assert(state.Replace([&] {
      event(MPV_EVENT_START_FILE, &newest);
      event(MPV_EVENT_FILE_LOADED);
      event(MPV_EVENT_PLAYBACK_RESTART);
      return 31;
    }));
    return 30;
  }));
  assert(state.Snapshot().owned_entry == 31 && state.Snapshot().decoded_ready);
  state.Frame(state.FrameToken());
  assert(state.Snapshot().presented);
}
`);
      const executable = path.join(directory, "check");
      execFileSync("c++", ["-std=c++20", "-pthread", "-I", directory, "-I", path.resolve("desktop/native/mpv-host/src"), path.join(directory, "check.cc"), "-o", executable]);
      execFileSync(executable, { timeout: 5_000 });
    } finally { rmSync(directory, { recursive: true, force: true }); }
  }, 30_000);

  it("waits for owned restart and presentation across replacement and pre-seeking redraws", () => {
    const binding = new DiagnosticBinding();
    const events: unknown[] = [];
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11471"), (channel, payload) => events.push([channel, payload]), 60_000);
    try {
      controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11471/first"]);
      binding.restart();
      Object.assign(binding.diagnostics, { videoWidth: 1920, videoHeight: 1080 });
      controller.poll();
      controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11471/second"]);
      binding.diagnostics.renderedFrames! += 10;
      controller.poll();
      expect(events).not.toContainEqual(["mpv-event-video-ready", { loadId: 2, ready: true }]);
      binding.restart(false);
      controller.poll();
      expect(events).not.toContainEqual(["mpv-event-video-ready", { loadId: 2, ready: true }]);
      binding.restart();
      controller.poll();
      expect(events).toContainEqual(["mpv-event-video-ready", { loadId: 2, ready: true }]);
      controller.recordRendererTiming("seek-requested");
      controller.handleSend("mpv-set-prop", ["time-pos", 20]);
      binding.diagnostics.renderedFrames! += 1;
      controller.poll();
      expect(controller.getPlaybackDiagnostics().lastSeekToFirstFrameMs).toBeNull();
      controller.recordRendererTiming("seek-requested");
      controller.handleSend("mpv-set-prop", ["time-pos", 40]);
      Object.assign(binding.diagnostics, { decodedReady: true, presented: true, restartedSeekGeneration: 1, presentedSeekGeneration: 1 });
      controller.poll();
      expect(controller.getPlaybackDiagnostics().lastSeekToFirstFrameMs).toBeNull();
      binding.diagnostics.paused = true;
      binding.restart(false);
      controller.poll();
      expect(controller.getPlaybackDiagnostics().lastSeekToFirstFrameMs).toBeNull();
      binding.restart();
      controller.poll();
      expect(controller.getPlaybackDiagnostics().lastSeekToFirstFrameMs).not.toBeNull();
    } finally { controller.destroy(); }
  });

  it("keeps initial and seek cache waits at two seconds until movement and then counts recovery stalls", () => {
    const binding = new DiagnosticBinding();
    let now = 0;
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11471"), () => undefined, 60_000, () => now);
    const waits = () => binding.commands.filter((command) => command.type === "setProperty" && command.name === "cache-pause-wait");
    try {
      controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11471/first"]);
      Object.assign(binding.diagnostics, { paused: false, videoWidth: 1920, videoHeight: 1080, buffering: true });
      binding.restart();
      controller.poll();
      now = 200;
      controller.poll();
      expect(controller.getPlaybackDiagnostics().rebufferCount).toBe(0);
      expect(waits().map((command) => command.type === "setProperty" ? command.value : null)).toEqual([2]);
      binding.diagnostics.buffering = false;
      controller.poll();
      expect(waits()).toHaveLength(1);
      binding.diagnostics.moving = true;
      controller.poll();
      expect(waits().at(-1)).toMatchObject({ value: 5 });
      controller.recordRendererTiming("seek-requested");
      controller.handleSend("mpv-set-prop", ["time-pos", 40]);
      binding.diagnostics.buffering = true;
      binding.restart();
      controller.poll();
      expect(controller.getPlaybackDiagnostics().rebufferCount).toBe(0);
      expect(waits().at(-1)).toMatchObject({ value: 2 });
      binding.diagnostics.buffering = false;
      binding.diagnostics.paused = true;
      controller.poll();
      expect(waits().at(-1)).toMatchObject({ value: 2 });
      binding.diagnostics.paused = false;
      binding.diagnostics.moving = true;
      controller.poll();
      expect(waits().at(-1)).toMatchObject({ value: 5 });
      now = 300;
      binding.diagnostics.buffering = true;
      controller.poll();
      now = 800;
      binding.diagnostics.buffering = false;
      controller.poll();
      expect(controller.getPlaybackDiagnostics()).toMatchObject({ rebufferCount: 1, rebufferMilliseconds: 500 });
      expect(controller.getPlaybackDiagnostics().timeline.map(({ event }) => event)).toEqual(expect.arrayContaining(["playback-restart", "cache-exit", "playback-moving", "seek-restart", "seek-cache-exit", "seek-moving"]));
    } finally { controller.destroy(); }
  });

  it("abandons an acknowledged seek without a restart after ten seconds and restores the cache wait once", () => {
    const binding = new DiagnosticBinding();
    let now = 0;
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11471"), () => undefined, 60_000, () => now);
    const waits = () => binding.commands.filter((command) => command.type === "setProperty" && command.name === "cache-pause-wait");
    try {
      controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11471/first"]);
      Object.assign(binding.diagnostics, { paused: false, seeking: false, videoWidth: 1920, videoHeight: 1080 });
      binding.restart(true, true);
      controller.poll();
      controller.recordRendererTiming("seek-requested");
      controller.handleSend("mpv-set-prop", ["time-pos", 40]);
      now = 9_999;
      controller.poll();
      expect(controller).toHaveProperty("pendingSeekGeneration", 1);
      expect(waits().at(-1)).toMatchObject({ value: 2 });
      now = 10_001;
      controller.poll();
      expect(controller).toHaveProperty("pendingSeekGeneration", null);
      expect(controller).toHaveProperty("awaitingMovement", false);
      expect(waits().at(-1)).toMatchObject({ value: 5 });
      const restored = waits().length;
      binding.diagnostics.moving = true;
      controller.poll();
      binding.diagnostics.buffering = true;
      controller.poll();
      expect(waits()).toHaveLength(restored);
      expect(controller.getPlaybackDiagnostics().rebufferCount).toBe(1);
      expect(controller.getPlaybackDiagnostics().timeline.map(({ event }) => event)).not.toContain("seek-first-frame");
    } finally { controller.destroy(); }
  });

  it("keeps a normally completed seek waiting for movement beyond its cancelled deadline", () => {
    const binding = new DiagnosticBinding();
    let now = 0;
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11471"), () => undefined, 60_000, () => now);
    const waits = () => binding.commands.filter((command) => command.type === "setProperty" && command.name === "cache-pause-wait");
    try {
      controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11471/first"]);
      Object.assign(binding.diagnostics, { paused: false, seeking: false, videoWidth: 1920, videoHeight: 1080 });
      binding.restart(true, true);
      controller.poll();
      controller.recordRendererTiming("seek-requested");
      controller.handleSend("mpv-set-prop", ["time-pos", 40]);
      now = 500;
      binding.restart();
      controller.poll();
      expect(controller).toHaveProperty("pendingSeekGeneration", null);
      expect(controller.getPlaybackDiagnostics().lastSeekToFirstFrameMs).toBe(500);
      now = 10_001;
      controller.poll();
      expect(waits().at(-1)).toMatchObject({ value: 2 });
      binding.diagnostics.moving = true;
      controller.poll();
      expect(waits().at(-1)).toMatchObject({ value: 5 });
      expect(controller.getPlaybackDiagnostics().timeline.filter(({ event }) => event === "seek-first-frame")).toHaveLength(1);
    } finally { controller.destroy(); }
  });

  it("keeps a restarted seek pending past its deadline until presentation", () => {
    const binding = new DiagnosticBinding();
    let now = 0;
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11471"), () => undefined, 60_000, () => now);
    try {
      controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11471/first"]);
      Object.assign(binding.diagnostics, { paused: false, seeking: false, videoWidth: 1920, videoHeight: 1080 });
      binding.restart(true, true);
      controller.poll();
      controller.recordRendererTiming("seek-requested");
      controller.handleSend("mpv-set-prop", ["time-pos", 40]);
      binding.restart(false);
      now = 10_001;
      controller.poll();
      expect(controller).toHaveProperty("pendingSeekGeneration", 1);
      now = 11_000;
      binding.restart();
      controller.poll();
      expect(controller.getPlaybackDiagnostics().lastSeekToFirstFrameMs).toBe(11_000);
    } finally { controller.destroy(); }
  });

  it("replaces the seek deadline on a newer seek and cancels it on a load", () => {
    const binding = new DiagnosticBinding();
    let now = 0;
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11471"), () => undefined, 60_000, () => now);
    const waits = () => binding.commands.filter((command) => command.type === "setProperty" && command.name === "cache-pause-wait");
    try {
      controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11471/first"]);
      binding.diagnostics.seeking = false;
      controller.handleSend("mpv-set-prop", ["time-pos", 40]);
      now = 9_000;
      controller.handleSend("mpv-set-prop", ["time-pos", 80]);
      now = 10_001;
      controller.poll();
      expect(controller).toHaveProperty("pendingSeekGeneration", 2);
      expect(waits().at(-1)).toMatchObject({ value: 2 });
      now = 19_001;
      binding.diagnostics.seeking = true;
      controller.poll();
      expect(controller).toHaveProperty("pendingSeekGeneration", 2);
      binding.diagnostics.seeking = false;
      controller.poll();
      expect(controller).toHaveProperty("pendingSeekGeneration", null);
      expect(waits().at(-1)).toMatchObject({ value: 5 });
      controller.handleSend("mpv-set-prop", ["time-pos", 120]);
      controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11471/second"]);
      now = 30_000;
      controller.poll();
      expect(controller).toHaveProperty("pendingSeekGeneration", null);
      expect(waits().at(-1)).toMatchObject({ value: 2 });
      expect(controller.getPlaybackDiagnostics().timeline.map(({ event }) => event)).not.toContain("seek-first-frame");
    } finally { controller.destroy(); }
  });

  it("keeps the native renderer and accepts ShellVideo disabling its absent controller", () => {
    const binding = new DiagnosticBinding();
    binding.dispatch = () => { throw new Error("Unsupported native setting"); };
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11471"), () => undefined, 60_000);
    try {
      expect(() => controller.handleSend("mpv-set-prop", ["osc", "no"])).not.toThrow();
      expect(() => controller.handleSend("mpv-set-prop", ["vo", "libmpv"])).not.toThrow();
      expect(() => controller.handleSend("mpv-set-prop", ["pause", false])).not.toThrow();
      expect(() => controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11471/media"]))
        .toThrow("Unsupported native setting");
    } finally { controller.destroy(); }
  });
  it("counts recovery stalls after startup and resets seek timing on source changes", () => {
    const binding = new DiagnosticBinding();
    let now = 0;
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11471"), () => undefined, 60_000, () => now);
    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11471/first"]);
    Object.assign(binding.diagnostics, { paused: false, videoWidth: 1920, videoHeight: 1080, buffering: true });
    controller.poll();
    expect(controller.getPlaybackDiagnostics().rebufferCount).toBe(0);
    binding.diagnostics.renderedFrames! += 1;
    binding.diagnostics.buffering = false;
    binding.restart(true, true);
    controller.poll();
    now = 100;
    binding.diagnostics.buffering = true;
    controller.poll();
    now = 600;
    binding.diagnostics.buffering = false;
    controller.poll();
    expect(controller.getPlaybackDiagnostics()).toMatchObject({ rebufferCount: 1, rebufferMilliseconds: 500 });
    controller.recordRendererTiming("seek-requested");
    controller.handleSend("mpv-set-prop", ["time-pos", 40]);
    binding.diagnostics.buffering = true;
    binding.diagnostics.seeking = true;
    controller.poll();
    expect(controller.getPlaybackDiagnostics().rebufferCount).toBe(1);
    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11471/second"]);
    expect(controller.getPlaybackDiagnostics()).toMatchObject({ rebufferCount: 0, rebufferMilliseconds: 0, lastSeekToFirstFrameMs: null });
    controller.destroy();
  });

  it("keeps unavailable render counters unknown while using owned presentation states", () => {
    const binding = new DiagnosticBinding();
    Object.assign(binding.diagnostics, { rendererBackend: "d3d11", renderedFrames: null, renderUpdates: null, reportedSwaps: null, videoWidth: 3840, videoHeight: 2160 });
    const events: unknown[] = [];
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11471"), (channel, payload) => events.push([channel, payload]), 60_000);
    controller.handleSend("mpv-command", ["loadfile", "http://127.0.0.1:11471/media"]);
    binding.restart();
    controller.poll();
    expect(events).toContainEqual(["mpv-event-video-ready", { loadId: 1, ready: true }]);
    expect(controller.getPlaybackDiagnostics()).toMatchObject({ rendererBackend: "d3d11", renderedFrames: null, renderUpdates: null, reportedSwaps: null });
    controller.destroy();
  });

  it("returns frame-pacing evidence without exposing the playback target", () => {
    const controller = new MpvController(
      new MpvHost(new DiagnosticBinding(), "http://127.0.0.1:11471"),
      () => undefined,
      60_000,
    );

    const diagnostics = controller.getPlaybackDiagnostics();

    expect(diagnostics).toEqual({
      mpvVersion: "0.41.0",
      videoCodec: "h264",
      hardwareDecoder: "videotoolbox",
      rendererBackend: null,
      rendererFallbackReason: null,
      audioOutputFormat: null,
      audioOutputDriver: null,
      videoOutputPrimaries: null,
      videoOutputTransferFunction: null,
      buffering: false,
      ownedEntryId: null,
      loadGeneration: undefined,
      fileStarted: false,
      fileLoaded: false,
      decodedReady: false,
      presented: false,
      presentationEvidence: undefined,
      moving: false,
      seekGeneration: undefined,
      restartedSeekGeneration: undefined,
      presentedSeekGeneration: undefined,
      rebufferCount: 0,
      rebufferMilliseconds: 0,
      cacheSeconds: 42,
      cacheEndSeconds: 162,
      cacheBufferingPercent: 73,
      cacheForwardBytes: 1048576,
      inputBytesPerSecond: 12500000,
      downloadMbps: 100,
      sourceBitrateMbps: null,
      sourceUnreachable: false,
      resumeBufferSeconds: 5,
      proxy: null,
      audioSampleRate: 48000,
      audioOutputSampleRate: 48000,
      audioCodec: "truehd",
      audioChannels: "7.1",
      audioOutputChannels: "stereo",
      videoPixelFormat: "p010",
      videoColorPrimaries: "bt.2020",
      videoTransferFunction: "pq",
      sourceFps: 24,
      displayFps: 60,
      frameDropCount: 2,
      decoderFrameDropCount: 0,
      mistimedFrameCount: 1,
      delayedFrameCount: 3,
      videoWidth: null,
      videoHeight: null,
      renderUpdates: 3000,
      renderedFrames: 2998,
      reportedSwaps: 2998,
      sourceKind: null,
      startupTiming: {
        totalMs: null,
        preparationMs: null,
        videoDispatchMs: null,
        nativeFirstFrameMs: null,
      },
      lastSeekToFirstFrameMs: null,
      timeline: [],
    });
    expect(JSON.stringify(diagnostics)).not.toContain("sensitive-token");
    expect(diagnostics).not.toHaveProperty("path");
    controller.destroy();
  });

  it("forwards the cache endpoint and real percentage independently of duration and pause", () => {
    const binding = new DiagnosticBinding();
    const events: Array<[string, unknown]> = [];
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11471"),
      (channel, payload) => events.push([channel, payload]), 60_000);
    try {
      controller.handleSend("mpv-observe-prop", "demuxer-cache-time");
      controller.handleSend("mpv-observe-prop", "cache-buffering-state");
      expect(events).toEqual([
        ["mpv-prop-change", { name: "demuxer-cache-time", data: 162 }],
        ["mpv-prop-change", { name: "cache-buffering-state", data: 73 }],
      ]);
      expect(controller.getPlaybackDiagnostics().cacheSeconds).toBe(42);
      controller.handleSend("mpv-set-prop", ["time-pos", 600]);
      binding.diagnostics = { ...binding.diagnostics, timeSeconds: 600, cacheSeconds: 0,
        cacheEndSeconds: 600, cacheBufferingPercent: 0, seeking: true };
      controller.poll();
      expect(events.slice(-2)).toEqual([
        ["mpv-prop-change", { name: "demuxer-cache-time", data: 600 }],
        ["mpv-prop-change", { name: "cache-buffering-state", data: 0 }],
      ]);
      expect(controller.getPlaybackDiagnostics().cacheSeconds).toBe(0);
      binding.diagnostics = { ...binding.diagnostics, cacheEndSeconds: null, cacheBufferingPercent: null };
      controller.poll();
      expect(events.slice(-2)).toEqual([
        ["mpv-prop-change", { name: "demuxer-cache-time", data: null }],
        ["mpv-prop-change", { name: "cache-buffering-state", data: null }],
      ]);
    } finally {
      controller.destroy();
    }
  });

  it.each([undefined, null, NaN, Infinity, -Infinity])("normalizes unavailable or non-finite diagnostics (%s)", (value) => {
    const binding = new DiagnosticBinding();
    binding.diagnostics = { ...binding.diagnostics, cacheSeconds: value ?? null,
      cacheEndSeconds: value, cacheBufferingPercent: value, cacheForwardBytes: value,
      inputBytesPerSecond: value, audioSampleRate: value, audioOutputSampleRate: value,
      audioCodec: undefined, audioChannels: undefined, audioOutputChannels: undefined,
      videoPixelFormat: undefined, videoColorPrimaries: undefined, videoTransferFunction: undefined };
    const events: Array<[string, unknown]> = [];
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11471"),
      (channel, payload) => events.push([channel, payload]), 60_000);
    try {
      expect(controller.getPlaybackDiagnostics()).toMatchObject({ cacheSeconds: null,
        cacheEndSeconds: null, cacheBufferingPercent: null, cacheForwardBytes: null,
        inputBytesPerSecond: null, audioSampleRate: null, audioOutputSampleRate: null,
        audioCodec: null, audioChannels: null, audioOutputChannels: null,
        videoPixelFormat: null, videoColorPrimaries: null, videoTransferFunction: null });
      controller.handleSend("mpv-observe-prop", "demuxer-cache-time");
      controller.handleSend("mpv-observe-prop", "cache-buffering-state");
      expect(events).toEqual([
        ["mpv-prop-change", { name: "demuxer-cache-time", data: null }],
        ["mpv-prop-change", { name: "cache-buffering-state", data: null }],
      ]);
    } finally {
      controller.destroy();
    }
  });

  it.each(["loadfile", "stop"])("does not replay the old snapshot after %s", (command) => {
    const binding = new DiagnosticBinding();
    const events: Array<[string, unknown]> = [];
    binding.dispatch = () => {
      binding.diagnostics = { ...binding.diagnostics, cacheSeconds: null,
        cacheEndSeconds: null, cacheBufferingPercent: null };
    };
    const controller = new MpvController(new MpvHost(binding, "http://127.0.0.1:11471"),
      (channel, payload) => events.push([channel, payload]), 60_000);
    try {
      controller.handleSend("mpv-observe-prop", "demuxer-cache-time");
      controller.handleSend("mpv-command", command === "stop" ? [command] : [command, "http://127.0.0.1:11471/next"]);
      controller.handleSend("mpv-observe-prop", "demuxer-cache-time");
      expect(events.at(-1)).toEqual(["mpv-prop-change", { name: "demuxer-cache-time", data: null }]);
      expect(controller.getPlaybackDiagnostics().cacheSeconds).toBeNull();
      binding.diagnostics = { ...binding.diagnostics, cacheSeconds: 5, cacheEndSeconds: 5, cacheBufferingPercent: 100 };
      controller.poll();
      expect(events.at(-1)).toEqual(["mpv-prop-change", { name: "demuxer-cache-time", data: 5 }]);
    } finally {
      controller.destroy();
    }
  });

  it("measures startup and seek recovery without exposing the media URL", () => {
    const binding = new DiagnosticBinding();
    let now = 1_000;
    const mediaUrl = "https://media.example/sensitive-token/movie.mkv";
    const controller = new MpvController(
      new MpvHost(binding, "http://127.0.0.1:11471", mediaUrl),
      () => undefined,
      60_000,
      () => now,
    );

    controller.recordRendererTiming("play-requested");
    now = 1_180;
    controller.recordRendererTiming("preparation-ready");
    now = 1_240;
    controller.recordRendererTiming("video-load-requested");
    now = 1_260;
    controller.handleSend("mpv-command", ["loadfile", mediaUrl]);
    now = 2_100;
    binding.restart();
    binding.diagnostics = {
      ...binding.diagnostics,
      renderedFrames: 2999,
      videoWidth: 3840,
      videoHeight: 2160,
    };
    controller.poll();

    now = 3_000;
    controller.recordRendererTiming("seek-requested");
    now = 3_010;
    controller.handleSend("mpv-set-prop", ["time-pos", 1_078]);
    now = 3_420;
    binding.restart();
    binding.diagnostics = {
      ...binding.diagnostics,
      renderedFrames: 3000,
      videoWidth: 3840,
      videoHeight: 2160,
      seeking: false,
    };
    controller.poll();

    const diagnostics = controller.getPlaybackDiagnostics();

    expect(diagnostics.sourceKind).toBe("remote");
    expect(diagnostics.startupTiming).toEqual({
      totalMs: 1_100,
      preparationMs: 180,
      videoDispatchMs: 20,
      nativeFirstFrameMs: 840,
    });
    expect(diagnostics.lastSeekToFirstFrameMs).toBe(420);
    expect(diagnostics.timeline.map(({ event, elapsedMs }) => [event, elapsedMs])).toEqual([
      ["play-requested", 0],
      ["preparation-ready", 180],
      ["video-load-requested", 240],
      ["loadfile-issued", 260],
      ["playback-restart", 1_100],
      ["cache-exit", 1_100],
      ["first-frame", 1_100],
      ["seek-requested", 2_000],
      ["seek-dispatched", 2_010],
      ["seek-restart", 2_420],
      ["seek-cache-exit", 2_420],
      ["seek-first-frame", 2_420],
    ]);
    expect(JSON.stringify(diagnostics)).not.toContain("sensitive-token");
    expect(JSON.stringify(diagnostics)).not.toContain("media.example");
    controller.destroy();
  });
});
