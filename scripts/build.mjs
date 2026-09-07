import { mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pkg = resolve(root, "packages/mini-canvas");
const wasmDir = resolve(pkg, "wasm");
const dist = resolve(pkg, "dist");
await mkdir(wasmDir, { recursive: true });
await mkdir(dist, { recursive: true });

function run(command, args, cwd = root) {
  const cargoBin = resolve(homedir(), ".cargo", "bin");
  const env = {
    ...process.env,
    PATH: `${cargoBin}:${process.env.PATH ?? ""}:${process.env.HOME ? `${process.env.HOME}/.cargo/bin` : ""}:/home/runner/.cargo/bin:/root/.cargo/bin`,
  };
  const result = Bun.spawnSync([command, ...args], { cwd, stdout: "inherit", stderr: "inherit", env });
  if (!result.success) {
    // Fallback: try with explicit PATH for wasm-bindgen
    if (command === "wasm-bindgen" || command.endsWith("wasm-bindgen") || command.endsWith("wasm-bindgen.exe")) {
      console.error(`[build] failed to spawn "${command}", trying fallback candidates...`);
      for (const cand of [
        resolve(homedir(), ".cargo", "bin", "wasm-bindgen"),
        resolve(homedir(), ".cargo", "bin", "wasm-bindgen.exe"),
        "/home/runner/.cargo/bin/wasm-bindgen",
        "/root/.cargo/bin/wasm-bindgen",
      ]) {
        if (existsSync(cand)) {
          console.error(`[build] retrying with ${cand}`);
          const r2 = Bun.spawnSync([cand, ...args], { cwd, stdout: "inherit", stderr: "inherit", env });
          if (r2.success) return;
        }
      }
      // Final fallback: try cargo run
      console.error(`[build] trying cargo run -p wasm-bindgen-cli`);
      const r3 = Bun.spawnSync(["cargo", "run", "--quiet", "-p", "wasm-bindgen-cli", "--", ...args], { cwd, stdout: "inherit", stderr: "inherit", env });
      if (r3.success) return;
    }
    process.exit(result.exitCode ?? 1);
  }
}

run("cargo", ["build", "-p", "mini-canvas-core", "--release", "--target", "wasm32-unknown-unknown"]);

// Resolve wasm-bindgen with cargo bin in PATH
let wasmBindgen = "wasm-bindgen";
const candidates = [
  resolve(homedir(), ".cargo", "bin", process.platform === "win32" ? "wasm-bindgen.exe" : "wasm-bindgen"),
  "/home/runner/.cargo/bin/wasm-bindgen",
  "/root/.cargo/bin/wasm-bindgen",
];
for (const c of candidates) {
  try { if (c && existsSync(c)) { wasmBindgen = c; break; } } catch {}
}
console.log(`[build] using wasm-bindgen: ${wasmBindgen}`);

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
