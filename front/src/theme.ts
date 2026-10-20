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
  /** The mesh's skin. A family of its own: it is not the blocking and not
   *  the geometry, it is what the two of them were for. */
  mesh: string;
  /** What a cell of the **blocking** wears to say what it follows, in
   *  `class` colouring. A family of its own rather than the colour of the
   *  entity itself: a block face painted the same as the surface it
   *  follows, and drawn on top of that surface, is two things nobody can
   *  tell apart. */
  onCorner: string;
  onCurve: string;
  onSurface: string;
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
  { key: "mesh", label: "mesh" },
  { key: "onCorner", label: "on a corner" },
  { key: "onCurve", label: "on a curve" },
  { key: "onSurface", label: "on a surface" },
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
    mesh: "#4f8f7a",
    onCorner: "#d58cff",
    onCurve: "#a66ff0",
    onSurface: "#6b4fa8",
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
    mesh: "#8fc7b4",
    onCorner: "#8e3fd4",
    onCurve: "#6f3fd4",
    onSurface: "#b9a8e0",
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

/** How many steps of undo to ask the kernel for, and the most the drawer
 *  offers. Each step is a copy of the whole block structure, which is why
 *  there is a number here at all rather than an unbounded stack. */
export const HISTORY_STEPS = 5;
export const MAX_HISTORY = 20;

/** How much room a bounding box leaves round the geometry, as a share of
 *  each half-side, and the most the drawer offers. The kernel's own default
 *  is the same 10%. */
export const BOX_MARGIN = 0.1;
export const MAX_BOX_MARGIN = 0.5;

/** How near a node has to come to a geometric entity to be taken by it, as
 *  a share of the geometry's own size. Used for `classify` and for a node
 *  dropped by hand, which are the same question asked twice. */
export const SNAP_SHARE = 0.02;
export const MAX_SNAP = 0.1;

/** How many segments an edge is divided into when its sheet has not been
 *  set, and the most the drawer offers. The kernel's own default is the
 *  same ten; this is what the slider sends. */
export const SEGMENTS = 10;
export const MAX_SEGMENTS = 60;

/** What a smoothing pass does, and the most the drawer offers: how many
 *  passes, and how far along the way to the middle of its neighbours a
 *  node goes in each. Half, not all: all of it is what makes a Laplacian
 *  swing from one side of a crease to the other and never settle. The
 *  kernel's own defaults are the same two numbers. */
export const SMOOTH_ROUNDS = 5;
export const MAX_SMOOTH_ROUNDS = 30;
export const SMOOTH_SHARE = 0.5;

export interface ViewPrefs {
  theme: Theme;
  axes: boolean;
  /** The two panels, and whether the interface is on screen at all. Only
   *  one panel is ever open: they sit in the same place. */
  drawer: boolean;
  params: boolean;
  chrome: boolean;
  /** Which parts of the display drawer are unfolded, by element id. A list
   *  rather than a flag each, so a section added later needs no migration:
   *  an id nobody recognises is simply not there. */
  panels: string[];
  /** Whether the window is split into four views. */
  quad: boolean;
  /** How a saved picture comes out. */
  shot: { scale: number; transparent: boolean };
  /** How many steps back the kernel is asked to keep. Kept here with the
   *  rest of what a viewer prefers, and sent to the kernel on boot: the
   *  kernel holds the states, so it is the kernel that has to be told. */
  history: number;
  /** What the next bounding box leaves round the geometry, and how near a
   *  node has to come to be taken by an entity.
   *
   *  Here rather than in the kernel because they are not state: each one is
   *  an argument to the command it belongs to, and the kernel's constant is
   *  the default for anyone who does not send one. */
  box: { margin: number; snap: number };
  /** How many segments a sheet nobody has set is meshed into. */
  segments: number;
  /** What a smoothing pass does: how many times over, and how far along
   *  the way to the middle of its neighbours a node goes each time. */
  smooth: { rounds: number; share: number };
  /** Nodes are always drawn — they are what you grab — so all they carry
   *  is how they are coloured. */
  nodes: { colouring: Colouring };
  /** Edges carry that, whether they are drawn at all, and how far they
   *  are pulled off the surfaces they would otherwise lie exactly on. */
  edges: { visible: boolean; shrink: number; colouring: Colouring };
  faces: Layer & { internal: boolean };
  blocks: Layer;
  /** The geometry, which is not edited and so has no notion of a selection
   *  or a shrink — only whether you are looking at it and how. */
  corners: GeomLayer;
  curves: GeomLayer;
  surfaces: GeomLayer;
  /** The mesh's skin, on the same three terms. Nothing is picked on it and
   *  nothing is exploded, so it takes no more than the geometry does. */
  mesh: GeomLayer;
}

