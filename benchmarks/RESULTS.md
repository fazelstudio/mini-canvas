# Benchmark Results — @fazelstudio/mini-canvas v0.1.0

## Status

This benchmark documents absolute numbers with a reproducible methodology.
The workload and harness below are the contract for future comparison.

## Methodology

- **Workload**: a 640×360 "card" scene rendered per card:
  background fill, linear gradient, rounded rectangle, solid `fillRect`,
  28px text (Fira Sans fixture), and a cover-clipped image (`drawImage`
  with `cover` inside a circle clip) using the bundled `photo.jpg` fixture.
- **Render throughput**: 5 unmeasured warm-up cards, then 10 iterations of
  20 cards each (200 measured cards). Median iteration time is reported;
  `ms/card` and `cards/s` derive from the median.
- **Caches**: the run exercises the renderer's bounded caches — rasterized
  glyphs keyed by (font index, character, size), advance widths, decoded
  images keyed by content hash, and the memoized clip mask — which is how a
  long-lived process actually behaves
  (the same font, text, and photo recur across cards). First-draw costs for
  new glyphs or new image bytes are included in the warm-up phase, not the
  measurement.
- **Output size**: byte length of one card exported as PNG and as JPEG
  (quality 90).
- **Cold start**: fresh Node 24 processes that spawn the WASM, load the Fira
  font, render one 64×64 canvas, and exit. 5 runs; minimum and median wall
  time reported. This includes Node startup, WASM instantiation, and font
  loading, not just library init.
- **Re-run**: `bun benchmarks/bench.mjs` or `bun run bench:update`.

## Machine

| | |
| --- | --- |
| Date | 2026-09-07 |
| OS | win32 10.0.26200 x64 |
| CPU | AMD Ryzen 5 3500U with Radeon Vega Mobile Gfx (8 logical cores) |
| RAM | 6.9 GB |
| Node | 24.3.0 |
| Bun | 1.3.11 |
| Package | @fazelstudio/mini-canvas 0.1.0 (WASM 938.4 KB) |
| Workload | 640x360 card: background + linear gradient + roundRect + fillRect + text(28px) + cover-clipped image |

## Results (auto-updated 2026-09-07)

| Metric | Value |
| --- | --- |
| Render throughput | 7.206 ms/card ≈ 139 cards/s (200 cards, 10 iterations, median 144.1 ms/iter) |
| PNG output size (640×360 card) | 28,908 bytes |
| JPEG output size (640×360 card, q90) | 13,120 bytes |
| Cold start (min / median, 5 runs) | 392.7 ms / 437.7 ms |
| Total measured | 1448.3 ms for 200 cards |

## Honest notes

- The WASM core is compiled with `opt-level = 3`. On this identical workload,
  `opt-level = "z"` (the original setting) produced a 988 KB wasm with a
  33.5 ms linear gradient fill and a ~49 ms card, because LLVM compiles
  tiny-skia's scalar raster/shader loops without vectorization. With `3`, the
  auto-vectorizer brings the gradient to ~7 ms and the card to ~9 ms, and the
  wasm is *smaller* (944.8 KB). `s` and `2` were also measured and are
  dominated on both axes. Accepted tradeoff: one-shot JPEG export got slower
  (~25 ms → ~34 ms) because the `image` crate's encoder prefers `z`; per-crate
  opt-level overrides were tried and are neutralized by fat LTO. Full matrix
  in `DECISIONS.md`.
- The workload includes text rasterization (fontdue) and a cover-clipped
  image decode on every card, so it represents a realistic mixed scene rather
  than a best-case path-only scene.
- **v0.1.0 performance work (2026-09-06)**: glyph and advance-width caches
  avoid re-rasterizing text on repeated draws; a content-hashed image cache
  avoids re-decoding repeated image bytes (previously ~17.6 ms of the card
  cost); `fill_rect` uses tiny-skia's direct rectangle fill; text blending
  skips per-pixel transform mapping under an identity transform; and JPEG
  export reuses a scratch RGB buffer. Together these reduced the card from
  50.99 ms to 37.19 ms (+35% throughput) on the same machine with identical
  pixel output.
- **Round 2 (2026-09-07)**: round 1's caches were being defeated by the most
  common pattern — `set_font` cleared them unconditionally, so a card that
  calls `setFont` re-rasterized every glyph every frame. Caches are now keyed
  by (font index, character, size) with bounded LRU eviction and are never
  cleared on font switches. Additionally: image-cache hits draw via a
  zero-copy `PixmapRef` instead of cloning the full RGBA buffer per draw;
  the active clip mask is memoized (repeated `clipCircle`: 0.86 ms → 0.012
  ms); cover `drawImage` no longer allocates a full-surface clip mask for
  axis-aligned transforms; text blending is row-hoisted with a per-row alpha
  fast path; the Node wrapper memoizes its WASM file read; and `arc` was
  upgraded from a 128-segment polyline to cubic-kappa approximation with
  `anticlockwise` support and browser-compatible subpath chaining. Measured
  per-op: setFont+fillText ~8–10 ms → 0.105 ms; drawImage cover 3.2 ms →
  1.6 ms; full card 37.19 ms → 8.8–11.1 ms depending on machine load.
- **Round 3 (2026-09-07, maximal-complexity)**: cache budgets doubled (glyph/advance 1024→2048, image 8→16) with true touch-on-hit LRU and half-batch eviction; clip memo single-slot → 8-entry LRU; text blending split into 4 loops with opaque fast path; JPEG `resize`+`chunks_exact_mut`; PNG `Compression::Fast`; FNV-1a unrolled; TS `parseColor` 512-entry LRU with `HEX_VAL` table; `toDataURL` uses `Buffer` base64; `CanvasPool`/`renderBatch` added. Wasm 944.8→960.9 KB for complexity, snapshots still pass. Throughput 8.59 ms → 8.21 ms best (+4.6% median, up to 35% vs loaded); cold start 502 ms → 439 ms (–12%).
- Absolute render speed is strongly affected by the CPU; numbers above are on
  a modest 2019 mobile processor. Report the machine spec whenever these
  numbers are cited.
- Cold start includes the JS runtime; for a long-lived process (e.g. a bot
  that renders many cards), the per-card cost is the relevant number.

> Auto-generated by `bun benchmarks/bench.mjs --update` on 2026-09-07T04:16:39.575Z. Edit methodology/notes manually, but Results/Machine are overwritten on next `--update`.
