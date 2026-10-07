import { spawn, type ChildProcess } from "node:child_process";
import { copyFile, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";

const temporaryDirectories: string[] = [];
const children: ChildProcess[] = [];

afterEach(async () => {
  for (const child of children.splice(0)) child.kill("SIGKILL");
  await Promise.all(temporaryDirectories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

// The launcher only narrows the service ports, so the fixture needs a free one.
async function freeServicePort(): Promise<number> {
  for (const port of [11474, 11473, 11472, 11471, 11470]) {
    const free = await new Promise<boolean>((resolve) => {
      const probe = createServer();
      probe.once("error", () => resolve(false));
      probe.listen(port, () => probe.close(() => resolve(true)));
    });
    if (free) return port;
  }
  throw new Error("No free Stremio service port for the fixture");
}

// Stands in for server.js: like the real one it listens without naming a host.
async function launch(serverSource: string, env: Record<string, string> = {}): Promise<{ child: ChildProcess; lines: string[] }> {
  const directory = await mkdtemp(path.join(tmpdir(), "panorama-stremio-launch-"));
  temporaryDirectories.push(directory);
  await copyFile(path.resolve("desktop/stremio-server/launch.cjs"), path.join(directory, "launch.cjs"));
  await writeFile(path.join(directory, "server.js"), serverSource);
  const child = spawn(process.execPath, [path.join(directory, "launch.cjs")], { env: { ...process.env, ...env }, stdio: ["ignore", "pipe", "inherit"] });
  children.push(child);
  const lines: string[] = [];
  child.stdout?.on("data", (chunk: Buffer) => lines.push(...chunk.toString().split("\n").filter(Boolean)));
  return { child, lines };
}

async function waitFor(condition: () => boolean, timeoutMs = 15_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!condition()) {
    if (Date.now() > deadline) throw new Error("Condition was not met in time");
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
}

describe("Stremio server launcher", { timeout: 20_000 }, () => {
  it("binds the service port to loopback and leaves other listeners alone", async () => {
    const port = await freeServicePort();
    const { lines } = await launch(`
      const http = require("node:http");
      const service = http.createServer(() => {});
      service.listen(${port}, () => console.log("service " + service.address().address));
      const other = http.createServer(() => {});
      other.listen(0, () => console.log("other " + other.address().address));
    `);

    await waitFor(() => lines.length === 2);

    expect(lines).toContain("service 127.0.0.1");
    expect(lines.find((line) => line.startsWith("other "))).not.toBe("other 127.0.0.1");
  });

  it("exits when the process that started it is gone", async () => {
    const parent = spawn(process.execPath, ["-e", "setTimeout(() => {}, 60000)"], { stdio: "ignore" });
    children.push(parent);
    const { child, lines } = await launch(`console.log("up"); setInterval(() => {}, 1000);`, {
      PANORAMA_PARENT_PID: String(parent.pid),
    });
    await waitFor(() => lines.includes("up"));
    expect(child.exitCode).toBeNull();

    parent.kill("SIGKILL");

    await waitFor(() => child.exitCode !== null);
  });
});
