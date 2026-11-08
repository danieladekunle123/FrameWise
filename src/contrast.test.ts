// WCAG contrast check for both themes, straight from the design tokens in styles.css. The design
// document requires all text to meet AA (4.5:1; 3:1 for large text and UI parts) in both themes.
import { describe, expect, it } from "vitest";
import css from "./styles.css?raw";

function block(selector: string): Record<string, string> {
  const start = css.indexOf(selector);
  const body = css.slice(css.indexOf("{", start) + 1, css.indexOf("}", start));
  const out: Record<string, string> = {};
  for (const m of body.matchAll(/--([\w-]+):\s*([^;]+);/g)) out[m[1]] = m[2].trim();
  return out;
}

const themes = {
  light: block(':root[data-theme="light"]'),
  dark: block(':root[data-theme="dark"]'),
};

type RGB = [number, number, number];
const rgb = (hex: string): RGB => {
  const h = hex.replace("#", "");
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)) as RGB;
};
const luminance = ([r, g, b]: RGB) => {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
};
const ratio = (a: RGB, b: RGB) => {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};
/** color-mix(in srgb, c pct%, base): the tinted backgrounds of badges, banners and cards. */
const mix = (c: RGB, pct: number, base: RGB): RGB => c.map((v, i) => Math.round(v * pct + base[i] * (1 - pct))) as RGB;

for (const [name, t] of Object.entries(themes)) {
  const c = (k: string) => rgb(t[k]);
  const tint = parseFloat(t.tint) / 100;

  describe(`contrast, ${name} theme`, () => {
    it("has every token the check needs", () => {
      for (const k of ["bg", "surface", "surface-2", "text", "text-2", "accent", "on-accent", "live", "success", "warning", "critical", "highlight", "success-text", "warning-text", "tint"]) {
        expect(t[k], k).toBeTruthy();
      }
    });

    // Normal-size text: 4.5:1.
    const text: [string, string][] = [
      ["text", "bg"],
      ["text", "surface"],
      ["text", "surface-2"],
      ["text-2", "bg"],
      ["text-2", "surface"],
      ["text-2", "surface-2"],
      ["on-accent", "accent"],
      ["accent", "surface"], // links and link-style buttons
      ["accent", "bg"],
      ["success-text", "surface"],
      ["warning-text", "surface"],
      ["critical", "surface"], // error text
    ];
    it.each(text)("%s on %s is at least 4.5:1", (fg, bg) => {
      expect(ratio(c(fg), c(bg))).toBeGreaterThanOrEqual(4.5);
    });

    // Badge and banner text sits on a tint of its status colour over the surface.
    it.each(["highlight", "warning", "success", "critical", "accent", "live"])("text on a %s tint is at least 4.5:1", (k) => {
      expect(ratio(c("text"), mix(c(k), tint, c("surface")))).toBeGreaterThanOrEqual(4.5);
    });

    // Large text (the live tiles' 26 px numbers) and UI parts (switches, icons): 3:1.
    it.each([
      ["live", "surface"],
      ["accent", "surface"],
      ["warning", "surface"],
      ["success", "surface"],
    ])("%s on %s is at least 3:1 (large text and UI parts)", (fg, bg) => {
      expect(ratio(c(fg), c(bg))).toBeGreaterThanOrEqual(3);
    });
  });
}
