// Minimal PNG decoder for test use only (RGBA8, non-interlaced, filters 0-4).
// Uses only node:zlib — no external packages.
import { inflateSync } from "node:zlib";

const SIGNATURE = [137, 80, 78, 71, 13, 10, 26, 10];

/**
 * Decodes an 8-bit RGBA, non-interlaced PNG into { width, height, data }
 * where data is a Buffer of width*height*4 bytes (RGBA order).
 * @param {Buffer|Uint8Array} input
 */
export function decodePNG(input) {
  const buf = Buffer.isBuffer(input) ? input : Buffer.from(input);
  for (let i = 0; i < SIGNATURE.length; i += 1) {
    if (buf[i] !== SIGNATURE[i]) throw new Error("not a PNG file");
  }

  let offset = 8;
  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  const idat = [];

  while (offset + 8 <= buf.length) {
    const length = buf.readUInt32BE(offset);
    const type = buf.toString("ascii", offset + 4, offset + 8);
    const dataStart = offset + 8;
    const dataEnd = dataStart + length;
    if (dataEnd > buf.length) throw new Error("truncated PNG chunk");
    if (type === "IHDR") {
      width = buf.readUInt32BE(dataStart);
      height = buf.readUInt32BE(dataStart + 4);
      bitDepth = buf[dataStart + 8];
      colorType = buf[dataStart + 9];
    } else if (type === "IDAT") {
      idat.push(buf.subarray(dataStart, dataEnd));
    } else if (type === "IEND") {
      break;
    }
    offset = dataEnd + 4; // skip CRC
  }

  if (!width || !height) throw new Error("missing IHDR");
  if (bitDepth !== 8 || colorType !== 6) {
    throw new Error(`unsupported PNG: bitDepth=${bitDepth} colorType=${colorType} (only RGBA8 is supported)`);
  }

  const raw = inflateSync(Buffer.concat(idat));
  const channels = 4;
  const stride = width * channels;
  const out = Buffer.alloc(height * stride);
  let prev = Buffer.alloc(stride);

  for (let y = 0; y < height; y += 1) {
    const rowStart = y * (stride + 1);
    const filter = raw[rowStart];
    const line = raw.subarray(rowStart + 1, rowStart + 1 + stride);
    const cur = out.subarray(y * stride, (y + 1) * stride);
    for (let i = 0; i < stride; i += 1) {
      const a = i >= channels ? cur[i - channels] : 0;
      const b = prev[i];
      const c = i >= channels ? prev[i - channels] : 0;
      let value = line[i];
      switch (filter) {
        case 0: // None
          break;
        case 1: // Sub
          value = (value + a) & 0xff;
          break;
        case 2: // Up
          value = (value + b) & 0xff;
          break;
        case 3: // Average
          value = (value + ((a + b) >> 1)) & 0xff;
          break;
        case 4: { // Paeth
          const p = a + b - c;
          const pa = Math.abs(p - a);
          const pb = Math.abs(p - b);
          const pc = Math.abs(p - c);
          const predictor = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
          value = (value + predictor) & 0xff;
          break;
        }
        default:
          throw new Error(`unknown PNG filter ${filter}`);
      }
      cur[i] = value;
    }
    prev = cur;
  }

  return { width, height, data: out };
}