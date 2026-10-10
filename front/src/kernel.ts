/**
 * What the application talks to, in both modes.
 *
 * **Asynchronous on purpose, even where it need not be.** In the static mode
 * the kernel is in this very tab and every answer is immediate; in the station
 * mode it is in a Python process at the other end of a socket. Writing the
 * application against a synchronous interface would quietly bake the first case
 * into every call site, and the second could then only be added by rewriting
 * all of them.
 *
 * `onChanged` is the other half of that: in the station, the kernel can change
 * without this tab asking — someone typed in the Python console. A client that
 * only ever polls after its own commands would simply not see it.
 */

export interface Reply {
  ok: boolean;
  message: string;
  viewChanged: boolean;
}

export interface Stats {
  nodes: number;
  edges: number;
  faces: number;
  blocks: number;
}

/** One edge of a sheet, and the way round a cut parameter would run on it. */
export interface SheetEdge {
  id: number;
  from: [number, number, number];
  to: [number, number, number];
}

export type Sheet =
  | { ok: true; edges: SheetEdge[]; blocks: number }
  | { ok: false; error: string };

export type Mode = "static" | "station";

export interface Kernel {
  readonly mode: Mode;
  apply(command: Record<string, unknown>): Promise<Reply>;
  view(): Promise<Uint8Array>;
  stats(): Promise<Stats>;
  /** The sheet of hexahedra through an edge. A question, not a change. */
  sheet(edge: number): Promise<Sheet>;
  /** Register interest in changes this client did not make. */
  onChanged(listener: () => void): void;
  close(): void;
}
