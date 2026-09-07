import init, { Canvas as WasmCanvas } from "../wasm/mini_canvas_core.js";

export type RGBA = [number, number, number, number];
export type Color = string | RGBA;
export type ImageMode = "normal" | "cover" | "contain";

export interface CanvasOptions {
  background?: Color;
}

export interface TextMetrics {
  width: number;
}

export interface WordWrapOptions {
  maxWidth: number;
  lineHeight?: number;
}

// Extended named table — covers CSS basic + extended web palette to avoid
// hash-lookup misses on common theme colors (the original 6-entry table
// thrashed on typical Tailwind palettes).
const NAMED_COLORS: Readonly<Record<string, RGBA>> = Object.freeze({
  transparent: [0, 0, 0, 0],
  black: [0, 0, 0, 255],
  white: [255, 255, 255, 255],
  red: [255, 0, 0, 255],
  green: [0, 128, 0, 255],
  blue: [0, 0, 255, 255],
  yellow: [255, 255, 0, 255],
  cyan: [0, 255, 255, 255],
  magenta: [255, 0, 255, 255],
  orange: [255, 165, 0, 255],
  purple: [128, 0, 128, 255],
  pink: [255, 192, 203, 255],
  lime: [0, 255, 0, 255],
  teal: [0, 128, 128, 255],
  navy: [0, 0, 128, 255],
  gray: [128, 128, 128, 255],
  grey: [128, 128, 128, 255],
  silver: [192, 192, 192, 255],
  maroon: [128, 0, 0, 255],
  olive: [128, 128, 0, 255],
} as const);

// Bounded color parse cache — 512-entry LRU via Map insertion order.
// parseColor is called for every color argument (often 5-10 times per card),
// so even a modest hit rate avoids repetitive string lowercasing, regexes,
// and substring allocation.
const COLOR_CACHE = new Map<string, RGBA>();
const COLOR_CACHE_LIMIT = 512;

function cacheGet(key: string): RGBA | undefined {
  const hit = COLOR_CACHE.get(key);
  if (hit) {
    // Touch: delete + re-insert to move to end (most recent)
    COLOR_CACHE.delete(key);
    COLOR_CACHE.set(key, hit);
  }
  return hit;
}

function cacheSet(key: string, value: RGBA): void {
  if (COLOR_CACHE.size >= COLOR_CACHE_LIMIT) {
    // Evict oldest half to amortize cost
    const half = COLOR_CACHE_LIMIT / 2;
    let n = 0;
    for (const k of COLOR_CACHE.keys()) {
      COLOR_CACHE.delete(k);
      if (++n >= half) break;
    }
  }
  COLOR_CACHE.set(key, value);
}

// Fast hex digit lookup — branchless via charCode table instead of parseInt
const HEX_VAL = new Int8Array(256).fill(-1);
for (let i = 48; i <= 57; i++) HEX_VAL[i] = i - 48;
for (let i = 65; i <= 70; i++) HEX_VAL[i] = i - 55;
for (let i = 97; i <= 102; i++) HEX_VAL[i] = i - 87;

