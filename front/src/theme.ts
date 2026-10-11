/**
 * How the scene looks, as against what it contains.
 *
 * This is the one piece of state in the project that deliberately does **not**
 * go through the kernel. The kernel describes what exists; it has no business
 * knowing what colour you like it. So a theme is per viewer, kept in the
 * browser, and two tabs on one station session may legitimately look nothing
 * alike.
 *
 * Everything is driven off the `Theme` keys: the settings panel is generated
 * from them, so adding a colour later costs one entry here and nothing in the
 * interface.
 */

import type { Colouring } from "./surface";
import { hexToRgb } from "./palette";

export interface Theme {
  background: string;
  nodes: string;
  edges: string;
  faces: string;
  blocks: string;
  /** The geometry's three groups. A family of their own — green against the
   *  blocking's blues and browns — so that at a glance you can tell what is
   *  the model and what is the thing the model is meant to follow. */
  corners: string;
  curves: string;
  surfaces: string;
  /** A node nothing may move. Grey, and nothing else is: a frozen node is
   *  not a kind of classification, it is a cell that is out of play. */
  frozen: string;
}

/** Order and labels for the generated settings panel. */
export const SWATCHES: ReadonlyArray<{ key: keyof Theme; label: string }> = [
  { key: "background", label: "background" },
  { key: "nodes", label: "nodes" },
  { key: "edges", label: "edges" },
  { key: "faces", label: "faces" },
  { key: "blocks", label: "blocks" },
  { key: "corners", label: "corners" },
  { key: "curves", label: "curves" },
  { key: "surfaces", label: "surfaces" },
  { key: "frozen", label: "frozen" },
];

export const PRESETS: Record<"dark" | "light", Theme> = {
  dark: {
    background: "#15171b",
    nodes: "#66b2ff",
    edges: "#f2f2ff",
    faces: "#4a6b8a",
    blocks: "#8a6b4a",
    corners: "#6ee7a8",
    curves: "#2fbf71",
    surfaces: "#3f7d5e",
    frozen: "#9aa0a8",
  },
  light: {
    background: "#f4f5f7",
    nodes: "#1763b8",
    edges: "#2b2f37",
    faces: "#8fb0cc",
    blocks: "#ccae8f",
    corners: "#1d8a52",
    curves: "#157f4a",
    surfaces: "#a8d5bd",
    frozen: "#6b7280",
  },
};

/**
 * Is this colour light enough to put dark text on?
 *
 * Used to decide which way round the interface's own colours go, so that a
 * light scene does not leave a slab of night floating on top of it. The
 * coefficients are the usual luminance weights: the eye is far more
 * sensitive to green than to blue.
 */
export function isLight(hex: string): boolean {
  const [r, g, b] = hexToRgb(hex);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.5;
}

/**
 * How one group of entities is drawn.
 *
 * The geometry's three groups take exactly this and no more. There is no
 * shrink: a geometry is not a cellular decomposition you can explode — its
 * surfaces meet along the curves that are also in the picture, and pulling
 * them apart would only break that.
 */
export interface GeomLayer {
  visible: boolean;
  colouring: Colouring;
  /** 1 is opaque. */
  alpha: number;
}

/** How one group of cells of the blocking is drawn. */
export interface Layer extends GeomLayer {
  /** 1 leaves a cell where it is; smaller pulls it towards its own centre. */
  shrink: number;
}

const KEY = "gecko.view";

/** The picture sizes on offer, as multiples of what the canvas renders. */
export const SHOT_SCALES = [1, 2, 4];

export interface ViewPrefs {
  theme: Theme;
  axes: boolean;
  /** The settings drawer, and whether the interface is on screen at all. */
  drawer: boolean;
  chrome: boolean;
  /** How a saved picture comes out. */
  shot: { scale: number; transparent: boolean };
  /** Edges keep one colour — the theme's — so all they carry is whether
   *  they are drawn, and how far they are pulled off the surfaces they
   *  would otherwise lie exactly on. */
  edges: { visible: boolean; shrink: number };
  faces: Layer & { internal: boolean };
  blocks: Layer;
  /** The geometry, which is not edited and so has no notion of a selection
   *  or a shrink — only whether you are looking at it and how. */
  corners: GeomLayer;
  curves: GeomLayer;
  surfaces: GeomLayer;
}

