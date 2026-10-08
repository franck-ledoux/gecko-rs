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
import { loadPrefs, savePrefs, PRESETS, SWATCHES, type Theme } from "./theme";
import type { Axis } from "./axes";
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

/** Where to put the next cube, so successive ones stand side by side.
 *  Placement is the interface's business; the kernel takes an origin. */
function nextOrigin(blocks: number): [number, number, number] {
  return [blocks * 1.5, 0, 0];
}

async function boot() {
  const kernel = await detectMode();

  const canvas = $<HTMLCanvasElement>("view");
  const engine = new Engine(canvas, true, { preserveDrawingBuffer: true }, true);
  const viewer = new Viewer(engine);
  viewer.attachTo(canvas);

  let message = "";
  let revision = 0;

  // --- how it looks: viewer state, never the kernel's ---------------------
  const prefs = loadPrefs();
  viewer.applyTheme(prefs.theme);
  viewer.axes.setVisible(prefs.axes);

  function setTheme(theme: Theme) {
    prefs.theme = { ...theme };
    viewer.applyTheme(prefs.theme);
    savePrefs(prefs);
    drawSwatches();
  }

  /** The panel is generated from the theme's own keys, so a colour added later
   *  needs no new markup. */
  function drawSwatches() {
    $("swatches").innerHTML = SWATCHES.map(
      ({ key, label }) =>
        `<label class="swatch">${label}` +
        `<input type="color" data-key="${key}" data-test="colour-${key}"` +
        ` value="${prefs.theme[key]}" /></label>`,
    ).join("");
    for (const input of $("swatches").querySelectorAll("input")) {
      input.addEventListener("input", (e) => {
        const target = e.target as HTMLInputElement;
        setTheme({ ...prefs.theme, [target.dataset.key as keyof Theme]: target.value });
      });
    }
  }
  drawSwatches();

  $<HTMLInputElement>("show-axes").checked = prefs.axes;
  $("show-axes").addEventListener("change", (e) => {
    prefs.axes = (e.target as HTMLInputElement).checked;
    viewer.axes.setVisible(prefs.axes);
    savePrefs(prefs);
  });
  $("preset-dark").addEventListener("click", () => setTheme(PRESETS.dark));
  $("preset-light").addEventListener("click", () => setTheme(PRESETS.light));

  // Clicking the triad swings the view onto that axis. Checked before
  // anything else: it is an overlay, so it is in front.
  canvas.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    const axis = viewer.axes.pick(e.offsetX, e.offsetY);
    if (!axis) return;
    viewer.axes.lookDown(axis);
    message = `vue selon ${axis}`;
    $("status").textContent = message;
  });

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

  $("cube").addEventListener("click", async () => {
    const stats = await kernel.stats();
    const reply = await kernel.apply({
      op: "create_cube",
      origin: nextOrigin(stats.blocks),
      size: 1,
    });
    message = reply.message;
    await refresh(reply.viewChanged);
  });

  // Dropping a node is one command, sent once. The scene has been following
  // the mouse on its own copy; this is where the kernel takes it back over,
  // and the refresh below redraws from what the kernel actually holds.
  viewer.onDrop(async (name, to) => {
    const reply = await kernel.apply({ op: "move_node", node: name, to });
    message = reply.message;
    await refresh();
  });
  window.addEventListener("resize", () => {
    engine.resize();
    viewer.resize();
  });

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
    names: () => viewer.names(),
    theme: () => ({ ...prefs.theme }),
    setTheme: (t: Theme) => setTheme(t),
    axesVisible: () => viewer.axes.visible,
    /** Jump the camera onto an axis without the animation, so a test asserts
     *  where it landed rather than racing it. */
    snapTo: (axis: Axis) => viewer.axes.snapTo(axis),
    pickAxis: (x: number, y: number) => viewer.axes.pick(x, y),
    axisScreen: (axis: Axis) => viewer.axes.axisScreen(axis),
    camera: () => ({ alpha: viewer.camera.alpha, beta: viewer.camera.beta }),
    pickNode: (x: number, y: number) => viewer.pickNode(x, y),
    nodeScreen: (name: number) => viewer.nodeScreen(name),
    nodePosition: (name: number) => viewer.nodePosition(name),
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
