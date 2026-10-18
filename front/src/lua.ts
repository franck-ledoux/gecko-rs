/**
 * The Lua interpreter, and what it is allowed to reach.
 *
 * Lua runs in a wasm module of its own, beside the kernel's: mlua builds
 * for emscripten and the kernel is built by wasm-bindgen, and the two
 * cannot be linked together. So this file is the wire between them — the
 * four primitives the interpreter imports, answered here.
 *
 * **Synchronous, and it has to be.** A script says `for _, n in
 * ipairs(gecko.nodes())`, and the loop cannot be unwound into promises
 * halfway through. That is why the console asks the kernel for its direct
 * door rather than the asynchronous one every other caller uses.
 */

import createLua from "./lua/gecko-lua-wasm.js";
import type { Direct } from "./kernel";

/** What a chunk did: what it printed, what it came to, or what went wrong. */
export interface Answer {
  ok: boolean;
  /** Everything `print` wrote while it ran. */
  out: string;
  /** The value the chunk came to, if it came to one. */
  value?: string;
  error?: string;
}

/** Files, as the page can offer them. */
export interface Files {
  /** The contents, or null when there is no such file — or no files at
   *  all, which is the case for a page nobody served from a machine. */
  read(path: string): string | null;
  /** Somewhere to put one. Returns an error, or nothing. */
  write(path: string, text: string): string;
  /** What can be opened by name: the files this page was handed. */
  kept(): string[];
}

export class Interpreter {
  private constructor(
    private readonly module: {
      ccall: (f: string, r: string, a: string[], v: unknown[]) => string;
    },
  ) {}

  /**
   * Start one, and give it the session.
   *
   * One interpreter for the life of the page: what a line defines the next
   * line can use, which is what makes it a console rather than a series of
   * unrelated scripts.
   */
  static async open(kernel: Direct, files: Files): Promise<Interpreter> {
    const module = await createLua({
      host: {
        apply: (command: string) => kernel.apply(command),
        query: (kind: string, arg: string) => kernel.query(kind, arg),
        read: (path: string) => files.read(path),
        files: () => files.kept(),
        write: (path: string, text: string) => files.write(path, text),
      },
    });
    return new Interpreter(module as never);
  }

  /** Run a chunk. Never throws: what went wrong is part of the answer. */
  run(source: string): Answer {
    try {
      const raw = this.module.ccall("lua_run", "string", ["string"], [source]);
      return JSON.parse(raw) as Answer;
    } catch (e) {
      return { ok: false, out: "", error: String(e) };
    }
  }
}