export const DEFAULTS: ViewPrefs = {
  theme: { ...PRESETS.dark },
  axes: true,
  // Shut, because the whole point of a drawer is that the window belongs to
  // the model. The toolbar carries the verbs; this carries the settings.
  drawer: false,
  chrome: true,
  // Twice the canvas, which is what a figure in a paper wants. The
  // background is kept by default: transparency is for dropping a figure
  // onto a page of unknown colour, and surprises anyone not expecting it.
  shot: { scale: 2, transparent: false },
  edges: { visible: true, shrink: 1 },
  // Faces on but well short of opaque, so the wireframe and the handles still
  // read through them. Blocks off: they draw the same surfaces, and two
  // colourings of one shape at once is a choice rather than a default.
  faces: { visible: true, colouring: "uniform", alpha: 0.35, shrink: 1, internal: true },
  blocks: { visible: false, colouring: "id", alpha: 0.55, shrink: 0.85 },
  // All three on, because a geometry you have just loaded is a geometry you
  // want to see. Surfaces by id: telling one CAD patch from the next is the
  // whole reason the file was read as a model rather than as triangles, and
  // one colour for the lot would hide exactly that. Translucent, because
  // what goes inside it is the point.
  corners: { visible: true, colouring: "uniform", alpha: 1 },
  curves: { visible: true, colouring: "uniform", alpha: 1 },
  surfaces: { visible: true, colouring: "id", alpha: 0.3 },
};

/** Out of range is clamped rather than refused: these come back from storage,
 *  which anyone can edit, and a shrink of 5 is an exploded model rather than
 *  an error anyone wants to read. */
const clamp = (v: unknown, fallback: number) =>
  typeof v === "number" && Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : fallback;

function geomLayer(stored: Partial<GeomLayer> | undefined, fallback: GeomLayer): GeomLayer {
  return {
    visible: typeof stored?.visible === "boolean" ? stored.visible : fallback.visible,
    colouring:
      stored?.colouring === "id" || stored?.colouring === "uniform"
        ? stored.colouring
        : fallback.colouring,
    alpha: clamp(stored?.alpha, fallback.alpha),
  };
}

function layer(stored: Partial<Layer> | undefined, fallback: Layer): Layer {
  return {
    ...geomLayer(stored, fallback),
    shrink: clamp(stored?.shrink, fallback.shrink),
  };
}

/**
 * Read the stored preferences, falling back to the defaults for anything
 * missing or unreadable.
 *
 * Storage can throw outright — a private window, or a browser told to block
 * site data — and it can also hold something written by an older version, as
 * every stored copy did on the day faces and blocks arrived. Both are
 * ordinary, so neither is allowed to stop the viewer from opening.
 */
export function loadPrefs(): ViewPrefs {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return structuredClone(DEFAULTS);
    const stored = JSON.parse(raw) as Partial<ViewPrefs>;
    return {
      theme: { ...DEFAULTS.theme, ...(stored.theme ?? {}) },
      axes: typeof stored.axes === "boolean" ? stored.axes : DEFAULTS.axes,
      drawer: typeof stored.drawer === "boolean" ? stored.drawer : DEFAULTS.drawer,
      chrome: typeof stored.chrome === "boolean" ? stored.chrome : DEFAULTS.chrome,
      shot: {
        // One of the offered sizes, not whatever is in storage: a scale of
        // 40 is a request for a gigapixel the browser will refuse to make.
        scale: SHOT_SCALES.includes(stored.shot?.scale as number)
          ? (stored.shot!.scale as number)
          : DEFAULTS.shot.scale,
        transparent:
          typeof stored.shot?.transparent === "boolean"
            ? stored.shot.transparent
            : DEFAULTS.shot.transparent,
      },
      edges: {
        visible:
          typeof stored.edges?.visible === "boolean"
            ? stored.edges.visible
            : DEFAULTS.edges.visible,
        shrink: clamp(stored.edges?.shrink, DEFAULTS.edges.shrink),
      },
      faces: {
        ...layer(stored.faces, DEFAULTS.faces),
        internal:
          typeof stored.faces?.internal === "boolean"
            ? stored.faces.internal
            : DEFAULTS.faces.internal,
      },
      blocks: layer(stored.blocks, DEFAULTS.blocks),
      corners: geomLayer(stored.corners, DEFAULTS.corners),
      curves: geomLayer(stored.curves, DEFAULTS.curves),
      surfaces: geomLayer(stored.surfaces, DEFAULTS.surfaces),
    };
  } catch {
    return structuredClone(DEFAULTS);
  }
}

export function savePrefs(prefs: ViewPrefs): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(prefs));
  } catch {
    // Not being able to remember a colour is not worth interrupting anyone.
  }
}
