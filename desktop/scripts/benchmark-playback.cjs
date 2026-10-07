const { app, BrowserWindow } = require('electron');
const path = require('node:path');
const rootIndex = process.argv.indexOf('--compiled-root');
const compiledRoot = rootIndex >= 0 ? path.resolve(process.argv[rootIndex + 1]) : path.resolve(__dirname, '../../desktop-dist');
const { MediaProxy } = require(path.join(compiledRoot, 'main/media-proxy.js'));
const mediaFetchPath = path.join(compiledRoot, 'main/media-fetch.js');
const createMediaFetch = require('node:fs').existsSync(mediaFetchPath) ? require(mediaFetchPath).createMediaFetch : null;
const { MpvController } = require(path.join(compiledRoot, 'main/mpv-controller.js'));
const { MpvHost } = require(path.join(compiledRoot, 'native/mpv-host/mpv-host.js'));
const { createNativeBinding } = require(path.join(compiledRoot, 'main/native-binding-factory.js'));
const { resolveNativeResourcePaths } = require(path.join(compiledRoot, 'main/platform-paths.js'));
const { createPlaybackFixtureOrigin, generateFixtureMedia } = require('./playback-fixtures.cjs');
const argument = (name, fallback) => { const index = process.argv.indexOf(name); return index >= 0 ? process.argv[index + 1] : fallback; };
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const percentile = (values, p) => values.length ? [...values].sort((a, b) => a - b)[Math.min(values.length - 1, Math.ceil(values.length * p) - 1)] : null;

