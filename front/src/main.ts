/**
 * Boot, mode detection, and the test surface.
 *
 * The only thing that differs between the two modes is which `Kernel` gets
 * built here. Everything below this line is written against the interface and
 * has no idea where the kernel is.
 */

import { Engine } from "@babylonjs/core/Engines/engine";
import { decodeView } from "./decode";
import type { Kernel } from "./kernel";
import { initKernel, WasmKernel } from "./kernel-wasm";
import { RemoteKernel } from "./kernel-remote";
import { Viewer } from "./viewer";
import "./style.css";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

/**
 * Ask whether a station is serving this page.
 *
 * A static host that falls back to `index.html` for unknown paths would make a
 * plain "did it 404?" test say yes, so the answer is checked for its content.
 */
async function detectMode(): Promise<Kernel> {
  try {
    const response = await fetch("mode");
    if (response.ok && (await response.text()).trim() === "station") {
      const scheme = location.protocol === "https:" ? "wss" : "ws";
      return await RemoteKernel.connect(`${scheme}://${location.host}/ws`);
    }
  } catch {
    // No station. The static build carries its own kernel.
  }
  await initKernel();
  return new WasmKernel();
}

/** A unit cube, shifted so successive blocks sit side by side. */
function cube(index: number): number[][] {
  const dx = index * 1.5;
  return [
    [dx, 0, 0],
    [dx + 1, 0, 0],
    [dx + 1, 1, 0],
    [dx, 1, 0],
    [dx, 0, 1],
    [dx + 1, 0, 1],
    [dx + 1, 1, 1],
    [dx, 1, 1],
  ];
}

async function boot() {
  const kernel = await detectMode();

  const canvas = $<HTMLCanvasElement>("view");
  const engine = new Engine(canvas, true, { preserveDrawingBuffer: true }, true);
  const viewer = new Viewer(engine);
  viewer.attachTo(canvas);

  let message = "";
  let revision = 0;

  async function refresh(frame = false) {
    const [bytes, stats] = await Promise.all([kernel.view(), kernel.stats()]);
    viewer.setView(decodeView(bytes));
    if (frame) viewer.frameAll();
    $("stats").innerHTML = `
      <dl>
        <dt>mode</dt><dd data-test="mode">${kernel.mode}</dd>
        <dt>blocs</dt><dd data-test="blocks">${stats.blocks}</dd>
        <dt>nœuds</dt><dd data-test="nodes">${stats.nodes}</dd>
        <dt>arêtes</dt><dd data-test="edges">${stats.edges}</dd>
        <dt>faces</dt><dd data-test="faces">${stats.faces}</dd>
      </dl>`;
    $("status").textContent = message;
    revision++;
  }

  // In the station the kernel moves without this tab asking: someone typed in
  // the Python console. This is the whole point of the mode.
  kernel.onChanged(() => {
    message = "modifié depuis Python";
    void refresh();
  });

  $("add").addEventListener("click", async () => {
    const stats = await kernel.stats();
    const reply = await kernel.apply({ op: "add_block", corners: cube(stats.blocks) });
    message = reply.message;
    if (reply.viewChanged) await refresh(true);
    else await refresh();
  });
  window.addEventListener("resize", () => engine.resize());

  engine.runRenderLoop(() => viewer.render());
  await refresh(true);

  // --- test surface -----------------------------------------------------
  // State, not pixels: everything an end-to-end test needs, with no reliance
  // on GPU output or on the DOM's current shape.
  (window as unknown as Record<string, unknown>).__gecko = {
    mode: () => kernel.mode,
    stats: () => kernel.stats(),
    message: () => message,
    revision: () => revision,
    counts: () => ({ nodes: viewer.nodeCount, edges: viewer.edgeCount }),
    centre: () => viewer.centre,
    apply: (command: Record<string, unknown>) => kernel.apply(command),
    refresh: () => refresh(),
    /** Resolves once the scene has drawn twice more. */
    settled: () =>
      new Promise<void>((r) =>
        requestAnimationFrame(() => requestAnimationFrame(() => r())),
      ),
  };
  document.body.dataset.ready = "1";
}

boot().catch((e) => {
  document.body.dataset.error = String(e);
  const status = document.getElementById("status");
  if (status) status.textContent = String(e);
  console.error(e);
});
