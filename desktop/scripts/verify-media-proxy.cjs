const assert = require('node:assert/strict');
const { app } = require('electron');
const { createHash } = require('node:crypto');
const { MediaProxy } = require('../../desktop-dist/main/media-proxy.js');
const { createMediaFetch } = require('../../desktop-dist/main/media-fetch.js');
const { createPlaybackFixtureOrigin, fixtureBytes } = require('./playback-fixtures.cjs');

app.whenReady().then(async () => {
  let exitCode = 0;
  const profiles = [
    { name: 'redirect-reuse' },
    { name: 'probe-stall', stallOnce: true, stallTimeoutMs: 100 },
    { name: 'wrong-range', wrongRange: true, fails: true },
    { name: 'retry-after', throttleOnce: true, retryAfterSeconds: 1 },
    { name: 'passthrough', noRanges: true },
  ];
  try {
    const failures = [];
    for (const mode of ['abort-after-headers', 'cancel-response-body']) {
      const fixture = await createPlaybackFixtureOrigin({ stallOnce: true });
      const controller = new AbortController();
      let result;
      try {
        result = await createMediaFetch()(`${fixture.origin}/resolver`, { signal: controller.signal });
        assert.equal(result.finalUrl, `${fixture.origin}/media`);
        assert.equal(fixture.counters.active, 1);
        if (mode === 'abort-after-headers') controller.abort();
        else await result.response.body.cancel();
        const deadline = Date.now() + 1000;
        while (fixture.counters.active > 0 && Date.now() < deadline) await new Promise((resolve) => setTimeout(resolve, 10));
        assert.equal(fixture.counters.active, 0, `${mode} must close the upstream transfer`);
        console.log(JSON.stringify({ profile: mode, passed: true, counters: fixture.counters }));
      } catch (error) {
        failures.push(error);
        console.error(JSON.stringify({ profile: mode, passed: false, message: error.message }));
      } finally {
        controller.abort();
        await result?.response.body?.cancel().catch(() => undefined);
        await fixture.close();
      }
    }
    if (failures.length) throw new AggregateError(failures, 'Media transport cancellation failed');
    for (const profile of profiles) {
      const fixture = await createPlaybackFixtureOrigin(profile);
      const proxy = new MediaProxy(createMediaFetch(), Date.now, profile.stallTimeoutMs || 8000);
      try {
        await proxy.start();
        const url = proxy.open(`${fixture.origin}/resolver`);
        let failed = false;
        let bytes;
        try { bytes = Buffer.from(await (await fetch(url, { signal: AbortSignal.timeout(30000) })).arrayBuffer()); } catch { failed = true; }
        assert.equal(failed, Boolean(profile.fails));
        if (!failed) assert.equal(createHash('sha256').update(bytes).digest('hex'), createHash('sha256').update(fixtureBytes(0, fixture.size)).digest('hex'));
        if (profile.name === 'redirect-reuse') { assert.equal(fixture.counters.resolverHits, 1); assert.equal(fixture.counters.mediaRequests, 4); }
        if (profile.name === 'retry-after') {
          const match = /^bytes=(\d+)-(\d+)$/.exec(fixture.counters.throttledRange);
          const retry = fixture.counters.ranges.find((range) => range.start === Number(match[1]) && range.end === Number(match[2]));
          assert(retry && retry.atMs >= fixture.counters.throttledAtMs + 1000);
        }
        console.log(JSON.stringify({ profile: profile.name, passed: true, counters: fixture.counters, cache: proxy.stats() }));
      } finally { proxy.destroy(); await fixture.close(); }
    }
  } catch (error) { console.error(error); exitCode = 1; }
  app.exit(exitCode);
});
