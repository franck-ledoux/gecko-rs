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

export interface Kernel {
  /** The same session, as JSON: the door Lua goes through. */
  readonly direct: Direct;
  apply(command: Record<string, unknown>): Reply;
  view(): Uint8Array;
  /** The geometry, in a buffer of its own. Asked for only when the
   *  generation in `stats` says it has changed. */
  geometry(): Uint8Array;
  /** The mesh's skin, on the same terms. Empty until something has been
   *  meshed, and empty again as soon as an edit throws it away. */
  mesh(): Uint8Array;
  /** The mesh as a VTK legacy file, or null if there is none. */
  vtk(): string | null;
  stats(): Stats;
  /** The sheet of hexahedra through an edge. A question, not a change. */
  sheet(edge: number): Sheet;

  /** The chord of blocks through a face, named as a click names it. Also a
   *  question: nothing is removed or pillowed by asking. */
  chord(face: number, block: number): Chord;

  /** Which components are **patches** — open sheets of faces — rather than
   *  blocks. What `Del` needs to know to take the right thing away. */
  patches(): number[];

  /** Which version of gecko this is: the kernel's own, so it cannot
   *  disagree with the release it was built from. */
  version(): string;
  /** The session as a Lua script: what it would take to arrive here. */
  script(): string;
}
