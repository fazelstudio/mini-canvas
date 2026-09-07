import { readFile, writeFile } from "node:fs/promises";
import { createCanvas } from "@fazelstudio/mini-canvas";

const canvas = await createCanvas(640, 360, { background: "#172033" });
const font = canvas.loadFont(await readFile("../fixtures/fonts/FiraSans-Regular.ttf"));
canvas.setFont(font, 42).fillText("Mini canvas", 32, 70, "#ffffff");
canvas.clipRoundRect(32, 100, 576, 220, 24)
  .drawImage(await readFile("../fixtures/images/photo.jpg"), 32, 100, 576, 220, "cover");
await writeFile("text-image.jpg", canvas.toBuffer("jpeg", 88));
