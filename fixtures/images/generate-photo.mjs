// Generates fixtures/images/photo.jpg using mini-canvas itself.
// Run with: bun fixtures/images/generate-photo.mjs
import { writeFile } from "node:fs/promises";
import { createCanvas } from "@fazelstudio/mini-canvas";

const W = 640;
const H = 400;

const canvas = await createCanvas(W, H, { background: "#7ec8e3" });

// Sky gradient
canvas.fillLinearGradient([0, 0, W, H], [0, 0], [0, H], "#9ad8f5", "#e8f4f8");

// Sun
canvas.save();
canvas.translate(500, 80);
canvas.beginPath().arc(0, 0, 46, 0, Math.PI * 2).fill("#ffd166");
canvas.translate(-8, 8);
canvas.beginPath().arc(0, 0, 34, 0, Math.PI * 2).fill("#ffe08a");
canvas.restore();

// Mountain silhouette (path with lines)
canvas.beginPath()
  .moveTo(0, 400)
  .lineTo(120, 240)
  .lineTo(230, 340)
  .lineTo(330, 190)
  .lineTo(470, 360)
  .lineTo(640, 250)
  .lineTo(640, 400)
  .closePath()
  .fill("#4a6b8a");

// Foreground hills
canvas.beginPath()
  .moveTo(0, 400)
  .lineTo(0, 330)
  .quadraticCurveTo(160, 260, 330, 330)
  .quadraticCurveTo(500, 390, 640, 320)
  .lineTo(640, 400)
  .closePath()
  .fill("#2f5d3a");

await writeFile(new URL("./photo.jpg", import.meta.url), canvas.toBuffer("jpeg", 90));
console.log("wrote fixtures/images/photo.jpg");