export const DEFAULTS: ViewPrefs = {
  theme: { ...PRESETS.dark },
  axes: true,
  // Shut, because the whole point of a drawer is that the window belongs to
  // the model. The toolbar carries the verbs; this carries the settings.
  drawer: false,
  params: false,
  chrome: true,
  // Cells and geometry unfolded, colours and picture put away: the first
  // two are what you reach for while working, the other two are set once.
  panels: ["sec-cells", "sec-geometry"],
  // One view. The split is for placing corners, which is a thing you do
  // for a while and then stop doing.
  quad: false,
  // Twice the canvas, which is what a figure in a paper wants. The
  // background is kept by default: transparency is for dropping a figure
  // onto a page of unknown colour, and surprises anyone not expecting it.
  shot: { scale: 2, transparent: false },
  history: HISTORY_STEPS,
  box: { margin: BOX_MARGIN, snap: SNAP_SHARE },
  segments: SEGMENTS,
  smooth: { rounds: SMOOTH_ROUNDS, share: SMOOTH_SHARE },
  // Both plain to begin with. Colouring by what a cell follows is a
  // question you ask once the blocking is on the model, not while you are
  // still putting it there.
  nodes: { colouring: "uniform" },
  edges: { visible: true, shrink: 1, colouring: "uniform" },
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
  // Shown and solid: a mesh you asked for is a mesh you want to look at,
  // and what you are looking for in it — whether the elements are any good
  // — is not visible through three other layers.
  mesh: { visible: true, colouring: "uniform", alpha: 1 },
};

/** Out of range is clamped rather than refused: these come back from storage,
 *  which anyone can edit, and a shrink of 5 is an exploded model rather than
 *  an error anyone wants to read. */
const clamp = (v: unknown, fallback: number) =>
  typeof v === "number" && Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : fallback;

/** Within 0 and a ceiling of its own, which `clamp` cannot do: these are
 *  shares of something rather than fractions of one. */
const within = (v: unknown, most: number, fallback: number) =>
  typeof v === "number" && Number.isFinite(v) ? Math.min(most, Math.max(0, v)) : fallback;

/** One of the three, or the fallback: storage is a thing anyone can edit,
 *  and a colouring nobody has heard of is not an error worth reporting. */
function colouring(stored: unknown, fallback: Colouring): Colouring {
  return stored === "id" || stored === "uniform" || stored === "class"
    ? stored
    : fallback;
}

function geomLayer(stored: Partial<GeomLayer> | undefined, fallback: GeomLayer): GeomLayer {
  return {
    visible: typeof stored?.visible === "boolean" ? stored.visible : fallback.visible,
    colouring: colouring(stored?.colouring, fallback.colouring),
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
      params: typeof stored.params === "boolean" ? stored.params : DEFAULTS.params,
      panels: Array.isArray(stored.panels)
        ? stored.panels.filter((id): id is string => typeof id === "string")
        : DEFAULTS.panels,
      chrome: typeof stored.chrome === "boolean" ? stored.chrome : DEFAULTS.chrome,
      quad: typeof stored.quad === "boolean" ? stored.quad : DEFAULTS.quad,
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
      // Whole steps, within the range the drawer can show: this comes back
      // from storage, which anyone can edit, and a thousand steps would be
      // a thousand copies asked for by a typo.
      history:
        typeof stored.history === "number" && Number.isFinite(stored.history)
          ? Math.min(MAX_HISTORY, Math.max(0, Math.round(stored.history)))
          : DEFAULTS.history,
      box: {
        margin: within(stored.box?.margin, MAX_BOX_MARGIN, DEFAULTS.box.margin),
        snap: within(stored.box?.snap, MAX_SNAP, DEFAULTS.box.snap),
      },
      segments:
        typeof stored.segments === "number" && Number.isFinite(stored.segments)
          ? Math.min(MAX_SEGMENTS, Math.max(1, Math.round(stored.segments)))
          : DEFAULTS.segments,
      smooth: {
        rounds:
          typeof stored.smooth?.rounds === "number" && Number.isFinite(stored.smooth.rounds)
            ? Math.min(MAX_SMOOTH_ROUNDS, Math.max(1, Math.round(stored.smooth.rounds)))
            : DEFAULTS.smooth.rounds,
        share: within(stored.smooth?.share, 1, DEFAULTS.smooth.share),
      },
      nodes: { colouring: colouring(stored.nodes?.colouring, DEFAULTS.nodes.colouring) },
      edges: {
        visible:
          typeof stored.edges?.visible === "boolean"
            ? stored.edges.visible
            : DEFAULTS.edges.visible,
        shrink: clamp(stored.edges?.shrink, DEFAULTS.edges.shrink),
        colouring: colouring(stored.edges?.colouring, DEFAULTS.edges.colouring),
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
      mesh: geomLayer(stored.mesh, DEFAULTS.mesh),
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