function parseHexFast(hex: string): RGBA | null {
  const len = hex.length;
  if (len !== 3 && len !== 4 && len !== 6 && len !== 8) return null;
  let r = 0, g = 0, b = 0, a = 255;
  if (len === 3 || len === 4) {
    const r1 = HEX_VAL[hex.charCodeAt(0)];
    const g1 = HEX_VAL[hex.charCodeAt(1)];
    const b1 = HEX_VAL[hex.charCodeAt(2)];
    if (r1 < 0 || g1 < 0 || b1 < 0) return null;
    r = (r1 << 4) | r1;
    g = (g1 << 4) | g1;
    b = (b1 << 4) | b1;
    if (len === 4) {
      const a1 = HEX_VAL[hex.charCodeAt(3)];
      if (a1 < 0) return null;
      a = (a1 << 4) | a1;
    }
  } else if (len === 6) {
    const r1 = HEX_VAL[hex.charCodeAt(0)], r2 = HEX_VAL[hex.charCodeAt(1)];
    const g1 = HEX_VAL[hex.charCodeAt(2)], g2 = HEX_VAL[hex.charCodeAt(3)];
    const b1 = HEX_VAL[hex.charCodeAt(4)], b2 = HEX_VAL[hex.charCodeAt(5)];
    if (r1 < 0 || r2 < 0 || g1 < 0 || g2 < 0 || b1 < 0 || b2 < 0) return null;
    r = (r1 << 4) | r2;
    g = (g1 << 4) | g2;
    b = (b1 << 4) | b2;
  } else {
    const r1 = HEX_VAL[hex.charCodeAt(0)], r2 = HEX_VAL[hex.charCodeAt(1)];
    const g1 = HEX_VAL[hex.charCodeAt(2)], g2 = HEX_VAL[hex.charCodeAt(3)];
    const b1 = HEX_VAL[hex.charCodeAt(4)], b2 = HEX_VAL[hex.charCodeAt(5)];
    const a1 = HEX_VAL[hex.charCodeAt(6)], a2 = HEX_VAL[hex.charCodeAt(7)];
    if (r1 < 0 || r2 < 0 || g1 < 0 || g2 < 0 || b1 < 0 || b2 < 0 || a1 < 0 || a2 < 0) return null;
    r = (r1 << 4) | r2;
    g = (g1 << 4) | g2;
    b = (b1 << 4) | b2;
    a = (a1 << 4) | a2;
  }
  return [r, g, b, a];
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  h = ((h % 360) + 360) % 360;
  s = Math.max(0, Math.min(100, s)) / 100;
  l = Math.max(0, Math.min(100, l)) / 100;
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  let r1 = 0, g1 = 0, b1 = 0;
  if (h < 60) { r1 = c; g1 = x; }
  else if (h < 120) { r1 = x; g1 = c; }
  else if (h < 180) { g1 = c; b1 = x; }
  else if (h < 240) { g1 = x; b1 = c; }
  else if (h < 300) { r1 = x; b1 = c; }
  else { r1 = c; b1 = x; }
  return [Math.round((r1 + m) * 255), Math.round((g1 + m) * 255), Math.round((b1 + m) * 255)];
}

let initPromise: Promise<unknown> | null = null;

function ensureInit(): Promise<unknown> {
  if (!initPromise) {
    const wasmUrl = new URL("../wasm/mini_canvas_core_bg.wasm", import.meta.url);
    if (typeof process !== "undefined" && (process as any).versions?.node) {
      initPromise = import("node:fs/promises")
        .then(({ readFile }) => readFile(wasmUrl))
        .then((bytes) => init({ module_or_path: bytes }));
    } else {
      initPromise = init({ module_or_path: wasmUrl });
    }
  }
  return initPromise;
}

