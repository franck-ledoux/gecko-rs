/**
 * The kernel, in this tab.
 *
 * A thin wrapper over the wasm session: it turns JSON into the shapes the
 * application is written against, and nothing else. What it does **not**
 * do is make anything asynchronous — the session is here, and a promise
 * per call would be a story about a server that no longer exists.
 */

import initWasm, { initSync, Session } from "./pkg/gecko";
import type { Chord, Direct, Kernel, Reply, Sheet, Stats } from "./kernel";

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
          case "entities":
            return this.session.entitiesJson();
          case "sheet":
            return this.session.sheetJson(Number(arg));
          case "patches":
            return this.session.patchesJson();
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
      : { ok: false, message: raw.error, viewChanged: false };
  }

  view(): Uint8Array {
    return this.session.viewBuffer();
  }

  geometry(): Uint8Array {
    return this.session.geometryBuffer();
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

  chord(face: number, block: number): Chord {
    return JSON.parse(this.session.chordJson(face, block)) as Chord;
  }

  patches(): number[] {
    return JSON.parse(this.session.patchesJson()) as number[];
  }

  version(): string {
    return this.session.version();
  }

  script(): string {
    return this.session.script();
  }
}
