const { createServer } = require('node:http');
const { mkdtemp, rm, open, stat } = require('node:fs/promises');
const { tmpdir } = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');

function fixtureBytes(start, length) {
  const bytes = Buffer.allocUnsafe(length);
  for (let i = 0; i < length; i++) bytes[i] = ((start + i) * 17 + Math.floor((start + i) / 251)) % 256;
  return bytes;
}

async function generateFixtureMedia({ codec = 'h264', bitrateMbps = 8, duration = 600, width = 1280, height = 720 } = {}) {
  const directory = await mkdtemp(path.join(tmpdir(), 'panorama-fixture-'));
  const file = path.join(directory, `${codec}.mkv`);
  try {
    const args = ['-v', 'error', '-f', 'lavfi', '-i', `testsrc2=size=${width}x${height}:rate=24`, '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000', '-t', String(duration), '-c:v', codec === 'hevc' ? 'libx265' : 'libx264', '-preset', 'ultrafast', '-b:v', `${bitrateMbps}M`, '-minrate', `${bitrateMbps}M`, '-maxrate', `${bitrateMbps}M`, '-bufsize', `${bitrateMbps * 2}M`, '-g', '24', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-y', file];
    await new Promise((resolve, reject) => {
      const child = spawn(process.env.PANORAMA_FIXTURE_FFMPEG || 'ffmpeg', args, { stdio: ['ignore', 'ignore', 'pipe'] });
      let stderr = '';
      child.stderr.on('data', (data) => { stderr = (stderr + data).slice(-4000); });
      child.on('error', reject);
      child.on('exit', (code) => code === 0 ? resolve() : reject(new Error(`Fixture encoding failed (${code}): ${stderr}`)));
    });
    return { file, close: () => rm(directory, { recursive: true, force: true }) };
  } catch (error) { await rm(directory, { recursive: true, force: true }); throw error; }
}

async function createPlaybackFixtureOrigin(profile = {}) {
  const file = profile.file ? await open(profile.file, 'r') : null;
  const size = profile.file ? (await stat(profile.file)).size : profile.sizeBytes || 8 * 1048576;
  const counters = { resolverHits: 0, mediaRequests: 0, active: 0, closed: 0, transferredBytes: 0, ranges: [] };
  const sockets = new Set();
  const transfers = [];
  let started = Date.now();
  let profileRequests = 0;
  let origin;
  let stopped = false;
  const server = createServer((request, response) => {
    if (request.url.startsWith('/resolver')) { counters.resolverHits++; response.writeHead(302, { Location: '/redirect' }); response.end(); return; }
    if (request.url === '/redirect') { response.writeHead(307, { Location: '/media' }); response.end(); return; }
    counters.mediaRequests++;
    const requestNumber = ++profileRequests;
    if (profile.maxConnections && counters.active >= profile.maxConnections || profile.throttleOnce && requestNumber === 2) {
      counters.throttledAtMs = Date.now() - started;
      counters.throttledRange = request.headers.range;
      response.writeHead(429, { 'Retry-After': String(profile.retryAfterSeconds || 1) }); response.end(); return;
    }
    const match = /^bytes=(\d+)-(\d*)$/.exec(request.headers.range || '');
    const start = match ? Number(match[1]) : 0;
    const end = match && match[2] ? Math.min(size - 1, Number(match[2])) : size - 1;
    if (start >= size) { response.writeHead(416, { 'Content-Range': `bytes */${size}` }); response.end(); return; }
    const actualStart = profile.wrongRange && start > 0 ? 0 : start;
    const actualEnd = actualStart + end - start;
    counters.ranges.push({ start, end, atMs: Date.now() - started });
    const headers = { 'Content-Type': profile.file ? 'video/x-matroska' : 'application/octet-stream', 'Content-Length': String(end - start + 1), ETag: profile.etag || '"fixture-v1"' };
    if (match && !profile.noRanges) headers['Content-Range'] = `bytes ${actualStart}-${actualEnd}/${size}`;
    response.writeHead(match && !profile.noRanges ? 206 : 200, headers);
    response.flushHeaders();
    if (request.method === 'HEAD') { response.end(); return; }
    const transfer = { response, position: actualStart, end: actualEnd, sent: 0, paused: false, readyAt: Date.now() + (profile.latencyMs || 0), stalls: profile.stallOnce && requestNumber === 1 };
    counters.active++;
    transfers.push(transfer);
    response.on('close', () => { const index = transfers.indexOf(transfer); if (index >= 0) transfers.splice(index, 1); counters.active--; counters.closed++; });
  });
  server.on('connection', (socket) => { sockets.add(socket); socket.on('close', () => sockets.delete(socket)); });
  let ticking = false;
  const clock = setInterval(async () => {
    if (ticking || stopped) return;
    ticking = true;
    try {
      const elapsed = Date.now() - started;
      const outage = (profile.outages || []).some(([from, to]) => elapsed >= from && elapsed < to);
      const jitter = profile.jitter ? [1, .2, 1.8, .5][Math.floor(elapsed / 1000) % 4] : 1;
      let budget = outage ? 0 : Math.floor((profile.aggregateMbps || 1000) * 125000 * .025 * jitter);
      const ready = transfers.filter((transfer) => !transfer.paused && Date.now() >= transfer.readyAt && !transfer.response.destroyed);
      let progressing = true;
      while (budget > 0 && progressing) {
        progressing = false;
        const allowance = Math.max(1, Math.floor(budget / Math.max(1, ready.length)));
        for (const transfer of ready) {
          if (budget <= 0) break;
          if (transfer.paused || transfer.response.destroyed || transfer.position > transfer.end) continue;
          if (transfer.stalls && transfer.sent >= (profile.stallAfterBytes || 32768)) continue;
          const length = Math.min(65536, allowance, budget, transfer.end - transfer.position + 1, transfer.stalls ? Math.max(0, (profile.stallAfterBytes || 32768) - transfer.sent) : Infinity);
          if (length <= 0) continue;
          const bytes = file ? Buffer.alloc(length) : fixtureBytes(transfer.position, length);
          const actual = file ? (await file.read(bytes, 0, length, transfer.position)).bytesRead : length;
          if (actual === 0) { transfer.response.destroy(); continue; }
          progressing = true;
          transfer.position += actual; transfer.sent += actual; budget -= actual; counters.transferredBytes += actual;
          if (!transfer.response.write(bytes.subarray(0, actual))) { transfer.paused = true; transfer.response.once('drain', () => { transfer.paused = false; }); }
          if (transfer.position > transfer.end) transfer.response.end();
        }
      }
    } finally { ticking = false; }
  }, 25);
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  origin = `http://127.0.0.1:${server.address().port}`;
  return { origin, size, counters, resetProfileClock() { started = Date.now(); profileRequests = 0; }, async close() { stopped = true; clearInterval(clock); for (const socket of sockets) socket.destroy(); await new Promise((resolve) => server.close(resolve)); while (ticking) await new Promise((resolve) => setTimeout(resolve, 10)); await file?.close(); } };
}

module.exports = { createPlaybackFixtureOrigin, generateFixtureMedia, fixtureBytes };
