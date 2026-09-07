# @fazelstudio/mini-canvas

Small, fast 2D canvas renderer for Node.js, Bun and edge runtimes. Zero runtime npm dependencies — the renderer is a Rust `tiny-skia` + `fontdue` core compiled to WebAssembly (~961 KB). Published to the `@fazelstudio` npm org with `publishConfig.access: public`.

```js
import { createCanvas } from "@fazelstudio/mini-canvas";
import { writeFile } from "node:fs/promises";

const canvas = await createCanvas(320, 180, { background: "#fff" });
canvas.roundRect(20, 20, 280, 140, 18, "#2364aa");
canvas.fillRect(40, 55, 240, 45, "#f7c548");
await writeFile("card.png", canvas.toBuffer("png"));
```

The v0.1 foundation includes solid and rounded rectangles, strokes, paths,
linear/radial gradients, transforms, clipping, PNG/JPEG image decoding, and
fontdue-backed text. Text rendering rasterizes glyphs on demand and caches
them per (character, size) so repeated text costs nothing after the first
draw; text shaping follows the v1 scope: per-glyph advance widths with
auto-shrink against `maxWidth`.

```js
import { readFile, writeFile } from "node:fs/promises";
import { createCanvas } from "@fazelstudio/mini-canvas";

const canvas = await createCanvas(320, 220, { background: "#182033" });
const font = canvas.loadFont(await readFile("fixtures/fonts/FiraSans-Regular.ttf"));
canvas.setFont(font, 28).fillText("Welcome", 24, 48, "#ffffff");
canvas.clipCircle(160, 130, 70)
  .drawImage(await readFile("fixtures/images/photo.jpg"), 90, 60, 140, 140, "cover");
await writeFile("welcome.jpg", canvas.toBuffer("jpeg", 88));
```

The repository ships example fixtures (Fira Sans, SIL OFL 1.1, and a
generated JPEG) under `fixtures/`; any TrueType/OpenType font or PNG/JPEG
file works in their place.

Use `measureText(text).width` for width measurement; advance widths are
cached (2048-entry LRU) so repeated measurement is cheap. Text shaping beyond advance widths
(ligatures, kerning pairs), colored emoji remain application-level in v1; `fillTextWrapped` helper and `CanvasPool`/`renderBatch` are available for batch workloads.

```js
import { createPooledCanvas, releasePooledCanvas } from "@fazelstudio/mini-canvas";
// Reuse canvas instances across 500 cards — no per-card alloc
const canvas = await createPooledCanvas(640, 360);
canvas.clear("#fff").roundRect(0,0,640,360,18,"#2364aa");
releasePooledCanvas(canvas);
```

**Install:** `npm install @fazelstudio/mini-canvas` or `bun add @fazelstudio/mini-canvas`  
**Benchmark:** `bun run bench:update` auto-writes `benchmarks/RESULTS.md` (also `BENCH_UPDATE=1 bun run test`)  
**License:** MIT — see `LICENSE`; third-party crates in `THIRD-PARTY-LICENSES.md`
