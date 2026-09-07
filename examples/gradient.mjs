import { createCanvas } from "@fazelstudio/mini-canvas";
import { writeFile } from "node:fs/promises";

const canvas = await createCanvas(400, 240);
canvas.fillRadialGradient([0, 0, 400, 240], [200, 120], 190, "#ffd166", "#073b4c");
await writeFile("gradient.png", canvas.toBuffer());
