import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { inflateSync } from "node:zlib";
import {
  resolveVitaPackageAssets,
  VITA_REQUIRED_SYSTEM_ASSETS,
} from "../vendor/pocketjs/scripts/vita-package.ts";

const root = new URL("..", import.meta.url).pathname;
const icon = `${root}crates/openstrike-vita/static/sce_sys/icon0.png`;

describe("OpenStrike Vita package artwork", () => {
  test("ships a complete 128x128 indexed LiveArea icon", () => {
    const png = readFileSync(icon);
    expect(png.subarray(0, 8)).toEqual(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
    expect(png.subarray(12, 16).toString("ascii")).toBe("IHDR");
    expect(png.readUInt32BE(16)).toBe(128);
    expect(png.readUInt32BE(20)).toBe(128);
    expect(png[24]).toBe(8);
    expect(png[25]).toBe(3);
    expect(png[28]).toBe(0);
    expect(png.byteLength).toBeLessThanOrEqual(420 * 1024);

    const types: string[] = [];
    const imageData: Buffer[] = [];
    let offset = 8;
    while (offset < png.length) {
      const length = png.readUInt32BE(offset);
      const type = png.subarray(offset + 4, offset + 8).toString("ascii");
      const dataEnd = offset + 8 + length;
      expect(dataEnd + 4).toBeLessThanOrEqual(png.length);
      types.push(type);
      if (type === "IDAT") imageData.push(png.subarray(offset + 8, dataEnd));
      offset = dataEnd + 4;
      if (type === "IEND") break;
    }
    expect(offset).toBe(png.length);
    expect(types).toContain("PLTE");
    expect(types.at(-1)).toBe("IEND");
    const scanlines = inflateSync(Buffer.concat(imageData));
    expect(scanlines.byteLength).toBe(128 * (1 + 128));
    const filters = Array.from({ length: 128 }, (_, row) => scanlines[row * 129]!);
    expect(filters.every((filter) => filter <= 4)).toBe(true);
  });

  test("overlays its icon on PocketJS's complete LiveArea", () => {
    const assets = resolveVitaPackageAssets({
      applicationAssets: `${root}crates/openstrike-vita/static`,
    });
    const destinations = new Set(assets.map((asset) => asset.destination));
    for (const path of VITA_REQUIRED_SYSTEM_ASSETS) expect(destinations.has(path)).toBe(true);
    expect(assets.find((asset) => asset.destination === "sce_sys/icon0.png")?.source).toBe(icon);
  });
});
