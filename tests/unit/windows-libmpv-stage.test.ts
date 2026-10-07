import { describe, expect, it } from "vitest";
import {
  assertPeX64,
  assertSafeArchiveMembers,
  validateWindowsLibmpvManifest,
  verifySha256,
} from "../../desktop/scripts/stage-windows-libmpv.mjs";

const manifest = {
  schemaVersion: 1,
  architecture: "x64",
  source: { url: "https://example.test/releases/20260814/mpv.7z", sha256: "a".repeat(64) },
  files: {
    headers: ["include/mpv/client.h", "include/mpv/render.h"],
    runtime: ["libmpv-2.dll"],
    rejectedLinkInputs: ["libmpv.dll.a"],
  },
  noticesDirectory: "desktop/native/mpv-host/licenses/windows-libmpv",
};

describe("Windows libmpv staging", () => {
  it("accepts the immutable x64 manifest", () => {
    expect(validateWindowsLibmpvManifest(manifest)).toEqual(manifest);
  });

  it.each([
    { ...manifest, architecture: "arm64" },
    { ...manifest, source: { ...manifest.source, url: "http://example.test/mpv.7z" } },
    { ...manifest, source: { ...manifest.source, url: "https://example.test/latest/mpv.7z" } },
    { ...manifest, source: { ...manifest.source, sha256: "bad" } },
  ])("rejects unsafe manifest variants", (candidate) => {
    expect(() => validateWindowsLibmpvManifest(candidate)).toThrow();
  });

  it("rejects path traversal archive members", () => {
    expect(() => assertSafeArchiveMembers(["include/mpv/client.h", "../escape.dll"])).toThrow();
    expect(() => assertSafeArchiveMembers(["C:/escape.dll"])).toThrow();
    expect(assertSafeArchiveMembers(["include/mpv/client.h", "libmpv-2.dll"])).toBeUndefined();
  });

  it("compares SHA-256 values without accepting malformed digests", () => {
    expect(() => verifySha256("b".repeat(64), "bad")).toThrow();
    expect(() => verifySha256("b".repeat(64), "c".repeat(64))).toThrow();
    expect(verifySha256("B".repeat(64), "b".repeat(64))).toBeUndefined();
  });

  it("accepts only an x64 PE runtime", () => {
    const x64 = Buffer.alloc(0x100);
    x64.write("MZ", 0, "ascii");
    x64.writeUInt32LE(0x80, 0x3c);
    x64.write("PE\0\0", 0x80, "binary");
    x64.writeUInt16LE(0x8664, 0x84);
    expect(assertPeX64(x64)).toBeUndefined();

    const arm64 = Buffer.from(x64);
    arm64.writeUInt16LE(0xaa64, 0x84);
    expect(() => assertPeX64(arm64)).toThrow("x64");
    expect(() => assertPeX64(Buffer.from("not a PE"))).toThrow("PE");
  });

  it("rejects unsafe manifest inventory and notice paths", () => {
    expect(() => validateWindowsLibmpvManifest({
      ...manifest,
      files: { ...manifest.files, runtime: ["../libmpv-2.dll"] },
    })).toThrow("path");
    expect(() => validateWindowsLibmpvManifest({
      ...manifest,
      noticesDirectory: "C:\\developer\\notices",
    })).toThrow("path");
  });
});
