/**
 * What the application talks to: the session, in this very tab.
 *
 * **Synchronous**, because it is here. It was asynchronous for as long as
 * there was a second kernel at the end of a socket, and a promise per call
 * was the price of pretending the two were alike. There is one kernel now
 * — the page holds it, and Lua holds it, and the binary that serves the
 * page does not — so the pretence has gone with the socket.
 */

export interface Reply {
  ok: boolean;
  message: string;
  viewChanged: boolean;
  /** The line of Lua that would have made this command, for the console
   *  to echo. Absent for the commands that leave no line. */
  script?: string;
  /** A refused pillowing's way out: the faces of a pillow surface that
   *  would pass instead. Absent when there is none. */
  offer?: { face: number; block: number }[];
}

/**
 * What there is of a geometry, and **which** geometry.
 *
 * `generation` counts loads rather than describing one. A client holds the
 * number it last drew and re-fetches when it differs — which is right for a
 * load it did not make itself, such as one typed into the Python console,
 * and which two geometries of identical size would defeat if the counts were
 * what was compared.
 */
export interface GeometryStats {
  generation: number;
  /** Absent when nothing is loaded. */
  corners?: number;
  curves?: number;
  surfaces?: number;
  segments?: number;
  triangles?: number;
}

export interface Stats {
  nodes: number;
  edges: number;
  faces: number;
  blocks: number;
  /** Open sheets of faces, each one a connected component — a blocking of a
   *  surface, on its way to being blocks. Not counted among them. */
  patches: number;
  geometry: GeometryStats;
  /** How much of the blocking is pinned to the geometry. */
  classified: { nodes: number; edges: number; faces: number; frozen: number };
  /** How far back the session can go, and how far back it can go *now*.
   *  `back` is what decides whether the undo button is live. */
  history: { steps: number; back: number };
  /** The mesh, if one has been built since the last edit. `generation`
   *  counts builds **and** the edits that threw one away, so a client
   *  holding the picture learns when it has gone. */
  mesh: { generation: number; nodes?: number; hexes?: number };
}

/** One edge of a sheet, and the way round a cut parameter would run on it. */
export interface SheetEdge {
  id: number;
  from: [number, number, number];
  to: [number, number, number];
}

/** The chord through a face: what to draw, and what is at stake. */
export type Chord =
  | { ok: true; edges: SheetEdge[]; blocks: number }
  | { ok: false; error: string };

/** What one layer of blocks is made of, and the layers beside it.
 *
 *  A different question from `Sheet`, which says what a cut would touch.
 *  `beside` has two entries, in no particular order, either of which may
 *  be null where that side of the layer is the model's surface. */
export type SheetCells =
  | {
      ok: true;
      /** The edge this layer answers to — the least of its sheet's — so
       *  that two readings of the same layer agree on which it is. */
      sheet: number;
      nodes: number[];
      edges: number[];
      faces: number[];
      blocks: number[];
      beside: [number | null, number | null];
    }
  | { ok: false; error: string };

export type Sheet =
  | {
      ok: true;
      edges: SheetEdge[];
      blocks: number;
      /** How many segments it is divided into when meshed. */
      segments: number;
    }
  | { ok: false; error: string };

/**
 * The same session as JSON, which is what the interpreter speaks.
 *
 * Lua asks for the nodes in the middle of a `for` loop; the call cannot be
 * unwound into a promise, and it has no use for the typed shapes above —
 * it turns JSON into Lua tables itself.
 */
export interface Direct {
  apply(command: string): string;
  /** A read by name: `nodes`, `edges`, `faces`, `blocks`, `stats`,
   *  `entities`, `sheet`, `vtk`, `script`. */
  query(kind: string, arg: string): string;
}

/** How good a mesh is, as the kernel measures it: the worst scaled Jacobian
 *  of each cell's corners, summed up. */
export interface Quality {
  /** Which mesh this is about, as `stats().mesh.generation` counts them. */
  generation: number;
  cells: number;
  min: number;
  mean: number;
  /** The worst cell: where it is in model coordinates, and its block. */
  worst: {
    cell: number;
    block: number;
    quality: number;
    center: [number, number, number];
    /** Corner to opposite corner. */
    size: number;
  } | null;
  /** How many cells are below each threshold, keyed by the threshold. */
  below: Record<string, number>;
  /** Where each histogram bin starts; the last runs to one. */
  bins: number[];
  histogram: number[];
  /** The worst blocks, worst first. */
  blocks: { block: number; worst: number; inverted: number; cells: number }[];
}

