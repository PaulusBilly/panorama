import { mkdtemp, rm, stat, writeFile } from "node:fs/promises";
import { createServer as createHttpServer, type Server } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { findAvailableLoopbackPort } from "../../desktop/main/loopback-port";
import { isStremioServerOnline, startBundledStremioServer, type BundledStremioServer } from "../../desktop/main/stremio-server";

const temporaryDirectories: string[] = [];
const started: BundledStremioServer[] = [];
const httpServers: Server[] = [];

afterEach(async () => {
  await Promise.all(started.splice(0).map((server) => server.stop()));
  await Promise.all(httpServers.splice(0).map((server) => new Promise((resolve) => server.close(resolve))));
  await Promise.all(temporaryDirectories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

// The fixture stands in for server.js: it answers the health path on a fixed
// port and records each launch so restarts can be counted.
async function createFixture(body: string): Promise<{ serverPath: string; origin: string; launches(): Promise<number> }> {
  const directory = await mkdtemp(path.join(tmpdir(), "panorama-stremio-"));
  temporaryDirectories.push(directory);
  const port = await findAvailableLoopbackPort();
  const launchLog = path.join(directory, "launches");
  const serverPath = path.join(directory, "server.js");
  await writeFile(serverPath, `
    const fs = require("node:fs");
    const http = require("node:http");
    fs.appendFileSync(${JSON.stringify(launchLog)}, "x");
    const launches = fs.readFileSync(${JSON.stringify(launchLog)}, "utf8").length;
    const listen = () => {
      const server = http.createServer((_request, response) => response.end(JSON.stringify([process.env.NO_HTTPS_SERVER, process.env.APP_PATH])));
      server.listen(${port}, "127.0.0.1");
      process.on("SIGTERM", () => server.close(() => process.exit(0)));
    };
    ${body}
  `);
  return {
    serverPath,
    origin: `http://127.0.0.1:${port}`,
    launches: async () => (await import("node:fs/promises")).readFile(launchLog, "utf8").then((text) => text.length, () => 0),
  };
}

async function waitFor(condition: () => Promise<boolean>, timeoutMs = 15_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await condition()) return;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  throw new Error("Condition was not met in time");
}

// Each case spawns real child processes, which start slowly on a loaded machine.
describe("bundled Stremio server", { timeout: 20_000 }, () => {
  it("uses a service that is already running instead of starting a second one", async () => {
    const fixture = await createFixture("listen();");
    const existing = createHttpServer((_request, response) => response.end("{}"));
    httpServers.push(existing);
    const existingPort = await findAvailableLoopbackPort();
    await new Promise<void>((resolve) => existing.listen(existingPort, "127.0.0.1", resolve));

    const server = await startBundledStremioServer({
      serverPath: fixture.serverPath,
      origins: [fixture.origin, `http://127.0.0.1:${existingPort}`],
      dataDirectory: path.dirname(fixture.serverPath),
    });

    expect(server).toBeNull();
    expect(await fixture.launches()).toBe(0);
  });

  it("starts the server, waits until it answers, and stops it cleanly", async () => {
    const fixture = await createFixture("listen();");

    const dataDirectory = path.join(path.dirname(fixture.serverPath), "data");
    const server = await startBundledStremioServer({ serverPath: fixture.serverPath, origins: [fixture.origin], dataDirectory });
    if (server) started.push(server);

    expect(server).not.toBeNull();
    expect(await isStremioServerOnline(fixture.origin)).toBe(true);
    // Settings and cache stay out of an installed Stremio's own directory.
    await expect(fetch(fixture.origin).then((response) => response.json())).resolves.toEqual(["1", dataDirectory]);
    await expect(stat(dataDirectory).then((entry) => entry.isDirectory())).resolves.toBe(true);

    await server?.stop();
    expect(await isStremioServerOnline(fixture.origin)).toBe(false);
    expect(await fixture.launches()).toBe(1);
  });

  it("resolves within the startup cap when the server never answers", async () => {
    const fixture = await createFixture("setInterval(() => {}, 1000);");
    const startedAt = Date.now();

    const server = await startBundledStremioServer({
      serverPath: fixture.serverPath,
      origins: [fixture.origin], dataDirectory: path.dirname(fixture.serverPath),
      startupTimeoutMs: 300,
    });
    if (server) started.push(server);

    expect(server).not.toBeNull();
    expect(Date.now() - startedAt).toBeLessThan(3_000);
  });

  it("restarts a server that exits unexpectedly", async () => {
    const fixture = await createFixture("if (launches === 1) setTimeout(() => process.exit(3), 300); listen();");

    const server = await startBundledStremioServer({ serverPath: fixture.serverPath, origins: [fixture.origin], dataDirectory: path.dirname(fixture.serverPath) });
    if (server) started.push(server);

    await waitFor(async () => await fixture.launches() === 2 && await isStremioServerOnline(fixture.origin));
  });

  it("stops restarting a server that keeps exiting", async () => {
    const fixture = await createFixture("process.exit(3);");

    const server = await startBundledStremioServer({
      serverPath: fixture.serverPath,
      origins: [fixture.origin], dataDirectory: path.dirname(fixture.serverPath),
      startupTimeoutMs: 1_500,
      maxRestarts: 2,
    });
    if (server) started.push(server);

    await waitFor(async () => await fixture.launches() === 3);
    await new Promise((resolve) => setTimeout(resolve, 300));
    expect(await fixture.launches()).toBe(3);
  });

  it("does not restart after stop", async () => {
    const fixture = await createFixture("listen();");
    const server = await startBundledStremioServer({ serverPath: fixture.serverPath, origins: [fixture.origin], dataDirectory: path.dirname(fixture.serverPath) });

    await server?.stop();
    await new Promise((resolve) => setTimeout(resolve, 300));

    expect(await fixture.launches()).toBe(1);
    expect(await isStremioServerOnline(fixture.origin)).toBe(false);
  });
});
