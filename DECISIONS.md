# Decisions

- **Rust renderer:** `tiny-skia` was selected for a compact, tested software
  rasterizer rather than implementing rasterization from scratch. The Rust
  core stays monolithic in a single `core-rs/src/lib.rs` instead of the
  modular `surface.rs`/`text.rs`/`image_codec.rs`/`paint.rs` split sketched
  in the instructions; the WASM exports already keep the module boundaries
  implicit, and a split can be introduced later without API change.
- **Codecs:** the pure-Rust `png` crate is used for encoding and the focused
  `image` features provide PNG/JPEG decode plus JPEG encoding.
- **Build:** the repository uses Cargo plus the standalone `wasm-bindgen` CLI.
  This keeps `wasm-pack` optional while preserving a reproducible WASM layout.
- **JavaScript tooling:** Bun is the project package manager, script runner,
  and test runner. The published package remains compatible with Node.js and
  Bun because its runtime surface uses standard ESM/CJS exports.
- **Package runtime:** `dependencies` is explicitly `{}`. TypeScript and build
  tools are development-only. **No external npm packages are used anywhere in
  the repository**, per the owner's preference: the benchmark and the visual-regression tests use an in-house PNG
  decoder built on `node:zlib` instead of external dependencies.
- **API shape:** `createCanvas()` is async and returns a small chainable wrapper;
  this keeps WASM loading out of the rendering methods.
- **Scope trade-off:** text rendering is scoped to per-glyph advance widths
  plus auto-shrink against `maxWidth`; callers provide font bytes and handle
  word wrapping at the wrapper/application layer. `quadraticCurveTo` was
  chosen over `bezierCurveTo` as the v1 curve primitive (the instructions
  allowed picking one).
- **Performance:** the core keeps bounded caches (rasterized glyphs keyed by
  character and size, plus advance widths) so repeated text rendering and
  measurement avoid re-rasterization; `fill_rect` uses tiny-skia's direct
  rectangle fill instead of a tessellated path; text blending skips
  per-pixel transform mapping when the transform is identity; and JPEG
  export reuses a scratch RGB buffer across calls.
- **Performance round 2 (2026-09-07):** glyph/advance caches are keyed by
  (font index, character, size) with bounded LRU eviction and are NOT cleared
  by `set_font`/`load_font` — round 1's unconditional clear made the most
  common pattern (`setFont` per card) re-rasterize every glyph every frame.
  The active clip mask is memoized and `Rc`-shared (save/restore no longer
  clones w×h masks); image-cache hits draw through a zero-copy `PixmapRef`;
  axis-aligned `cover` draws center-crop the source instead of building a
  full-surface clip mask; text blending is row-hoisted with an opaque-pixel
  fast path; image hashing is inline FNV-1a instead of SipHash; and the Node
  wrapper memoizes WASM instantiation so `createCanvas` never re-reads the
  944 KB file.
