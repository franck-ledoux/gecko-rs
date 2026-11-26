/**
 * The kernel, in this tab.
 *
 * A thin wrapper over the wasm session: it turns JSON into the shapes the
 * application is written against, and nothing else. What it does **not**
 * do is make anything asynchronous — the session is here, and a promise
 * per call would be a story about a server that no longer exists.
 */

import initWasm, { initSync, Session } from "./pkg/gecko";
import type {
  Chord,
  Direct,
  Entity,
  Fidelity,
  Group,
  Kernel,
  Metric,
  Quality,
  Reply,
  Sheet,
  SheetCells,
  Stats,
} from "./kernel";

let ready = false;

/** Load the wasm kernel. Pass bytes in Node (tests); let it fetch in a browser. */
export async function initKernel(source?: BufferSource): Promise<void> {
  if (ready) return;
  if (source) initSync({ module: source });
  else await initWasm();
  ready = true;
}

export class WasmKernel implements Kernel {
  private session: Session;

  constructor() {
    if (!ready) throw new Error("initKernel() first");
    this.session = new Session();
  }

  /** The same session as JSON, which is what the interpreter speaks. */
  get direct(): Direct {
    return {
      apply: (command: string) => this.session.applyJson(command),
      query: (kind: string, arg: string) => {
        switch (kind) {
          case "nodes":
            return this.session.nodesJson();
          case "edges":
            return this.session.edgesJson();
          case "faces":
            return this.session.facesJson();
          case "blocks":
            return this.session.blocksJson();
          case "stats":
            return this.session.statsJson();
          case "quality":
            return this.session.qualityJson(arg);
          case "fidelity":
            return this.session.fidelityJson(arg === "" ? NaN : Number(arg));
          case "entities":
            return this.session.entitiesJson();
          case "sheet":
            return this.session.sheetJson(Number(arg));
          case "sheet_cells":
            return this.session.sheetCellsJson(Number(arg));
          case "patches":
            return this.session.patchesJson();
          case "groups":
            return this.session.groupsJson();
          case "chord": {
            // Two numbers, as a JSON array: one argument, and a chord is
            // named by a face and the block it was clicked on.
            const [face, block] = JSON.parse(arg) as [number, number];
            return this.session.chordJson(face, block);
          }
          case "vtk":
            return JSON.stringify(this.session.meshVtk() ?? null);
          case "script":
            return JSON.stringify(this.session.script());
          default:
            return "null";
        }
      },
    };
  }

  apply(command: Record<string, unknown>): Reply {
    const raw = JSON.parse(this.session.applyJson(JSON.stringify(command)));
    return raw.ok
      ? {
          ok: true,
          message: raw.outcome.message,
          viewChanged: raw.outcome.viewChanged,
          script: raw.outcome.script,
        }
      : {
          ok: false,
          message: raw.error,
          viewChanged: false,
          ...(raw.offer ? { offer: raw.offer } : {}),
        };
  }

  nodes(): Uint8Array {
    return this.session.nodesBuffer();
  }

  view(): Uint8Array {
    return this.session.viewBuffer();
  }

  geometry(): Uint8Array {
    return this.session.geometryBuffer();
  }

  cellsIn(metric: Metric, from: number, to: number): Uint8Array {
    return this.session.cellsBuffer(metric, from, to);
  }

  skinScores(metric: Metric): Float32Array {
    const bytes = this.session.skinScores(metric);
    // Read through a DataView: nothing promises the bytes start on a
    // multiple of four, which a Float32Array over them would need.
    const out = new Float32Array(bytes.length / 4);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    for (let i = 0; i < out.length; i++) out[i] = view.getFloat32(i * 4, true);
    return out;
  }

  fidelity(share: number): Fidelity | null {
    return JSON.parse(this.session.fidelityJson(share)) as Fidelity | null;
  }

  quality(metric: Metric = "scaled_jacobian"): Quality | null {
    return JSON.parse(this.session.qualityJson(metric)) as Quality | null;
  }

  mesh(): Uint8Array {
    return this.session.meshBuffer();
  }

  vtk(): string | null {
    return this.session.meshVtk() ?? null;
  }

  stats(): Stats {
    return JSON.parse(this.session.statsJson()) as Stats;
  }

  sheet(edge: number): Sheet {
    return JSON.parse(this.session.sheetJson(edge)) as Sheet;
  }

  sheetCells(edge: number): SheetCells {
    return JSON.parse(this.session.sheetCellsJson(edge)) as SheetCells;
  }

  chord(face: number, block: number): Chord {
    return JSON.parse(this.session.chordJson(face, block)) as Chord;
  }

  patches(): number[] {
    return JSON.parse(this.session.patchesJson()) as number[];
  }

  groups(): Group[] {
    return JSON.parse(this.session.groupsJson()) as Group[];
  }

  entities(): Entity[] {
    return JSON.parse(this.session.entitiesJson()) as Entity[];
  }

  version(): string {
    return this.session.version();
  }

  script(): string {
    return this.session.script();
  }
}
