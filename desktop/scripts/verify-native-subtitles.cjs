const assert = require('node:assert/strict');
const { app, BrowserWindow } = require('electron');
const path = require('node:path');
const { resolveNativeResourcePaths } = require('../../desktop-dist/main/platform-paths.js');
const { createPlaybackFixtureOrigin, generateFixtureMedia } = require('./playback-fixtures.cjs');
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

app.whenReady().then(async () => {
  let host, win, fixture, media;
  let code = 0;
  const samples = [];
  try {
    media = await generateFixtureMedia({ duration: 30 });
    fixture = await createPlaybackFixtureOrigin({ file: media.file });
    const resources = process.argv[2] ? path.resolve(process.argv[2]) : undefined;
    const native = resolveNativeResourcePaths({ packaged: Boolean(resources), platform: process.platform, architecture: process.arch, resourcesPath: resources || process.resourcesPath, projectRoot: path.resolve(__dirname, '../..') });
    const { NativeMpvHost } = require(native.addon);
    win = new BrowserWindow({ width: 960, height: 540, transparent: true, backgroundColor: '#00000000' });
    await win.loadURL('data:text/html,<body style="background:transparent;margin:0"></body>');
    host = process.platform === 'win32' ? new NativeMpvHost(win.getNativeWindowHandle(), native.runtimeDirectory) : new NativeMpvHost(win.getNativeWindowHandle());
    host.setBounds(0, 0, 960, 540, 1);
    host.setProperty('volume', 0);
    const visible = [];
    host.onSubtitleCue((cue) => {
      const time = host.getDiagnostics().timeSeconds;
      if (cue.kind === 'text' && cue.text) visible.push(cue);
      samples.push({ playbackGeneration: cue.playbackGeneration, selectionGeneration: cue.selectionGeneration, seekGeneration: cue.seekGeneration, sequence: cue.sequence, kind: cue.kind, startSeconds: cue.startSeconds, endSeconds: cue.endSeconds, timeSeconds: time, at: Date.now() });
    });
    host.load(`${fixture.origin}/media`, 0);
    const loadDeadline = Date.now() + 10000;
    while (Date.now() < loadDeadline && !host.getDiagnostics().videoCodec) await wait(20);
    assert(host.getDiagnostics().videoCodec, 'Fixture media did not load');
    host.command(['sub-add', path.resolve('tests/fixtures/subtitles/dialogue.ass'), 'cached', 'Panorama timing fixture', 'en']);
    const deadline = Date.now() + 5000;
    let track;
    while (Date.now() < deadline) { track = host.getDiagnostics().tracks.find((entry) => entry.title === 'Panorama timing fixture'); if (track) break; await wait(20); }
    assert(track, 'Fixture subtitle track missing');
    host.setProperty('sid', String(track.id));
    host.setProperty('sub-visibility', false);
    host.seek(0);
    await wait(6000);
    assert(visible.some((cue) => cue.text.includes('♪')), 'Music-note cue missing');
    assert(visible.some((cue) => cue.text.includes('♫') && cue.text.includes('\n')), 'Multiline music cue missing');
    assert(samples.some((sample) => sample.endSeconds - sample.startSeconds <= .11 && sample.kind === 'text'), '100 ms cue missing');
    const beforeReselect = samples.at(-1).selectionGeneration;
    host.setProperty('sid', String(track.id));
    await wait(100);
    assert(samples.some((sample) => sample.selectionGeneration > beforeReselect && sample.kind === 'text'), 'Cached reselection did not refill current-generation cues');
    const selection = samples.at(-1).selectionGeneration;
    const offStart = samples.length;
    host.setProperty('sid', 'no');
    await wait(200);
    assert(samples.some((sample) => sample.selectionGeneration > selection && sample.kind === 'none'), 'Off did not clear cues');
    assert(samples.slice(offStart).every((sample) => sample.kind === 'none'), 'A queued old cue survived Off');
    host.command(['sub-add', path.resolve('tests/fixtures/subtitles/mixed-signs.ass'), 'cached', 'Panorama authored fixture', 'en']);
    await wait(500);
    const authored = host.getDiagnostics().tracks.find((entry) => entry.title === 'Panorama authored fixture');
    assert(authored, 'Authored fixture track missing');
    host.setProperty('sid', String(authored.id));
    host.seek(2);
    await wait(1000);
    assert(samples.some((sample) => sample.kind === 'authored'), 'Authored drawing fallback missing');
    console.log(JSON.stringify({ passed: true, platform: process.platform, samples }));
  } catch (error) { console.error(error); code = 1; }
  finally { host?.onSubtitleCue(null); host?.destroy(); win?.destroy(); await fixture?.close(); await media?.close(); app.exit(code); }
});
