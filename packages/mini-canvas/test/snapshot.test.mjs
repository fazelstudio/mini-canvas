// Visual regression snapshot tests.
//
// Renders a set of deterministic scenes and compares the output against
// reference PNGs in ./snapshots with a small per-pixel tolerance.
//
// Regenerate references with:
//   SNAPSHOT_UPDATE=1 bun test
import { test, expect } from "bun:test";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { decodePNG } from "./lib/decode-png.mjs";
import { createCanvas } from "../dist/index.js";

const here = dirname(fileURLToPath(import.meta.url));
const SNAPSHOTS = resolve(here, "snapshots");
const FONT = resolve(here, "../../../fixtures/fonts/FiraSans-Regular.ttf");
const PHOTO = resolve(here, "../../../fixtures/images/photo.jpg");
const UPDATE = process.env.SNAPSHOT_UPDATE === "1";

const scenes = [
  {
    name: "rounded-card",
    render: async () => {
      const canvas = await createCanvas(160, 100, { background: "#ffffff" });
      canvas.fillLinearGradient([12, 12, 136, 76], [12, 12], [148, 88], "#4b6cb7", "#182848");
      canvas.roundRect(30, 30, 60, 40, 10, "#f7c548");
      canvas.strokeRect(30, 30, 60, 40, "#ffffff", 2);
      return canvas;
    },
  },
  {
    name: "gradients",
    render: async () => {
      const canvas = await createCanvas(120, 120);
      canvas.fillRadialGradient([0, 0, 120, 120], [60, 60], 56, "#ffd166", "#073b4c");
      canvas.fillLinearGradient([20, 20, 80, 80], [20, 20], [100, 100], "#ff0000", "#0000ff");
      return canvas;
    },
  },
  {
    name: "text",
    render: async () => {
      const canvas = await createCanvas(200, 80, { background: "#ffffff" });
      const font = canvas.loadFont(readFileSync(FONT));
      canvas.setFont(font, 24).fillText("Hello Mini", 8, 30, "#111111");
      canvas.setFont(font, 14).fillText("Second line", 8, 56, "#e63946");
      return canvas;
    },
  },
  {
    name: "image-clip",
    render: async () => {
      const canvas = await createCanvas(120, 120, { background: "#ffffff" });
      canvas.clipCircle(60, 60, 52);
      canvas.drawImage(readFileSync(PHOTO), 0, 0, 120, 120, "cover");
      return canvas;
    },
  },
  {
    name: "path-transform",
    render: async () => {
      const canvas = await createCanvas(140, 120, { background: "#ffffff" });
      canvas.save().translate(70, 60).rotate(Math.PI / 4);
      canvas.roundRect(-28, -28, 56, 56, 10, "#e63946");
      canvas.restore();
      canvas.beginPath().moveTo(10, 105).lineTo(70, 20).lineTo(130, 105).closePath().fill("#2a9d8f");
      canvas.beginPath().moveTo(20, 40).quadraticCurveTo(60, 5, 100, 40).stroke("#1d3557", 2);
      return canvas;
    },
  },
];

function pixelDiff(actual, expected) {
  const n = actual.data.length;
  let max = 0;
  let sum = 0;
  for (let i = 0; i < n; i += 1) {
    const d = Math.abs(actual.data[i] - expected.data[i]);
    if (d > max) max = d;
    sum += d;
  }
  return { max, mean: sum / n };
}

for (const scene of scenes) {
  test(`snapshot: ${scene.name}`, async () => {
    const canvas = await scene.render();
    const bytes = canvas.toBuffer("png");
    const file = resolve(SNAPSHOTS, `${scene.name}.png`);
    if (UPDATE || !existsSync(file)) {
      mkdirSync(SNAPSHOTS, { recursive: true });
      writeFileSync(file, bytes);
      if (UPDATE) return;
    }
    const actual = decodePNG(bytes);
    const expected = decodePNG(readFileSync(file));
    expect(actual.width).toBe(expected.width);
    expect(actual.height).toBe(expected.height);
    const { max, mean } = pixelDiff(actual, expected);
    expect(max, `max channel diff ${max} for ${scene.name}`).toBeLessThanOrEqual(3);
    expect(mean, `mean channel diff ${mean} for ${scene.name}`).toBeLessThanOrEqual(1);
  });
}