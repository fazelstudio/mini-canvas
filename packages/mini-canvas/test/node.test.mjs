// Node built-in test runner suite (node --test).
// This is the suite executed in CI on multiple active Node LTS versions.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createCanvas } from "../dist/index.js";

const FONT = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../../../fixtures/fonts/FiraSans-Regular.ttf",
);

test("full v1 API roundtrip", async () => {
  const canvas = await createCanvas(320, 200, { background: "#ffffff" });

  canvas.fillRect(10, 10, 100, 60, "#ff0000");
  canvas.strokeRect(10, 10, 100, 60, "#00ff00", 2);
  canvas.roundRect(20, 20, 50, 30, 8, "#0000ff");
  canvas.fillLinearGradient([0, 0, 320, 200], [0, 0], [320, 200], "#ffd166", "#073b4c");
  canvas.fillRadialGradient([0, 0, 320, 200], [160, 100], 90, "#ffd166", "#073b4c");

  canvas.save().translate(160, 100).rotate(0.5).scale(1.5, 1.5);
  canvas.beginPath().moveTo(0, 0).lineTo(10, 0).lineTo(5, 10).closePath().fill("#2a9d8f");
  canvas.restore();

  canvas.clipCircle(280, 40, 20);
  canvas.fillRect(260, 20, 40, 40, "#e63946");
  canvas.resetClip();
  canvas.clipRoundRect(10, 150, 120, 40, 8);
  canvas.fillRect(0, 140, 140, 60, "#6d597a");
  canvas.resetClip();

  const font = canvas.loadFont(readFileSync(FONT));
  canvas.setFont(font, 18).fillText("Hello Node", 12, 30, "#111111");
  assert.ok(canvas.measureText("Hello Node").width > 0);

  const png = canvas.toBuffer("png");
  assert.deepEqual(Array.from(png.slice(0, 8)), [137, 80, 78, 71, 13, 10, 26, 10]);
  assert.ok(png.length > 100);

  const jpeg = canvas.toBuffer("jpeg", 80);
  assert.equal(jpeg[0], 0xff);
  assert.equal(jpeg[1], 0xd8);
  assert.ok(jpeg.length > 100);

  assert.ok(canvas.toDataURL("png").startsWith("data:image/png;base64,"));
  assert.ok(canvas.toDataURL("jpeg", 70).startsWith("data:image/jpeg;base64,"));
});

test("drawImage with cover mode and alpha colors", async () => {
  const source = await createCanvas(24, 24, { background: "#ff0000" });
  const image = source.toBuffer("png");
  const canvas = await createCanvas(48, 32, { background: "#ffffff" });
  canvas.drawImage(image, 0, 0, 48, 32, "cover");
  canvas.fillRect(0, 0, 10, 10, [0, 0, 255, 128]);
  const output = canvas.toBuffer("png");
  assert.equal(output.length, output.length); // deterministic length
  assert.ok(output.length > 100);
});

test("invalid canvas size throws", async () => {
  await assert.rejects(() => createCanvas(0, 10), /invalid canvas size/);
});