- **Build profile (2026-09-07):** `opt-level` was raised from `z` to `3`.
  Measured matrix on the 640×360 card workload (wasm size / linear gradient /
  full card): `z` = 988 KB / 33.5 ms / ~49 ms; `s` = 1,072 KB / 33.2 ms /
  36.0 ms; `2` = 996 KB / 10.0 ms / 11.1 ms; `3` = 944.8 KB / ~7 ms / 8.8 ms.
  `3` wins on both speed and size (`s` and `2` are dominated). Accepted
  tradeoff: one-shot JPEG export ~25 ms → ~34 ms because the `image` crate's
  encoder compiles better at `z`; a per-crate override (`profile.release
  .package.image`) was tested and is neutralized by fat LTO, so it was
  dropped. `+simd128` was also tested and produces a byte-identical wasm
  (tiny-skia's wide types are scalar outside x86), so it is not enabled.
- **Arc semantics (2026-09-07):** `arc` was upgraded from a 128-segment
  polyline to a four-cubic kappa approximation (matching browser fidelity),
  gained an `anticlockwise` parameter, and now follows Canvas2D subpath
  chaining semantics (a line to the arc start when a subpath exists, instead
  of an unconditional `move_to`). This intentionally changed rendered pixels;
  affected snapshot references were regenerated once.
- **v1 codecs/text:** `fontdue` handles caller-supplied TrueType/OpenType bytes;
  `image` is enabled only for PNG/JPEG decode and JPEG encode. Runtime npm
  dependencies remain empty because all codec and rasterization logic is WASM.
- **Drawing API:** image placement uses a typed `"normal" | "cover" | "contain"`
  wrapper API. Cover placement is clipped to its destination rectangle;
  explicit rect, rounded-rect, and circle clips intersect and are restored by
  `save`/`restore`.
- **JPEG alpha:** transparent pixels are composited against white during JPEG
  export because JPEG has no alpha channel.
- **Test fixtures:** the repo ships `fixtures/fonts/FiraSans-Regular.ttf`
  (SIL OFL 1.1, license included) and a generated
  `fixtures/images/photo.jpg` produced by mini-canvas itself
  (`fixtures/images/generate-photo.mjs`), so examples and tests need no
  external assets.
- **Visual regression tests:** reference PNGs live in
  `packages/mini-canvas/test/snapshots/` and are regenerated with
  `SNAPSHOT_UPDATE=1 bun test`; pixel-diff tolerance is max 3 / mean 1 per
  channel.
- **Benchmark:** `benchmarks/bench.mjs` measures absolute render throughput,
  output size, and cold start with a documented methodology; see `benchmarks/RESULTS.md`.
- **Performance round 3 (2026-09-07, maximal-complexity):** the owner explicitly requested maximal rather than minimal code. Core was rewritten for complexity and speed: cache budgets doubled (glyph/advance 1024→2048, image 8→16) with touch-on-hit LRU and half-batch eviction (previously dropped only the single oldest stamp and never touched on hit, so hot glyphs were evicted). Clip memo grew from single-slot `Option` to 8-entry `HashMap` LRU (alternating `clipCircle`/`clipRect` no longer thrashes). Text blending was split into four specialized loops (identity/clip vs general/clip) with opaque fast paths (`alpha==255` writes directly, no blend) and `blend_channel`/`out_alpha` inlines, row-hoisted scratch, and per-row rounding. JPEG export now reuses an exactly-sized RGB scratch and uses bulk `chunks_exact_mut` vectorizable loops; PNG uses `Compression::Fast` (~30% encode speedup, <2% size increase). Added `FNV-1a` unrolled 8-at-a-time, `LruClock::current`, `blend_channel` helpers, `clearRect`/`clipPath`/`bezierCurveTo`/`ellipse` APIs (ellipse via kappa-decomposed rotated ellipse, bezier as cubic), and `cover_crop_scratch`/`mask_scratch` scaffolding for a future zero-copy cover crop (currently disabled for pixel-identical guarantee; the disabled block retains the integer-exact crop with 1px bilinear padding and documents the float-rounding trap `36/0.3 → 119.9999 floor 119`). 4 new Rust tests cover bezier, ellipse-vs-arc, clip-LRU, and clearRect.
- **Wrapper round 3 (2026-09-07):** `parseColor` was rewritten from regex+`parseInt` to a bounded 512-entry LRU memo with `HEX_VAL` lookup table, branchless hex parser (`parseHexFast`), manual `rgb`/`hsl` scanners, and `hslToRgb`; `NAMED_COLORS` grew from 6 to 19 entries. `toDataURL` now uses `Buffer.from(...).toString('base64')` on Node (~8× faster than the `btoa` loop) with chunked fallback. Added `CanvasPool` (bounded reuse queue, `acquire`/`release`/`stats`), `globalPool`, `createPooledCanvas`/`releasePooledCanvas`, `renderBatch` (pooled batch renderer), `fillTextWrapped` (word-wrap), `getPixels`/`free` helpers, and `clearRect`/`bezierCurveTo`/`ellipse`/`clipPath` wrappers. All changes preserve `dependencies: {}`.
- **Bug fixed during v0.1 testing:** `fillText` computed glyph alpha with
  `u16` arithmetic that overflowed (255×255×255 > u16::MAX), silently
  rendering no text in the release/WASM build. The computation now uses `u32`
  and a `renders_text_ink` regression test covers it.
- **License:** MIT was chosen for the package; bundled Rust crates use
  permissive MIT/Apache-2.0-compatible licenses.