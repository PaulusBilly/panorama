import { spawn } from "node:child_process";
import path from "node:path";
import { utilityProcess } from "electron";
import { findAvailableLoopbackPort } from "./loopback-port";

export type RendererServer = {
  origin: string;
  stop(): Promise<void>;
};

type RendererServerOptions = {
  serverPath?: string;
  port?: number;
  startupTimeoutMs?: number;
  pollIntervalMs?: number;
};

export const DEFAULT_RENDERER_PORT = 11475;

export type NodeChild = {
  readonly exitCode: number | null;
  readonly signalCode: NodeJS.Signals | null;
  kill(signal?: NodeJS.Signals): boolean;
  once(event: "exit", listener: () => void): unknown;
};

export function spawnNodeChild(serverPath: string, env: NodeJS.ProcessEnv, serviceName: string): NodeChild {
  if (!process.versions.electron || process.platform === "win32") {
    return spawn(process.execPath, [serverPath], {
      stdio: "ignore",
      windowsHide: true,
      env: process.versions.electron ? { ...env, ELECTRON_RUN_AS_NODE: "1" } : env,
    });
  }
  const child = utilityProcess.fork(serverPath, [], {
    stdio: "ignore",
    env,
    serviceName,
  });
  let exitCode: number | null = null;
  child.once("exit", (code) => { exitCode = code; });
  return {
    get exitCode() { return exitCode; },
    signalCode: null,
    kill: (signal?: NodeJS.Signals) => {
      if (signal === "SIGKILL" && child.pid) {
        try {
          process.kill(child.pid, signal);
          return true;
        } catch {
          return false;
        }
      }
      return child.kill();
    },
    once: (_event: "exit", listener: () => void) => child.once("exit", listener),
  };
}

export function waitForExit(child: NodeChild, timeoutMs: number): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve();
  return new Promise((resolve) => {
    const timeout = setTimeout(() => {
      child.kill("SIGKILL");
      resolve();
    }, timeoutMs);
    child.once("exit", () => {
      clearTimeout(timeout);
      resolve();
    });
  });
}

export async function startRendererServer(options: RendererServerOptions = {}): Promise<RendererServer> {
  const port = await findAvailableLoopbackPort(options.port ?? DEFAULT_RENDERER_PORT);
  const origin = `http://127.0.0.1:${port}`;
  const serverPath = options.serverPath ?? path.join(process.resourcesPath, "desktop-resources", "standalone", "server.js");
  const child = spawnNodeChild(serverPath, {
    ...process.env,
    HOSTNAME: "127.0.0.1",
    PORT: String(port),
    NODE_ENV: "production",
    PANORAMA_DESKTOP_BUILD: "1",
  }, "Panorama Renderer Server");
  const startupTimeoutMs = options.startupTimeoutMs ?? 30_000;
  const pollIntervalMs = options.pollIntervalMs ?? 100;
  const deadline = Date.now() + startupTimeoutMs;

  try {
    while (Date.now() < deadline) {
      if (child.exitCode !== null || child.signalCode !== null) {
        throw new Error(`Renderer server exited before readiness (${child.exitCode ?? child.signalCode})`);
      }
      try {
        await fetch(origin, { signal: AbortSignal.timeout(Math.min(500, pollIntervalMs * 2)) });
        return {
          origin,
          async stop() {
            if (child.exitCode !== null || child.signalCode !== null) return;
            child.kill("SIGTERM");
            await waitForExit(child, 5_000);
          },
        };
      } catch {
        await new Promise((resolve) => setTimeout(resolve, pollIntervalMs));
      }
    }
    throw new Error("Renderer server startup timed out");
  } catch (error) {
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGTERM");
    await waitForExit(child, 1_000);
    throw error;
  }
}
