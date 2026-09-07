declare module "../wasm/mini_canvas_core.js" {
  export default function init(input?: unknown): Promise<unknown>;
  export class Canvas {
    constructor(width: number, height: number);
    clear(r: number, g: number, b: number, a: number): void;
    set_stroke_width(width: number): void;
    fill_rect(x: number, y: number, w: number, h: number, r: number, g: number, b: number, a: number): void;
    stroke_rect(x: number, y: number, w: number, h: number, r: number, g: number, b: number, a: number): void;
    round_rect(x: number, y: number, w: number, h: number, radius: number, r: number, g: number, b: number, a: number, fill: boolean): void;
    fill_linear_gradient(x: number, y: number, w: number, h: number, x0: number, y0: number, x1: number, y1: number, ...colors: number[]): void;
    fill_radial_gradient(x: number, y: number, w: number, h: number, cx: number, cy: number, radius: number, ...colors: number[]): void;
    begin_path(): void; move_to(x: number, y: number): void; line_to(x: number, y: number): void;
    quadratic_curve_to(cx: number, cy: number, x: number, y: number): void; close_path(): void;
    arc(cx: number, cy: number, radius: number, start: number, end: number): void;
    fill_path(r: number, g: number, b: number, a: number): void; stroke_path(r: number, g: number, b: number, a: number): void;
    save(): void; restore(): void; translate(x: number, y: number): void; scale(x: number, y: number): void;
    rotate(radians: number): void; reset_transform(): void; to_png(): Uint8Array;
    clip_rect(x: number, y: number, w: number, h: number): void;
    clip_round_rect(x: number, y: number, w: number, h: number, radius: number): void;
    clip_circle(cx: number, cy: number, radius: number): void;
    reset_clip(): void;
    load_font(bytes: Uint8Array): number;
    set_font(index: number, size: number): void;
    set_font_size(size: number): void;
    measure_text(text: string, size: number): number;
    fill_text(text: string, x: number, baseline: number, r: number, g: number, b: number, a: number, maxWidth: number): void;
    draw_image(bytes: Uint8Array, x: number, y: number, width: number, height: number, mode: number): void;
    to_jpeg(quality: number): Uint8Array;
  }
}
