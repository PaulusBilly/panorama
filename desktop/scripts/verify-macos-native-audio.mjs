import assert from 'node:assert/strict';
import path from 'node:path';
import { createRequire } from 'node:module';
import { app, BrowserWindow } from 'electron';
const require = createRequire(import.meta.url);
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

app.whenReady().then(async () => {
  let host;
  let win;
  let exitCode = 0;
  try {
    const { NativeMpvHost } = require(path.resolve(process.argv[2]));
    const fixtures = process.argv.slice(3);
    assert(fixtures.length > 0, 'Audio fixtures are required');
    win = new BrowserWindow({ show: false, transparent: true });
    host = new NativeMpvHost(win.getNativeWindowHandle());
    host.setProperty('volume', 0);
    host.setProperty('audio-spdif', '');
    host.setProperty('audio-channels', 'auto-safe');
    const devices = ['auto'];
    if (host.getDiagnostics().audioDevices.some((device) => device.name === 'coreaudio/BuiltInSpeakerDevice')) {
      devices.push('coreaudio/BuiltInSpeakerDevice');
    }
    async function opened() {
      const deadline = Date.now() + 3_000;
      let diagnostics;
      do {
        await wait(100);
        diagnostics = host.getDiagnostics();
        assert.equal(diagnostics.audioOutputErrorSequence, 0, 'CoreAudio initialization failed');
        if (/^\d+$/.test(diagnostics.selectedAudioId ?? '') && diagnostics.audioOutputDriver === 'coreaudio' && diagnostics.audioOutputFormat) return diagnostics;
      } while (Date.now() < deadline);
      throw new Error('No active CoreAudio output');
    }
    for (const device of devices) {
      host.stop();
      host.setProperty('audio-device', device);
      for (const fixture of fixtures) {
        host.setProperty('aid', 'no');
        host.load(path.resolve(fixture), 0);
        host.setProperty('aid', 'auto');
        await wait(500);
        const first = await opened();
        host.setProperty('aid', 'no');
        await wait(100);
        host.setProperty('aid', first.selectedAudioId);
        host.seek(0);
        await wait(250);
        const diagnostics = await opened();
        console.log(JSON.stringify({ fixture: path.basename(fixture), output: device === 'auto' ? 'system default' : 'built-in speakers',
          passed: true, aid: diagnostics.selectedAudioId, driver: diagnostics.audioOutputDriver,
          format: diagnostics.audioOutputFormat, channels: diagnostics.audioOutputChannels, errors: diagnostics.audioOutputErrorSequence }));
      }
    }
  } catch (error) {
    console.error(error);
    exitCode = 1;
  } finally {
    host?.destroy();
    win?.destroy();
    app.exit(exitCode);
  }
});
