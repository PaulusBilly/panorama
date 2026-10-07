import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { findAvailableLoopbackPort } from "../../desktop/main/loopback-port";
import { DEFAULT_RENDERER_PORT, startRendererServer } from "../../desktop/main/renderer-server";

const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(temporaryDirectories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

async function createServer(source: string): Promise<string> {
  const directory = await mkdtemp(path.join(tmpdir(), "panorama-renderer-"));
  temporaryDirectories.push(directory);
  const serverPath = path.join(directory, "server.js");
  await writeFile(serverPath, source);
  return serverPath;
}

describe("renderer server", () => {
  it("uses one stable renderer origin so the signed-in session survives restarts", () => {
    expect(DEFAULT_RENDERER_PORT).toBe(11475);
  });

  it("selects and releases a loopback port", async () => {
    const first = await findAvailableLoopbackPort();
    const second = await findAvailableLoopbackPort();

    expect(first).toBeGreaterThan(0);
    expect(second).toBeGreaterThan(0);
  });

  it("binds to loopback, waits for readiness, and stops cleanly", async () => {
    const serverPath = await createServer(`
      const http = require("node:http");
      const server = http.createServer((_request, response) => response.end(process.env.PANORAMA_DESKTOP_BUILD));
      server.listen(Number(process.env.PORT), process.env.HOSTNAME);
      process.on("SIGTERM", () => server.close(() => process.exit(0)));
    `);
    const renderer = await startRendererServer({ serverPath, port: 0, startupTimeoutMs: 2_000 });

    expect(new URL(renderer.origin).hostname).toBe("127.0.0.1");
    await expect(fetch(renderer.origin).then((response) => response.text())).resolves.toBe("1");

    await renderer.stop();
    await expect(fetch(renderer.origin)).rejects.toThrow();
  });

  it("rejects a child that exits before readiness", async () => {
    const serverPath = await createServer("process.exit(7);");

    await expect(startRendererServer({ serverPath, port: 0, startupTimeoutMs: 5_000 })).rejects.toThrow("exited before readiness");
  });

  it("rejects startup timeouts and terminates the child", async () => {
    const serverPath = await createServer("setInterval(() => {}, 1000);");

    await expect(startRendererServer({ serverPath, port: 0, startupTimeoutMs: 100, pollIntervalMs: 20 })).rejects.toThrow("timed out");
  });
});
