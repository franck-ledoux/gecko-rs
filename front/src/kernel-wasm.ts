/**
 * Static mode: the kernel is in this tab.
 *
 * Every method resolves immediately. The promises are not pretence — they are
 * what lets the same application code run against the station.
 */

import initWasm, { initSync, Session } from "./pkg/gecko";
import type { Kernel, Reply, Sheet, Stats } from "./kernel";

let ready = false;

/** Load the wasm kernel. Pass bytes in Node (tests); let it fetch in a browser. */
export async function initKernel(source?: BufferSource): Promise<void> {
  if (ready) return;
  if (source) initSync({ module: source });
  else await initWasm();
  ready = true;
}

export class WasmKernel implements Kernel {
  readonly mode = "static" as const;
  private session: Session;

  constructor() {
    if (!ready) throw new Error("initKernel() first");
    this.session = new Session();
  }

  /**
   * The synchronous door, for Lua.
   *
   * The same session the asynchronous methods reach — this is the one
   * call underneath them, without the promise a socket would have needed.
   */
  get direct() {
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

  async apply(command: Record<string, unknown>): Promise<Reply> {
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

  async view(): Promise<Uint8Array> {
    return this.session.viewBuffer();
  }

  async geometry(): Promise<Uint8Array> {
    return this.session.geometryBuffer();
  }

  async mesh(): Promise<Uint8Array> {
    return this.session.meshBuffer();
  }

  async vtk(): Promise<string | null> {
    return this.session.meshVtk() ?? null;
  }

  async stats(): Promise<Stats> {
    return JSON.parse(this.session.statsJson()) as Stats;
  }

  async sheet(edge: number): Promise<Sheet> {
    return JSON.parse(this.session.sheetJson(edge)) as Sheet;
  }

  /** Nothing else holds this kernel, so nothing else can change it. */
  onChanged(_listener: () => void): void {}

  close(): void {}
}
