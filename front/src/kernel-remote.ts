/**
 * Station mode: the kernel is in a Python process, at the other end of a
 * socket.
 *
 * One connection carries everything. Text frames are JSON and carry a request
 * id; binary frames are a view buffer with that id in their first four bytes,
 * which is what lets a reply be matched to its question without a second
 * channel to keep in sync.
 *
 * Frames with no id are pushes: the kernel changed because someone typed in the
 * Python console, and this tab is being told so.
 */

import type { Kernel, Reply, Sheet, Stats } from "./kernel";

type Pending = {
  resolve: (value: unknown) => void;
  reject: (reason: Error) => void;
};

export class RemoteKernel implements Kernel {
  readonly mode = "station" as const;
  private pending = new Map<number, Pending>();
  private listeners: Array<() => void> = [];
  private nextId = 1;

  private constructor(private socket: WebSocket) {
    socket.binaryType = "arraybuffer";
    socket.onmessage = (event) => this.receive(event.data);
    socket.onclose = () => {
      const closed = new Error("the station closed the connection");
      for (const p of this.pending.values()) p.reject(closed);
      this.pending.clear();
    };
  }

  static connect(url: string, timeoutMs = 5000): Promise<RemoteKernel> {
    return new Promise((resolve, reject) => {
      const socket = new WebSocket(url);
      const timer = setTimeout(() => {
        socket.close();
        reject(new Error(`no station answered at ${url}`));
      }, timeoutMs);
      socket.onopen = () => {
        clearTimeout(timer);
        resolve(new RemoteKernel(socket));
      };
      socket.onerror = () => {
        clearTimeout(timer);
        reject(new Error(`could not reach the station at ${url}`));
      };
    });
  }

  private receive(data: unknown) {
    if (data instanceof ArrayBuffer) {
      const id = new DataView(data).getUint32(0, true);
      this.settle(id, new Uint8Array(data, 4));
      return;
    }
    const message = JSON.parse(data as string);
    if (message.id === undefined) {
      // A push: something other than this tab moved the kernel.
      if (message.kind === "changed") this.listeners.forEach((l) => l());
      return;
    }
    this.settle(message.id, message);
  }

  private settle(id: number, value: unknown) {
    const waiting = this.pending.get(id);
    if (!waiting) return;
    this.pending.delete(id);
    waiting.resolve(value);
  }

  private ask<T>(request: Record<string, unknown>): Promise<T> {
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as Pending["resolve"], reject });
      this.socket.send(JSON.stringify({ id, ...request }));
    });
  }

  async apply(command: Record<string, unknown>): Promise<Reply> {
    const raw = await this.ask<{ ok: boolean; outcome?: Reply; error?: string }>({
      kind: "apply",
      command,
    });
    return raw.ok
      ? { ok: true, message: raw.outcome!.message, viewChanged: raw.outcome!.viewChanged }
      : { ok: false, message: raw.error ?? "unknown error", viewChanged: false };
  }

  view(): Promise<Uint8Array> {
    return this.ask<Uint8Array>({ kind: "view" });
  }

  async stats(): Promise<Stats> {
    const raw = await this.ask<{ stats: Stats }>({ kind: "stats" });
    return raw.stats;
  }

  sheet(edge: number): Promise<Sheet> {
    return this.ask<Sheet>({ kind: "sheet", edge });
  }

  onChanged(listener: () => void): void {
    this.listeners.push(listener);
  }

  close(): void {
    this.socket.close();
  }
}
