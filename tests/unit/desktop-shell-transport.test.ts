import { describe, expect, it, vi } from "vitest";
import { createDesktopShellTransport } from "../../runtime/desktop-shell-transport";

describe("desktop ShellVideo transport", () => {
  it("forwards allowlisted messages and cleans up subscriptions", () => {
    const send = vi.fn();
    const unsubscribe = vi.fn();
    const event = { emit: null as ((payload: unknown) => void) | null };
    window.panoramaDesktop = {
      getCapabilities: vi.fn(),
      openExternal: vi.fn(),
      mpv: {
        send,
        on: vi.fn((_channel, listener) => {
          event.emit = listener;
          return unsubscribe;
        }),
        setVideoSurface: vi.fn(),
      },
    };
    const transport = createDesktopShellTransport();
    const listener = vi.fn();

    transport.send("mpv-command", ["stop"]);
    const remove = transport.on("mpv-prop-change", listener);
    event.emit?.({ name: "pause", data: true });
    remove();
    transport.destroy();

    expect(send).toHaveBeenCalledWith("mpv-command", ["stop"]);
    expect(listener).toHaveBeenCalledWith({ name: "pause", data: true });
    expect(unsubscribe).toHaveBeenCalledOnce();
    expect(() => transport.send("mpv-command", ["stop"])).toThrow("destroyed");
  });

  it("requires the preload MPV API", () => {
    delete window.panoramaDesktop;
    expect(() => createDesktopShellTransport()).toThrow("unavailable");
  });

  it("keeps MPV controls and input disabled for the embedded player", () => {
    const send = vi.fn();
    window.panoramaDesktop = {
      getCapabilities: vi.fn(),
      openExternal: vi.fn(),
      mpv: { send, on: vi.fn(() => vi.fn()), setVideoSurface: vi.fn() },
    };
    const transport = createDesktopShellTransport();

    transport.send("mpv-set-prop", ["osc", "yes"]);
    transport.send("mpv-set-prop", ["input-default-bindings", true]);
    transport.send("mpv-set-prop", ["input-vo-keyboard", "yes"]);

    expect(send.mock.calls).toEqual([
      ["mpv-set-prop", ["osc", "no"]],
      ["mpv-set-prop", ["input-default-bindings", "no"]],
      ["mpv-set-prop", ["input-vo-keyboard", "no"]],
    ]);
  });
});
