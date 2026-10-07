import { execFileSync } from "node:child_process";
import { cp, readdir } from "node:fs/promises";
import path from "node:path";

export async function stageMacRuntime(target, projectRoot) {
  const prefix = path.join(projectRoot, ".cache/panorama/macos-libmpv/current");
  const libraries = (await readdir(path.join(prefix, "lib"))).filter((name) => name.endsWith(".dylib"));
  for (const name of libraries) await cp(path.join(prefix, "lib", name), path.join(target, name), { dereference: true });
  await cp(path.join(prefix, "licenses"), path.join(target, "licenses"), { recursive: true });
  for (const name of ["mpv_host.node", ...libraries]) {
    const file = path.join(target, name);
    const architectures = execFileSync("lipo", ["-archs", file], { encoding: "utf8" }).trim().split(/\s+/);
    if (!architectures.includes("arm64")) throw new Error(`Missing arm64 native library: ${name}`);
    if (architectures.length > 1) execFileSync("lipo", [file, "-thin", "arm64", "-output", file]);
    const dependencies = execFileSync("otool", ["-L", file], { encoding: "utf8" }).split("\n").slice(1)
      .map((line) => line.trim().split(" (compatibility")[0]).filter(Boolean);
    for (const dependency of dependencies) {
      if (dependency.startsWith("/System/Library/") || dependency.startsWith("/usr/lib/")) continue;
      const base = path.basename(dependency);
      if (!libraries.includes(base)) throw new Error(`Unbundled native dependency in ${name}`);
      execFileSync("install_name_tool", ["-change", dependency, `@loader_path/${base}`, file]);
    }
    if (name.endsWith(".dylib")) execFileSync("install_name_tool", ["-id", `@loader_path/${name}`, file]);
    const commands = execFileSync("otool", ["-l", file], { encoding: "utf8" });
    for (const match of commands.matchAll(/cmd LC_RPATH\s+cmdsize \d+\s+path (.+?) \(offset/g)) {
      if (path.isAbsolute(match[1])) execFileSync("install_name_tool", ["-delete_rpath", match[1], file]);
    }
    execFileSync("codesign", ["--force", "--sign", "-", file], { stdio: "pipe" });
  }
}
