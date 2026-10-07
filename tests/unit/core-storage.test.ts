import { describe, expect, it } from "vitest";
import { createCoreStorage, installCoreStorage } from "@/runtime/core-storage";

function memoryBackend() {
  const values = new Map<string, string>();
  return {
    keys: values,
    getItem(key: string) {
      return values.has(key) ? values.get(key)! : null;
    },
    setItem(key: string, value: string) {
      values.set(key, value);
    },
    removeItem(key: string) {
      values.delete(key);
    },
  };
}

describe("core storage", () => {
  it("round-trips core profile JSON without writing a password key", () => {
    const backend = memoryBackend();
    const storage = createCoreStorage(backend);
    const profile = JSON.stringify({
      auth: { key: "session-key", user: { email: "viewer@example.com" } },
    });

    storage.setItem("profile", profile);

    expect([...backend.keys.keys()]).toEqual(["profile"]);
    expect(storage.getItem("profile")).toBe(profile);
    expect(JSON.parse(storage.getItem("profile") ?? "{}")).not.toHaveProperty("password");
  });

  it("survives missing or throwing storage", () => {
    expect(createCoreStorage(null).getItem("profile")).toBeNull();
    expect(() => createCoreStorage(null).setItem("profile", "{}")).not.toThrow();
    expect(() => createCoreStorage(null).removeItem("profile")).not.toThrow();

    const throwing = {
      getItem() {
        throw new Error("denied");
      },
      setItem() {
        throw new Error("denied");
      },
      removeItem() {
        throw new Error("denied");
      },
    };
    const storage = createCoreStorage(throwing);
    expect(storage.getItem("profile")).toBeNull();
    expect(() => storage.setItem("profile", "{}")).not.toThrow();
    expect(() => storage.removeItem("profile")).not.toThrow();
  });

  it("installs getItem/setItem/removeItem on the provided localStorage", () => {
    const backend = memoryBackend();
    const scope = { localStorage: backend as unknown as Storage };
    installCoreStorage(scope);
    scope.localStorage.setItem("profile", JSON.stringify({ auth: { key: "session-key" } }));
    expect(scope.localStorage.getItem("profile")).toContain("session-key");
    scope.localStorage.removeItem("profile");
    expect(scope.localStorage.getItem("profile")).toBeNull();
  });
});
