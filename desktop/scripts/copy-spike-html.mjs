import { cp, mkdir } from "node:fs/promises";
import path from "node:path";

const target = path.resolve("desktop-dist/spike");
await mkdir(target, { recursive: true });
await cp(path.resolve("desktop/spike/index.html"), path.join(target, "index.html"));
