import { createCanvas } from "@fazelstudio/mini-canvas";
import { writeFile } from "node:fs/promises";

const canvas = await createCanvas(640, 360, { background: "#f4f7fb" });
canvas.roundRect(24, 24, 592, 312, 24, "#182848");
canvas.fillLinearGradient([48, 48, 544, 264], [48, 48], [592, 312], "#4b6cb7", "#182848");
canvas.roundRect(80, 90, 180, 120, 16, "#f7c548");
canvas.strokeRect(80, 90, 180, 120, "#ffffff", 3);
await writeFile("basic-card.png", canvas.toBuffer());
