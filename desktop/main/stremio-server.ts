import { mkdir } from "node:fs/promises";
import { spawnNodeChild, waitForExit, type NodeChild } from "./renderer-server";

export type BundledStremioServer = {
  stop(): Promise<void>;
};

type BundledStremioServerOptions = {
  /** The launcher script staged beside server.js. */
  serverPath: string;
  /** Loopback origins the renderer discovers; the server claims the first free port among them. */
  origins: readonly string[];
  /** Where the server keeps its settings and cache, apart from an installed Stremio's. */
  dataDirectory: string;
  startupTimeoutMs?: number;
  maxRestarts?: number;
};

const HEALTH_PATH = "/stats.json";
// Panorama plays through MPV and never casts, so the HTTPS endpoint on 12470
// and device discovery stay off. The launcher exits once this process is gone.
const SERVER_ENV = { NO_HTTPS_SERVER: "1", CASTING_DISABLED: "1", PANORAMA_PARENT_PID: String(process.pid) };

export async function isStremioServerOnline(origin: string): Promise<boolean> {
  try {
    return (await fetch(`${origin}${HEALTH_PATH}`, { signal: AbortSignal.timeout(500) })).ok;
  } catch {
    return false;
  }
}

/**
 * Starts the bundled streaming server unless a Stremio Service or Stremio app
 * already answers on one of the origins, in which case that one is used and
 * null is returned. A server that fails to start is not fatal: the renderer
 * reports the service as offline exactly as it does without a bundled server.
 */
export async function startBundledStremioServer(options: BundledStremioServerOptions): Promise<BundledStremioServer | null> {
  const { serverPath, origins, dataDirectory } = options;
  const maxRestarts = options.maxRestarts ?? 3;
  const anyOnline = async () => (await Promise.all(origins.map(isStremioServerOnline))).some(Boolean);
  if (await anyOnline()) return null;

  await mkdir(dataDirectory, { recursive: true });
  let child: NodeChild;
  let stopping = false;
  let restarts = 0;
  const exited = () => child.exitCode !== null || child.signalCode !== null;
  const launch = () => {
    const current = spawnNodeChild(serverPath, { ...process.env, ...SERVER_ENV, APP_PATH: dataDirectory }, "Panorama Streaming Server");
    child = current;
    current.once("exit", () => {
      if (stopping || restarts >= maxRestarts) return;
      restarts += 1;
      // A service started by the user in the meantime takes over instead.
      void anyOnline().then((online) => {
        if (!online && !stopping) launch();
      });
    });
  };
  launch();

  const deadline = Date.now() + (options.startupTimeoutMs ?? 10_000);
  while (Date.now() < deadline && !(await anyOnline())) {
    if (exited() && restarts >= maxRestarts) break;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  return {
    async stop() {
      stopping = true;
      if (exited()) return;
      child.kill("SIGTERM");
      await waitForExit(child, 5_000);
    },
  };
}
