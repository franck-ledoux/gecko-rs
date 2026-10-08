/**
 * Starting a station from a test, and talking to it.
 *
 * Shared by the station spec and the dev-loop spec: both need a real Python
 * process holding a real session, because that is the thing under test.
 */

import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

const PYTHON = fileURLToPath(new URL("../../../.venv/bin/python", import.meta.url));
const DRIVER = fileURLToPath(new URL("../station.py", import.meta.url));

/** A station, and a line-by-line conversation with it. */
export class Station {
  private lines: string[] = [];
  private waiting: Array<(line: string) => void> = [];

  private constructor(
    readonly process: ChildProcessWithoutNullStreams,
    readonly url: string,
  ) {}

  /** `port` pins the station, which the dev server has to be told in
   *  advance so it can forward to it. Left out, the system picks one. */
  static async start(port?: number): Promise<Station> {
    const child = spawn(PYTHON, [DRIVER], {
      stdio: ["pipe", "pipe", "pipe"],
      env: { ...process.env, ...(port ? { GECKO_PORT: String(port) } : {}) },
    });
    let stderr = "";
    child.stderr.on("data", (d) => (stderr += d));
    const station = new Station(child, await Station.firstLine(child, () => stderr));
    createInterface({ input: child.stdout }).on("line", (line) => station.receive(line));
    return station;
  }

  private static firstLine(
    child: ChildProcessWithoutNullStreams,
    stderr: () => string,
  ): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = createInterface({ input: child.stdout });
      reader.once("line", (line) => {
        reader.close();
        resolve(line.trim());
      });
      child.once("exit", (code) =>
        reject(new Error(`the station exited with ${code}: ${stderr()}`)),
      );
    });
  }

  private receive(line: string) {
    const next = this.waiting.shift();
    if (next) next(line);
    else this.lines.push(line);
  }

  async ask<T>(request: string): Promise<T> {
    const reply = new Promise<string>((resolve) => {
      const buffered = this.lines.shift();
      if (buffered) resolve(buffered);
      else this.waiting.push(resolve);
    });
    this.process.stdin.write(`${request}\n`);
    return JSON.parse(await reply) as T;
  }

  stop() {
    this.process.stdin.end("quit\n");
    this.process.kill();
  }
}
