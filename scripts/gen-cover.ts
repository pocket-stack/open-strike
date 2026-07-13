// Generate the EBOOT cover art from the desktop hero shot:
//   crates/openstrike-psp/assets/ICON0.png  (144x80, XMB icon)
//   crates/openstrike-psp/assets/PIC1.png   (480x272, XMB backdrop)
//   crates/openstrike-vita/static/sce_sys/icon0.png (128x128, LiveArea icon)
//
//   bun scripts/gen-cover.ts
//
// Requires ImageMagick (`magick`) and the macOS system fonts. Re-run only
// when the branding or source shot changes — the outputs are committed.

import { existsSync, mkdirSync } from "node:fs";

const repo = new URL("..", import.meta.url).pathname;
const hero = `${repo}docs/hero.jpg`;
const out = `${repo}crates/openstrike-psp/assets`;
const vitaOut = `${repo}crates/openstrike-vita/static/sce_sys`;
const IMPACT = "/System/Library/Fonts/Supplemental/Impact.ttf";
const HELV = "/System/Library/Fonts/HelveticaNeue.ttc";

if (!existsSync(hero)) {
  console.error(`missing source shot: ${hero}`);
  process.exit(1);
}

mkdirSync(out, { recursive: true });
mkdirSync(vitaOut, { recursive: true });

async function magick(args: string[]): Promise<void> {
  const child = Bun.spawn(["magick", ...args], { stdout: "inherit", stderr: "inherit" });
  const status = await child.exited;
  if (status !== 0) throw new Error(`ImageMagick failed with exit ${status}`);
}

// PIC1 — full-screen backdrop: darkened dust courtyard, left→right scrim so
// the lime/white wordmark reads, a footer band for the tagline.
await magick([
  hero,
  "-resize", "480x272^", "-gravity", "center", "-extent", "480x272", "-modulate", "78,88",
  "(", "-size", "480x272", "gradient:rgba(5,8,12,0.86)-rgba(5,8,12,0.28)", ")",
  "-compose", "over", "-composite",
  "(", "-size", "480x272", "xc:none", "-fill", "rgba(4,7,10,0.55)",
  "-draw", "rectangle 0,214 480,272", ")", "-composite",
  "-font", IMPACT, "-gravity", "West",
  "-fill", "#b8f34a", "-pointsize", "62", "-annotate", "+28-16", "OPEN",
  "-fill", "#e8f0f2", "-pointsize", "62", "-annotate", "+28+42", "STRIKE",
  "-font", HELV, "-fill", "#8fa3ad", "-pointsize", "13", "-annotate", "+30+96", "TACTICAL  OPERATIONS",
  "-gravity", "SouthWest", "-fill", "#8fa3ad", "-pointsize", "11", "-annotate", "+12+10",
  "A CS-shaped FPS in TypeScript · PocketJS · Pocket3D",
  `${out}/PIC1.png`,
]);

// ICON0 — compact game icon: right-weighted crate/gun, dark scrim, wordmark.
await magick([
  hero,
  "-resize", "288x160^", "-gravity", "East", "-extent", "288x160", "-modulate", "82,92",
  "(", "-size", "288x160", "gradient:rgba(5,8,12,0.9)-rgba(5,8,12,0.15)", ")",
  "-compose", "over", "-composite", "-font", IMPACT, "-gravity", "West",
  "-fill", "#b8f34a", "-pointsize", "40", "-annotate", "+12-13", "OPEN",
  "-fill", "#e8f0f2", "-pointsize", "40", "-annotate", "+12+22", "STRIKE",
  "-resize", "144x80", `${out}/ICON0.png`,
]);

// Vita icon — square crop with the same wordmark, encoded as the indexed,
// non-interlaced PNG-8 expected by LiveArea.
await magick([
  hero,
  "-resize", "256x256^", "-gravity", "center", "-extent", "256x256", "-modulate", "82,92",
  "(", "-size", "256x256", "gradient:rgba(5,8,12,0.92)-rgba(5,8,12,0.12)", "-rotate", "90", ")",
  "-compose", "over", "-composite", "-font", IMPACT, "-gravity", "NorthWest",
  "-fill", "#b8f34a", "-pointsize", "54", "-annotate", "+16+22", "OPEN",
  "-fill", "#e8f0f2", "-pointsize", "54", "-annotate", "+16+72", "STRIKE",
  "-resize", "128x128", "-colors", "256", "-strip", "-interlace", "none",
  `PNG8:${vitaOut}/icon0.png`,
]);

console.log(
  `wrote ${out}/ICON0.png (144x80) + ${out}/PIC1.png (480x272) + ${vitaOut}/icon0.png (128x128 PNG-8)`,
);
