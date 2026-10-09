/**
 * A colour per cell name.
 *
 * Three properties make this usable, and all three are easy to get wrong:
 *
 * **Stable.** A name keeps its colour for ever. Colouring by position in a
 * list would repaint the model on every edit, which is exactly when you are
 * watching it.
 *
 * **Independent of how many there are.** `hue = name / total` is the obvious
 * formula and the wrong one: adding one block recolours every other. Stepping
 * the hue by the golden angle instead spreads any number of names evenly
 * without any of them needing to know the total.
 *
 * **Different per dimension.** Face 3 and block 3 are different cells and must
 * not look like the same one.
 *
 * Names are never reused, so hues drift as a session goes on and two distant
 * names can land close together. That is the honest trade for stability: the
 * alternative is a palette that renumbers behind you.
 */

/** 360° / φ. Successive multiples never land near each other. */
const GOLDEN_ANGLE = 137.50776405003785;

/** What the kernel sends for a cell it has not named yet. */
export const UNNAMED = 0xffffffff;

const UNNAMED_COLOUR = "#9aa0a8";

/** Fixed saturation and lightness: a mid-tone reads against both presets, and
 *  letting them vary too would trade legibility for nothing. */
const SATURATION = 0.62;
const LIGHTNESS = 0.56;

/**
 * The colour for a cell, by its stable name and its dimension (2 for a face,
 * 3 for a block).
 */
export function colourForId(name: number, dimension: number): string {
  if (name === UNNAMED) return UNNAMED_COLOUR;
  // The dimension offset is a half-turn-ish shift, so a face and a block of
  // the same number are never confusable.
  const hue = (name * GOLDEN_ANGLE + dimension * 57) % 360;
  return hslToHex(hue < 0 ? hue + 360 : hue, SATURATION, LIGHTNESS);
}

/** HSL to `#rrggbb`. Hex because that is what the theme and `<input
 *  type="color">` both speak. */
export function hslToHex(hue: number, saturation: number, lightness: number): string {
  const c = (1 - Math.abs(2 * lightness - 1)) * saturation;
  const h = hue / 60;
  const x = c * (1 - Math.abs((h % 2) - 1));
  const [r, g, b] =
    h < 1
      ? [c, x, 0]
      : h < 2
        ? [x, c, 0]
        : h < 3
          ? [0, c, x]
          : h < 4
            ? [0, x, c]
            : h < 5
              ? [x, 0, c]
              : [c, 0, x];
  const m = lightness - c / 2;
  const byte = (v: number) =>
    Math.round(Math.min(1, Math.max(0, v + m)) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${byte(r)}${byte(g)}${byte(b)}`;
}

/** `#rrggbb` to three 0..1 components, the way Babylon reads a hex colour —
 *  a plain division by 255, no gamma anywhere. */
export function hexToRgb(hex: string): [number, number, number] {
  const n = parseInt(hex.replace("#", ""), 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

/**
 * The two selection colours.
 *
 * Fixed rather than themed, for two reasons: they have to stand out against
 * whatever the model is coloured, which is the one thing the user can change;
 * and the two must not be confusable with each other, because which is which
 * decides where the seam of a glue ends up.
 */
export const ANCHOR = "#ffcc33";
export const MATE = "#ff5577";
