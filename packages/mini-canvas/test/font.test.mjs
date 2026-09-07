// Font/text tests using the committed Fira Sans fixture font.
import { test, expect } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { decodePNG } from "./lib/decode-png.mjs";
import { createCanvas } from "../dist/index.js";

const FONT = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../../../fixtures/fonts/FiraSans-Regular.ttf",
);
const fontBytes = readFileSync(FONT);

function countInkPixels(pngBuffer, threshold = 128) {
  const { data } = decodePNG(pngBuffer);
  let ink = 0;
  for (let i = 0; i < data.length; i += 4) {
    if (data[i] < threshold) ink += 1;
  }
  return ink;
}

function rightmostInkPixel(pngBuffer, threshold = 128) {
  const { data, width, height } = decodePNG(pngBuffer);
  let rightmost = 0;
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      if (data[(y * width + x) * 4] < threshold) rightmost = Math.max(rightmost, x);
    }
  }
  return rightmost;
}

test("loads a font and measures text deterministically", async () => {
  const canvas = await createCanvas(100, 50, { background: "#ffffff" });
  const index = canvas.loadFont(fontBytes);
  expect(index).toBe(0);
  canvas.setFont(index, 24);
  const width = canvas.measureText("Hello").width;
  expect(width).toBeGreaterThan(0);
  expect(canvas.measureText("Hello").width).toBeCloseTo(width, 5);
});

test("larger font sizes measure wider", async () => {
  const canvas = await createCanvas(100, 50);
  const index = canvas.loadFont(fontBytes);
  canvas.setFont(index, 12);
  const small = canvas.measureText("ABC").width;
  const large = canvas.measureText("ABC", 48).width;
  expect(large).toBeGreaterThan(small * 2);
});

test("setFontSize changes subsequent measurement", async () => {
  const canvas = await createCanvas(100, 50);
  const index = canvas.loadFont(fontBytes);
  canvas.setFont(index, 12);
  const before = canvas.measureText("ABC").width;
  canvas.setFontSize(36);
  const after = canvas.measureText("ABC").width;
  expect(after).toBeGreaterThan(before);
});

test("fillText renders ink pixels", async () => {
  const canvas = await createCanvas(120, 60, { background: "#ffffff" });
  const index = canvas.loadFont(fontBytes);
  canvas.setFont(index, 24).fillText("Hello", 8, 42, "#000000");
  expect(countInkPixels(canvas.toBuffer("png"))).toBeGreaterThan(50);
});

test("auto-shrink keeps text inside maxWidth", async () => {
  const canvas = await createCanvas(200, 60, { background: "#ffffff" });
  const index = canvas.loadFont(fontBytes);
  canvas.setFont(index, 48);
  const natural = canvas.measureText("Auto Shrink").width;
  const maxWidth = natural / 2;
  canvas.fillText("Auto Shrink", 5, 45, "#000000", maxWidth);
  const rightmost = rightmostInkPixel(canvas.toBuffer("png"));
  expect(rightmost).toBeLessThanOrEqual(Math.ceil(5 + maxWidth));
});

test("invalid font data and out-of-range font index throw", async () => {
  const canvas = await createCanvas(10, 10);
  expect(() => canvas.loadFont(new Uint8Array([1, 2, 3]))).toThrow();
  expect(() => canvas.setFont(99, 12)).toThrow();
});

test("text without a loaded font is a no-op", async () => {
  const canvas = await createCanvas(40, 20, { background: "#ffffff" });
  canvas.fillText("no font", 2, 12, "#000000");
  expect(canvas.measureText("no font").width).toBe(0);
  expect(countInkPixels(canvas.toBuffer("png"))).toBe(0);
});