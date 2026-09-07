import { createCanvas } from "@fazelstudio/mini-canvas";
import { writeFile } from "node:fs/promises";

const canvas = await createCanvas(320, 240, { background: "white" });
canvas.save().translate(160, 120).rotate(Math.PI / 4);
canvas.roundRect(-60, -60, 120, 120, 18, "#e63946");
canvas.restore();
canvas.beginPath().moveTo(40, 190).lineTo(160, 50).lineTo(280, 190).closePath().fill("#2a9d8f");
await writeFile("path-and-transform.png", canvas.toBuffer());
