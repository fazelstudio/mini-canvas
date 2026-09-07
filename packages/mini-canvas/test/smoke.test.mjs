import { test, expect } from "bun:test";
import { createCanvas } from "../dist/index.js";

test("creates a PNG", async () => {
  const canvas = await createCanvas(16, 16);
  canvas.roundRect(1, 1, 14, 14, 3, "#ff0000");
  const output = canvas.toBuffer();
  expect(Array.from(output.slice(0, 8))).toEqual([137, 80, 78, 71, 13, 10, 26, 10]);
  expect(output.length).toBeGreaterThan(100);
});

test("clips images and exports JPEG", async () => {
  const source = await createCanvas(24, 24, { background: "#ff0000" });
  const image = source.toBuffer("png");
  const canvas = await createCanvas(48, 32, { background: "#ffffff" });
  canvas.clipCircle(24, 16, 12).drawImage(image, 0, 0, 48, 32, "cover");
  const output = canvas.toBuffer("jpeg", 80);
  expect(Array.from(output.slice(0, 2))).toEqual([0xff, 0xd8]);
  expect(output.length).toBeGreaterThan(100);
});
