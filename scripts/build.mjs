import { mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pkg = resolve(root, "packages/mini-canvas");
const wasmDir = resolve(pkg, "wasm");
const dist = resolve(pkg, "dist");
await mkdir(wasmDir, { recursive: true });
await mkdir(dist, { recursive: true });

function run(command, args, cwd = root) {
  const result = Bun.spawnSync([command, ...args], { cwd, stdout: "inherit", stderr: "inherit" });
  if (!result.success) process.exit(result.exitCode ?? 1);
}

run("cargo", ["build", "-p", "mini-canvas-core", "--release", "--target", "wasm32-unknown-unknown"]);
const wasmBindgen = process.platform === "win32"
  ? resolve(process.env.USERPROFILE ?? "", ".cargo", "bin", "wasm-bindgen.exe")
  : "wasm-bindgen";
const wasmPath = resolve(root, "target/wasm32-unknown-unknown/release/mini_canvas_core.wasm");
run(wasmBindgen, [
  "--target", "web",
  "--out-dir", process.platform === "win32" ? wasmDir.replaceAll("\\", "/") : wasmDir,
  process.platform === "win32" ? wasmPath.replaceAll("\\", "/") : wasmPath
]);
await Bun.$`${process.execPath} x tsc -p ${resolve(pkg, "tsconfig.json")}`;
await Bun.write(resolve(pkg, "dist/index.cjs"),
  "const esm = import('./index.js');\n" +
  "exports.createCanvas = (...args) => esm.then((m) => m.createCanvas(...args));\n" +
  "exports.Canvas = { create: (...args) => esm.then((m) => m.Canvas.create(...args)) };\n"
);
console.log("Built WASM and TypeScript package.");
