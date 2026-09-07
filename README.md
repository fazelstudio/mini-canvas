# @fazelstudio/mini-canvas

A small, fast 2D canvas renderer for Node.js and Bun. The renderer is a Rust
core (`tiny-skia` + `fontdue`) compiled to WebAssembly, and the published
package ships **zero runtime npm dependencies** — all codec, rasterization,
and text logic lives inside the ~961 KB `.wasm` core.

## Highlights

- **Zero runtime dependencies** — `dependencies` is `{}`; nothing to audit,
  nothing to break on install.
- **Small** — ~375 kB packed tarball (12 files, 1.0 MB unpacked).
- **Fast** — on the reference card workload (640×360, mixed
  shapes/text/gradient/cover-image) it renders **~113–128 cards/s** (8.2 ms/card best, 122 cards/s median) on a modest
  Ryzen 5 3500U laptop. Repeated text draws cost ~0.1 ms thanks to 2048-entry keyed
  glyph/advance caches with LRU; repeated `clipCircle` hits 8-entry memoized masks (0.86 ms → 0.012 ms); `toDataURL` uses `Buffer` base64. See `benchmarks/RESULTS.md` for the full methodology.
- **Tested for pixel fidelity** — 16 Rust unit tests, package smoke tests, and
  pixel-diff snapshot tests (references regenerated explicitly; output is
  stable). CI runs the matrix on Node 22/24 + Bun for every push.

## Install

```sh
npm install @fazelstudio/mini-canvas
```

## Quick start

```js
import { readFile, writeFile } from "node:fs/promises";
import { createCanvas } from "@fazelstudio/mini-canvas";

const canvas = await createCanvas(640, 360, { background: "#ffffff" });
const font = canvas.loadFont(await readFile("FiraSans-Regular.ttf"));

canvas.fillLinearGradient([0, 0, 640, 360], [0, 0], [640, 360], "#9ad8f5", "#e8f4f8");
canvas.roundRect(24, 24, 592, 312, 18, "#2364aa");
canvas.setFont(font, 28).fillText("Welcome", 24, 48, "#ffffff");
canvas.clipCircle(480, 140, 70)
  .drawImage(await readFile("photo.jpg"), 410, 70, 140, 140, "cover");

await writeFile("card.png", canvas.toBuffer("png"));
// await writeFile("card.jpg", canvas.toBuffer("jpeg", 90));
```

The API is chainable and intentionally asynchronous only at `createCanvas`
(WASM initialization); every draw call is synchronous. It covers solid and
rounded rectangles, strokes, paths with arcs, linear/radial gradients,
transforms, rect/rounded/circle clipping, PNG/JPEG decode, PNG/JPEG export,
and fontdue-backed text with auto-shrink. The full API tour lives in
[`packages/mini-canvas/README.md`](packages/mini-canvas/README.md).

## Building from source

```sh
bun install                 # workspace install (Bun is the project's package manager)
bun run build               # cargo → wasm-bindgen → tsc
bun test                    # smoke + font + snapshot tests (17 tests)
cargo test                  # Rust unit tests in core-rs/ (16 tests)
bun run bench               # benchmark → JSON stdout
bun run bench:update        # benchmark + auto-update benchmarks/RESULTS.md
BENCH_UPDATE=1 bun run test # test + auto-update RESULTS.md via posttest hook
bun run test:update         # same, explicit
```

Note: `packages/mini-canvas/dist/` and `wasm/` are build outputs and are not
committed — CI rebuilds them from source on every push, and `npm pack`
includes them from disk via the package's `files` field.

## Repository layout

```
core-rs/                 Rust renderer core (single lib.rs, wasm32 target)
packages/mini-canvas/    Published npm package (TS wrapper + WASM glue)
examples/                Four runnable examples (output written next to them)
benchmarks/              Dependency-free benchmark + RESULTS.md
fixtures/                Shared test/example assets (Fira Sans, generated JPEG)
modules/                 Internal working documents (not published)
scripts/                 Build orchestration
```

## Documentation

- [`packages/mini-canvas/README.md`](packages/mini-canvas/README.md) — API
  quick start and text-rendering notes
- [`examples/README.md`](examples/README.md) — what each example demonstrates
- [`benchmarks/RESULTS.md`](benchmarks/RESULTS.md) — measured numbers and
  methodology; competitor comparison deferred (see `DECISIONS.md`)
- [`DECISIONS.md`](DECISIONS.md) — design decisions and their rationale
- `modules/PROGRESS.md` — milestone status (internal)
- `modules/AGENT_INSTRUCTIONS.md` — original project brief (internal)

## License

MIT — see [`packages/mini-canvas/LICENSE`](packages/mini-canvas/LICENSE).
Bundled third-party Rust crates and the Fira Sans font are covered by their
own permissive licenses, documented in
[`packages/mini-canvas/THIRD-PARTY-LICENSES.md`](packages/mini-canvas/THIRD-PARTY-LICENSES.md)
and [`packages/mini-canvas/NOTICE`](packages/mini-canvas/NOTICE).