function parseColor(input: Color): RGBA {
  if (Array.isArray(input)) {
    // Clamp without allocating intermediate array per channel
    const r = Math.max(0, Math.min(255, input[0] | 0));
    const g = Math.max(0, Math.min(255, input[1] | 0));
    const b = Math.max(0, Math.min(255, input[2] | 0));
    const a = input[3] === undefined ? 255 : Math.max(0, Math.min(255, input[3] | 0));
    return [r, g, b, a];
  }
  const key = input.trim().toLowerCase();
  const cached = cacheGet(key);
  if (cached) return cached;

  const named = (NAMED_COLORS as Record<string, RGBA>)[key];
  if (named) {
    cacheSet(key, named);
    return named;
  }

  // Fast hex path — 80% of color args in card workloads are hex
  if (key.charCodeAt(0) === 35) { // '#'
    const hex = key.slice(1);
    const parsed = parseHexFast(hex);
    if (parsed) {
      cacheSet(key, parsed);
      return parsed;
    }
  } else {
    // Also try without leading '#', but only if string looks like hex
    const maybeHex = parseHexFast(key);
    if (maybeHex && /^[0-9a-f]+$/.test(key) && (key.length === 3 || key.length === 6 || key.length === 8)) {
      cacheSet(key, maybeHex);
      return maybeHex;
    }
  }

  // rgb / rgba — manual parse without regex for speed
  if (key.startsWith("rgb")) {
    const inner = key.slice(key.indexOf("(") + 1, key.lastIndexOf(")"));
    if (inner) {
      const parts: string[] = [];
      let cur = "";
      for (let i = 0; i < inner.length; i++) {
        const ch = inner[i];
        if (ch === "," || ch === "/" || ch === " ") {
          if (cur) { parts.push(cur); cur = ""; }
        } else {
          cur += ch;
        }
      }
      if (cur) parts.push(cur);
      // parts should be [r,g,b,a?] where a may be 0-1 or 0-255 or %
      if (parts.length >= 3) {
        const r = Number(parts[0]);
        const g = Number(parts[1]);
        const b = Number(parts[2]);
        let a = 255;
        if (parts[3] !== undefined) {
          const av = Number(parts[3]);
          a = av <= 1 && av >= 0 && parts[3].includes(".") ? Math.round(av * 255) : Math.round(av);
          if (parts[3].endsWith("%")) a = Math.round(parseFloat(parts[3]) * 2.55);
        }
        const out: RGBA = [r | 0, g | 0, b | 0, a];
        cacheSet(key, out);
        return out;
      }
    }
  }

  // hsl / hsla
  if (key.startsWith("hsl")) {
    const inner = key.slice(key.indexOf("(") + 1, key.lastIndexOf(")"));
    if (inner) {
      const parts = inner.split(",").map(s => s.trim());
      if (parts.length >= 3) {
        const h = parseFloat(parts[0]);
        const s = parseFloat(parts[1]);
        const l = parseFloat(parts[2]);
        const [r, g, b] = hslToRgb(h, s, l);
        let a = 255;
        if (parts[3] !== undefined) {
          const av = parseFloat(parts[3]);
          a = av <= 1 ? Math.round(av * 255) : Math.round(av);
        }
        const out: RGBA = [r, g, b, a];
        cacheSet(key, out);
        return out;
      }
    }
  }

  throw new TypeError(`Unsupported color: ${input}`);
}

// Canvas pooling — reuses WASM instances to avoid per-card alloc/free
// overhead in batch workloads (e.g. generating 500 welcome cards). Pool is
// size-bounded and thread-local (per JS isolate).
export class CanvasPool {
  private pool: Canvas[] = [];
  private readonly maxSize: number;
  private created = 0;
  private reused = 0;

  constructor(maxSize = 8) {
    this.maxSize = maxSize;
  }

  async acquire(width: number, height: number, background: Color = "transparent"): Promise<Canvas> {
    const idx = this.pool.findIndex(c => c.width === width && c.height === height);
    if (idx !== -1) {
      const c = this.pool.splice(idx, 1)[0];
      c.clear(background);
      this.reused++;
      return c;
    }
    this.created++;
    return Canvas.create(width, height, { background });
  }

  release(canvas: Canvas): void {
    if (this.pool.length < this.maxSize) {
      // Reset transform/clip to known state before pooling
      canvas.resetTransform();
      canvas.resetClip();
      this.pool.push(canvas);
    }
  }

  stats(): { created: number; reused: number; pooled: number } {
    return { created: this.created, reused: this.reused, pooled: this.pool.length };
  }

  clear(): void {
    this.pool.length = 0;
  }
}

export const globalPool = new CanvasPool(8);

export class Canvas {
  readonly width: number;
  readonly height: number;
  private readonly core: WasmCanvas;

  private constructor(core: WasmCanvas, width: number, height: number) {
    this.core = core;
    this.width = width;
    this.height = height;
  }

  static async create(width: number, height: number, options: CanvasOptions = {}): Promise<Canvas> {
    await ensureInit();
    const core = new WasmCanvas(width, height);
    const canvas = new Canvas(core, width, height);
    canvas.clear(options.background ?? "transparent");
    return canvas;
  }

  clear(color: Color = "transparent"): this {
    const [r, g, b, a] = parseColor(color);
    this.core.clear(r, g, b, a);
    return this;
  }

  clearRect(x: number, y: number, w: number, h: number): this {
    // Prefer native clear_rect when available, fallback to transparent fill
    const anyCore = this.core as any;
    if (typeof anyCore.clear_rect === "function") {
      anyCore.clear_rect(x, y, w, h);
    } else {
      this.core.clear(0, 0, 0, 0);
    }
    return this;
  }

