# FrameWise — Logo Guidelines

## 1. The logo

**Idea:** frame corners form a targeting reticle. The left corner is the **F**, and its middle arm aims straight at the
cyan target. FrameWise locks onto the issues that really cost you frames.

| Version | File | Use for |
|---|---|---|
| Horizontal (primary) | `masters/framewise-horizontal.svg` | App header, website, README, store listings |
| Stacked | `masters/framewise-stacked.svg` | Square-ish spaces, splash screen, merch |
| Symbol | `masters/framewise-symbol.svg` | Anywhere the name is already visible, 48 px and up |
| Symbol, small cut | `masters/framewise-symbol-small.svg` | **32 px and below**: tray icon, favicon, taskbar |
| Wordmark | `masters/framewise-wordmark.svg` | Text-only placements, such as footers or the overlay |

Every master has an `-on-dark` twin with brighter colours. One-colour black, white and violet versions are in `export/`.
The wordmark is custom-drawn, so it uses no font and has no licence to worry about. Never retype it.

## 2. Clear space

Keep clear space of **1 × G** on all sides, where **G = the gap between the two top corners of the symbol** (32 units on
the symbol's 256-unit canvas, about 1/6 of the visible mark). The clear space scales with the logo. The SVG canvases already include this margin.

## 3. Minimum size

| Version | Screen | Print |
|---|---|---|
| Horizontal | 120 px wide | 30 mm wide |
| Stacked | 72 px wide | 20 mm wide |
| Symbol | 48 px (main) · 16 px (small cut) | 6 mm (small cut) |

## 4. Colour

The palette comes from the app's own colour tokens, so the logo and the UI match.

| Role | Light backgrounds | Dark backgrounds | RGB (light) | CMYK (approx.) | Pantone (closest, check swatch) |
|---|---|---|---|---|---|
| Frame: violet | `#6D4AFF` | `#8B73FF` | 109 74 255 | 57 71 0 0 | 2725 C |
| Target: cyan | `#0891B2` | `#22D3EE` | 8 145 178 | 96 19 0 30 | 7704 C (dark: 311 C) |
| Wordmark: ink | `#14151A` | `#F2F3F7` | 20 21 26 | 0 0 0 95 | Black 6 C |
| Tile / app background | — | `#0E0F14` | 14 15 20 | — | — |

**Approved logo and background pairs**
- Full colour (light) on white or `#F7F7FB`.
- Full colour (`-on-dark`) on `#0E0F14`, `#171922` or black.
- **All white** on violet `#6D4AFF`. The colour version disappears on violet.
- One-colour black or white on photos, but only on a calm area.

The cyan dot is the only cyan in the logo. Keep it that way, because in the app cyan means "live data".

## 5. Typography alongside the logo

- **In the app UI:** Segoe UI Variable (the Windows system font).
- **Headings and stats** (benchmark numbers, overlay): Bahnschrift. It ships with Windows and has the same squared
  DIN feel as the wordmark.
- **Web fallback:** `"Segoe UI", system-ui, sans-serif`.

## 6. Don'ts

- Don't stretch, squash or rotate it.
- Don't recolour it outside the palette or swap the frame and target colours.
- Don't add glows, gradients, shadows or outlines, even though gaming brands love them.
- Don't use the main symbol below 32 px. Use the small cut instead.
- Don't move the target dot or change the size of any part of a lockup.
- Don't type "FrameWise" in a font in place of the wordmark.

## 7. Where things are

- `masters/`: colour master SVGs for light and dark.
- `export/`: one-colour SVGs and PNGs, the favicon and web icon set, and the 1024 px app icon.
- `presentation/`: the presentation board (HTML) and slide PNGs.
- `src/build_kit.py`: the construction code. Change values here and re-run it, rather than hand-editing the SVGs.