/** A gap between the mesh and the geometry: where, how far, and how big
 *  the cells are there. */
export interface Gap {
  at: [number, number, number];
  distance: number;
  size: number;
}

/** How faithfully a mesh follows its geometry, as the kernel measures it. */
export interface Fidelity {
  /** Which mesh this is about, as `stats().mesh.generation` counts them. */
  generation: number;
  /** The tolerance, as a share of the local element size. */
  share: number;
  /** Every node of the skin, to the nearest surface. */
  skin: { nodes: number; max: number; mean: number; beyond: number };
  /** Every vertex and triangle middle of the geometry, to the skin. */
  geometry: {
    samples: number;
    max: number;
    beyond: number;
    surfaces: { tag: number; max: number; beyond: number }[];
  };
  /** The curves with mesh edges along them, and those without. */
  curves: {
    checked: number;
    followed: number;
    missed: { tag: number; max: number; beyond: number; samples: number }[];
  };
  /** The corners with a mesh node on them, and those without. */
  corners: {
    checked: number;
    missed: { tag: number; at: [number, number, number]; distance: number }[];
  };
  /** The worst gap beyond the tolerance, and what it is a gap of. */
  worst: (Gap & { what: string }) | null;
  /** What is missed, in model coordinates, to be drawn. */
  overlay: {
    segments: number[][];
    triangles: number[][];
    /** Every sample of the geometry off the mesh. */
    points: [number, number, number][];
  };
}

/** One group of blocks, as the kernel describes it. */
export interface Group {
  name: string;
  /** Which bit of a cell's group mask this one is. */
  bit: number;
  /** Written into the mesh as a variable of its own. */
  written: boolean;
  /** How many blocks are in it. */
  blocks: number;
}

/** An entity of the geometry, as the kernel lists it. */
export interface Entity {
  dim: number;
  tag: number;
  /** The merge standing in for it: its pieces, and the joins it swallowed. */
  in?: { dim: number; tag: number };
  /** For a composite: what it is made of, whether it is a merge of the
   *  geometry, and what the kernel calls it. */
  members?: number[];
  merged?: boolean;
  name?: string;
}

export interface Kernel {
  /** The same session, as JSON: the door Lua goes through. */
  readonly direct: Direct;
  apply(command: Record<string, unknown>): Reply;
  view(): Uint8Array;
  /** The nodes alone, for a command that moved one and made nothing.
   *
   *  A tenth of the bytes of a view and a twentieth of the time, at the
   *  price of being right about nodes only — so it is asked for after a
   *  move and never instead of a view that something else changed. */
  nodes(): Uint8Array;
  /** The geometry, in a buffer of its own. Asked for only when the
   *  generation in `stats` says it has changed. */
  geometry(): Uint8Array;
  /** The mesh's skin, on the same terms. Empty until something has been
   *  meshed, and empty again as soon as an edit throws it away. */
  mesh(): Uint8Array;
  /** Every face of every cell of the mesh worse than `below`, the worst
   *  first, in the mesh buffer's encoding: the cells to look at, inside the
   *  mesh as well as on its skin. */
  cellsBelow(below: number): Uint8Array;
  /** How good the mesh is, or null if there is none. */
  quality(): Quality | null;
  /** How faithfully the mesh follows the geometry, with the tolerance a
   *  share of the local element size; null without both. */
  fidelity(share: number): Fidelity | null;
  /** The mesh as a VTK legacy file, or null if there is none. */
  vtk(): string | null;
  stats(): Stats;
  /** The sheet of hexahedra through an edge. A question, not a change. */
  sheet(edge: number): Sheet;

  /** What that layer is made of, and the layers lying against it — what it
   *  takes to show one layer on its own and to step to the next. */
  sheetCells(edge: number): SheetCells;

  /** The chord of blocks through a face, named as a click names it. Also a
   *  question: nothing is removed or pillowed by asking. */
  chord(face: number, block: number): Chord;

  /** Which components are **patches** — open sheets of faces — rather than
   *  blocks. What `Del` needs to know to take the right thing away. */
  patches(): number[];

  /** The groups of blocks: what each is called, which bit of a cell's mask
   *  it is, whether it is written into the mesh, and how many blocks are
   *  in it. */
  groups(): Group[];

  /** Every entity of the geometry, with the merge that took it, if one
   *  did: what a click on one piece of a merge has to light. */
  entities(): Entity[];

  /** Which version of gecko this is: the kernel's own, so it cannot
   *  disagree with the release it was built from. */
  version(): string;
  /** The session as a Lua script: what it would take to arrive here. */
  script(): string;
}