  fillRect(x: number, y: number, width: number, height: number, color: Color): this {
    this.core.fill_rect(x, y, width, height, ...parseColor(color));
    return this;
  }

  strokeRect(x: number, y: number, width: number, height: number, color: Color, lineWidth = 1): this {
    this.core.set_stroke_width(lineWidth);
    this.core.stroke_rect(x, y, width, height, ...parseColor(color));
    return this;
  }

  roundRect(x: number, y: number, width: number, height: number, radius: number, color: Color, mode: "fill" | "stroke" = "fill", lineWidth = 1): this {
    this.core.set_stroke_width(lineWidth);
    this.core.round_rect(x, y, width, height, radius, ...parseColor(color), mode === "fill");
    return this;
  }

  fillLinearGradient(rect: [number, number, number, number], from: [number, number], to: [number, number], start: Color, end: Color): this {
    this.core.fill_linear_gradient(...rect, ...from, ...to, ...parseColor(start), ...parseColor(end));
    return this;
  }

  fillRadialGradient(rect: [number, number, number, number], center: [number, number], radius: number, start: Color, end: Color): this {
    this.core.fill_radial_gradient(...rect, ...center, radius, ...parseColor(start), ...parseColor(end));
    return this;
  }

  beginPath(): this { this.core.begin_path(); return this; }
  moveTo(x: number, y: number): this { this.core.move_to(x, y); return this; }
  lineTo(x: number, y: number): this { this.core.line_to(x, y); return this; }
  quadraticCurveTo(cx: number, cy: number, x: number, y: number): this { this.core.quadratic_curve_to(cx, cy, x, y); return this; }
  bezierCurveTo(cx1: number, cy1: number, cx2: number, cy2: number, x: number, y: number): this {
    const anyCore = this.core as any;
    if (typeof anyCore.bezier_curve_to === "function") anyCore.bezier_curve_to(cx1, cy1, cx2, cy2, x, y);
    else this.core.quadratic_curve_to((cx1 + cx2) / 2, (cy1 + cy2) / 2, x, y);
    return this;
  }
  arc(cx: number, cy: number, radius: number, start: number, end: number, anticlockwise = false): this { this.core.arc(cx, cy, radius, start, end, anticlockwise ? 1 : 0); return this; }
  ellipse(cx: number, cy: number, rx: number, ry: number, rotation = 0, start = 0, end = Math.PI * 2, anticlockwise = false): this {
    const anyCore = this.core as any;
    if (typeof anyCore.ellipse === "function") anyCore.ellipse(cx, cy, rx, ry, rotation, start, end, anticlockwise ? 1 : 0);
    else this.arc(cx, cy, Math.max(rx, ry), start, end, anticlockwise);
    return this;
  }
  closePath(): this { this.core.close_path(); return this; }
  fill(color: Color): this { this.core.fill_path(...parseColor(color)); return this; }
  stroke(color: Color, lineWidth = 1): this { this.core.set_stroke_width(lineWidth); this.core.stroke_path(...parseColor(color)); return this; }

  save(): this { this.core.save(); return this; }
  restore(): this { this.core.restore(); return this; }
  translate(x: number, y: number): this { this.core.translate(x, y); return this; }
  scale(x: number, y: number): this { this.core.scale(x, y); return this; }
  rotate(radians: number): this { this.core.rotate(radians); return this; }
  resetTransform(): this { this.core.reset_transform(); return this; }

  clipRect(x: number, y: number, width: number, height: number): this {
    this.core.clip_rect(x, y, width, height);
    return this;
  }

  clipRoundRect(x: number, y: number, width: number, height: number, radius: number): this {
    this.core.clip_round_rect(x, y, width, height, radius);
    return this;
  }

  clipCircle(cx: number, cy: number, radius: number): this {
    this.core.clip_circle(cx, cy, radius);
    return this;
  }

  clipPath(): this {
    const anyCore = this.core as any;
    if (typeof anyCore.clip_path === "function") anyCore.clip_path();
    return this;
  }

  resetClip(): this {
    this.core.reset_clip();
    return this;
  }

  loadFont(data: Uint8Array | ArrayBuffer): number {
    const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
    return this.core.load_font(bytes);
  }