app.whenReady().then(async () => {
  let win;
  let media;
  let code = 0;
  try {
    const repetitions = Number(argument('--repetitions', 10));
    const sustainedSeconds = Number(argument('--sustain', 600));
    const duration = Number(argument('--duration', Math.max(60, sustainedSeconds + 30)));
    const suppliedMedia = argument('--media', null);
    media = suppliedMedia ? { file: path.resolve(suppliedMedia), close: async () => undefined } : await generateFixtureMedia({ codec: argument('--codec', 'h264'), bitrateMbps: Number(argument('--bitrate', 8)), duration });
    const resourceArgument = argument('--resources', null);
    const resources = resourceArgument ? path.resolve(resourceArgument) : null;
    const native = resolveNativeResourcePaths({ packaged: Boolean(resources), platform: process.platform, architecture: process.arch, resourcesPath: resources || process.resourcesPath, projectRoot: path.resolve(__dirname, '../..') });
    win = new BrowserWindow({ width: 960, height: 540, transparent: true, backgroundColor: '#00000000', webPreferences: { nodeIntegration: false, contextIsolation: true } });
    await win.loadURL('data:text/html,<body style="background:transparent;margin:0"></body>');
    for (const ratio of argument('--ratios', '2,1.3,1,.6').split(',').map(Number)) {
      for (const condition of argument('--conditions', 'stable,jitter,outage,single-connection,throttle').split(',')) {
        const fileSize = (await require('node:fs/promises').stat(media.file)).size;
        const sourceMbps = fileSize * 8 / duration / 1000000;
        const fixture = await createPlaybackFixtureOrigin({ file: media.file, aggregateMbps: sourceMbps * ratio, latencyMs: condition === 'stable' ? 100 : 250, jitter: condition === 'jitter', outages: condition === 'outage' ? [[30000, 40000]] : [], maxConnections: condition === 'single-connection' ? 1 : undefined, throttleOnce: condition === 'throttle', retryAfterSeconds: 2 });
        const source = `${fixture.origin.replace('127.0.0.1', 'localhost')}/resolver`;
        const proxy = new MediaProxy(createMediaFetch ? createMediaFetch() : require("electron").net.fetch.bind(require("electron").net), Date.now, undefined, process.argv.includes('--trace') ? (event) => console.log(JSON.stringify({ proxyEvent: event, at: Date.now() })) : undefined);
        let controller;
        try {
          await proxy.start();
          const createController = () => {
            const binding = createNativeBinding({ platform: process.platform, architecture: process.arch, nativeWindowHandle: win.getNativeWindowHandle(), runtimeDirectory: native.runtimeDirectory, addon: require(native.addon) });
            if (!binding) throw new Error('Native fixture target unsupported');
            const host = new MpvHost(binding, fixture.origin);
            host.setPlaybackProperty('volume', 0);
            controller = new MpvController(host, () => undefined, 50, Date.now, () => undefined, undefined, undefined, proxy);
            controller.setVideoSurface({ visible: true, x: 0, y: 0, width: 960, height: 540, scaleFactor: 1 });
          };
          const startup = [];
          const seek = [];
          const playableStartup = [];
          const startupPhases = [];
          const seekPhases = [];
          const phaseTimings = (sample, prefix, from) => {
            const mark = (event) => sample.timeline.findLast((entry) => entry.event === event)?.elapsedMs;
            const origin = mark(from);
            const elapsed = (event) => origin === undefined || mark(event) === undefined ? null : mark(event) - origin;
            return { restartMs: elapsed(prefix === 'seek' ? 'seek-restart' : 'playback-restart'), cacheExitMs: elapsed(prefix === 'seek' ? 'seek-cache-exit' : 'cache-exit'), movementMs: elapsed(prefix === 'seek' ? 'seek-moving' : 'playback-moving') };
          };
          const waitFor = async (predicate, timeout = 120000) => {
            const deadline = Date.now() + timeout;
            while (Date.now() < deadline) { const sample = controller.getPlaybackDiagnostics(); if (predicate(sample)) return sample; await wait(25); }
            throw new Error(`Playback fixture timed out: ${JSON.stringify(controller.getPlaybackDiagnostics())}`);
          };
          for (let repetition = 0; repetition < repetitions; repetition++) {
            createController();
            const requestedAt = Date.now();
            controller.recordRendererTiming('play-requested');
            controller.handleSend('mpv-command', ['loadfile', source, 'replace', '-1', 'start=+0']);
            const opened = await waitFor((sample) => sample.startupTiming.totalMs !== null);
            startup.push(opened.startupTiming.totalMs);
            const moving = await waitFor((sample) => sample.moving === true && sample.timeline.some((entry) => entry.event === 'playback-moving') || sample.moving === undefined && !sample.buffering && sample.renderedFrames > opened.renderedFrames);
            startupPhases.push(phaseTimings(moving, 'startup', 'play-requested'));
            playableStartup.push(Date.now() - requestedAt);
            if (!proxy.stats().sizeBytes) throw new Error('Benchmark bypassed the media proxy');
            controller.recordRendererTiming('seek-requested');
            controller.handleSend('mpv-set-prop', ['time-pos', 20]);
            const sought = await waitFor((sample) => sample.lastSeekToFirstFrameMs !== null);
            seek.push(sought.lastSeekToFirstFrameMs);
            const seekMoving = await waitFor((sample) => sample.timeline.some((entry) => entry.event === 'seek-moving') || sample.moving === undefined && !sample.buffering);
            seekPhases.push(phaseTimings(seekMoving, 'seek', 'seek-requested'));
            controller.handleSend('mpv-command', ['stop']);
            await waitFor((sample) => sample.videoWidth === null && sample.videoHeight === null);
            controller.destroy();
            controller = null;
            console.log(JSON.stringify({ progress: { implementation: argument('--label', rootIndex >= 0 ? 'baseline' : 'current'), repetition: repetition + 1, repetitions, firstFrameMs: opened.startupTiming.totalMs, playableMs: playableStartup.at(-1), seekMs: sought.lastSeekToFirstFrameMs, startupPhases: startupPhases.at(-1), seekPhases: seekPhases.at(-1) } }));
          }
          fixture.resetProfileClock();
          createController();
          controller.recordRendererTiming('play-requested');
          controller.handleSend('mpv-command', ['loadfile', source, 'replace', '-1', 'start=+0']);
          const initial = await waitFor((sample) => sample.startupTiming.totalMs !== null && (sample.moving === true && sample.timeline.some((entry) => entry.event === 'playback-moving') || sample.moving === undefined && !sample.buffering));
          const samples = [];
          const deadline = Date.now() + sustainedSeconds * 1000;
          while (Date.now() < deadline) {
            const diagnostics = controller.getPlaybackDiagnostics();
            samples.push({ at: Date.now(), rss: process.memoryUsage().rss, cpu: process.cpuUsage(), cache: proxy.stats(), bufferedSeconds: diagnostics.cacheSeconds, buffering: diagnostics.buffering, frames: diagnostics.renderedFrames, drops: diagnostics.frameDropCount, rebufferMs: diagnostics.rebufferMilliseconds });
            await wait(1000);
          }
          const result = controller.getPlaybackDiagnostics();
          console.log(JSON.stringify({ implementation: argument('--label', rootIndex >= 0 ? 'baseline' : 'current'), startupMs: startup, startupPhases, seekPhases, playableStartupMs: playableStartup, seekMs: seek, ratio, condition, policy: rootIndex >= 0 ? 'baseline' : process.env.PANORAMA_ADAPTIVE_BUFFERING === '1' ? 'experimental' : 'default', repetitions, sustainedSeconds, sourceMbps, firstFrameMedianMs: percentile(startup, .5), firstFrameP95Ms: percentile(startup, .95), playableMedianMs: percentile(playableStartup, .5), playableP95Ms: percentile(playableStartup, .95), seekMedianMs: percentile(seek, .5), seekP95Ms: percentile(seek, .95), rebufferSeconds: (result.rebufferMilliseconds - initial.rebufferMilliseconds) / 1000, rebufferCount: result.rebufferCount - initial.rebufferCount, transferredBytes: fixture.counters.transferredBytes, resolverHits: fixture.counters.resolverHits, peakRss: Math.max(...samples.map((sample) => sample.rss)), peakDisk: Math.max(...samples.map((sample) => sample.cache.diskBytes || 0)), result, samples }));
        } finally { controller?.destroy(); proxy.destroy(); await fixture.close(); }
      }
    }
  } catch (error) { console.error(error); code = 1; }
  finally { await media?.close(); win?.destroy(); app.exit(code); }
});