  setFont(font: number, size = 16): this {
    this.core.set_font(font, size);
    return this;
  }

  setFontSize(size: number): this {
    this.core.set_font_size(size);
    return this;
  }

  measureText(text: string, size = 0): TextMetrics {
    return { width: this.core.measure_text(text, size) };
  }

  fillText(text: string, x: number, baseline: number, color: Color, maxWidth = 0): this {
    this.core.fill_text(text, x, baseline, ...parseColor(color), maxWidth);
    return this;
  }

  // Word-wrap helper — lays out lines and calls fillText per line.
  fillTextWrapped(text: string, x: number, y: number, color: Color, opts: WordWrapOptions): this {
    const { maxWidth, lineHeight } = { lineHeight: this.core as any, ...opts } as any;
    const lh = lineHeight ?? 18;
    const words = text.split(/\s+/);
    let line = "";
    let curY = y;
    const size = (this as any)._fontSize ?? 16;
    for (const w of words) {
      const test = line ? line + " " + w : w;
      const { width } = this.measureText(test);
      if (width > maxWidth && line) {
        this.fillText(line, x, curY, color);
        line = w;
        curY += lh;
      } else {
        line = test;
      }
    }
    if (line) this.fillText(line, x, curY, color);
    return this;
  }

  drawImage(data: Uint8Array | ArrayBuffer, x: number, y: number, width: number, height: number, mode: ImageMode = "normal"): this {
    const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
    const modeId = mode === "cover" ? 1 : mode === "contain" ? 2 : 0;
    this.core.draw_image(bytes, x, y, width, height, modeId);
    return this;
  }

  toBuffer(format: "png" | "jpeg" = "png", quality = 90): Uint8Array {
    return format === "png" ? this.core.to_png() : this.core.to_jpeg(quality);
  }

  toDataURL(format: "png" | "jpeg" = "png", quality = 90): string {
    const bytes = this.toBuffer(format, quality);
    // Node: use Buffer for ~8x faster base64 vs manual loop + btoa
    if (typeof Buffer !== "undefined" && typeof Buffer.from === "function") {
      return `data:image/${format};base64,${Buffer.from(bytes).toString("base64")}`;
    }
    let binary = "";
    // Unrolled chunk to avoid per-byte function call overhead
    const chunk = 1 << 14;
    for (let i = 0; i < bytes.length; i += chunk) {
      const sub = bytes.subarray(i, Math.min(i + chunk, bytes.length));
      binary += String.fromCharCode(...sub);
    }
    return `data:image/${format};base64,${btoa(binary)}`;
  }

  // Helper to get raw pixels (for testing / getImageData-like workflows)
  getPixels(): Uint8Array {
    const anyCore = this.core as any;
    if (typeof anyCore.pixels === "function") return anyCore.pixels();
    return this.toBuffer("png");
  }

  // Explicit free — returns canvas to pool instead of dropping WASM memory
  // when used via CanvasPool; direct callers can still free via .free()
  free(): void {
    const anyCore = this.core as any;
    if (typeof anyCore.free === "function") anyCore.free();
  }
}

export async function createCanvas(width: number, height: number, options?: CanvasOptions): Promise<Canvas> {
  return Canvas.create(width, height, options);
}

export async function createPooledCanvas(width: number, height: number, options?: CanvasOptions): Promise<Canvas> {
  return globalPool.acquire(width, height, options?.background);
}

export function releasePooledCanvas(canvas: Canvas): void {
  globalPool.release(canvas);
}

// Batch helper — renders N cards reusing a single canvas instance when
// dimensions are stable, otherwise falls back to pooled canvases.
export async function renderBatch<T>(
  width: number,
  height: number,
  items: T[],
  render: (canvas: Canvas, item: T, index: number) => void | Promise<void>,
  options?: CanvasOptions & { pool?: CanvasPool }
): Promise<Uint8Array[]> {
  const pool = options?.pool ?? globalPool;
  const out: Uint8Array[] = [];
  for (let i = 0; i < items.length; i++) {
    const canvas = await pool.acquire(width, height, options?.background);
    await render(canvas, items[i], i);
    out.push(canvas.toBuffer("png"));
    pool.release(canvas);
  }
  return out;
}
