/**
 * Boot, mode detection, and the test surface.
 *
 * The only thing that differs between the two modes is which `Kernel` gets
 * built here. Everything below this line is written against the interface and
 * has no idea where the kernel is.
 */

import { Engine } from "@babylonjs/core/Engines/engine";
import { decodeView } from "./decode";
import { readProbe, type Probe } from "./mode";
import type { Kernel } from "./kernel";
import { initKernel, WasmKernel } from "./kernel-wasm";
import { RemoteKernel } from "./kernel-remote";
import { Viewer, type Group } from "./viewer";
import { loadPrefs, savePrefs, PRESETS, SWATCHES, type Theme } from "./theme";
import type { Colouring } from "./surface";
import type { Axis } from "./axes";
import "./style.css";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const PROBE_PAUSE = 500;

/**
 * Ask whether a station is serving this page, and keep asking while the answer
 * is "not yet".
 *
 * Never falls back to a local kernel once a station has been promised: a page
 * that is wired to Python and silently is not is worse than a page that says
 * it is waiting.
 */
async function detectMode(waiting: () => void): Promise<Kernel> {
  for (;;) {
    let answer: Probe;
    try {
      const response = await fetch("mode", { cache: "no-store" });
      answer = readProbe(response.status, await response.text());
    } catch {
      answer = readProbe(0, "");
    }

    if (answer === "static") {
      await initKernel();
      return new WasmKernel();
    }
    if (answer === "station") {
      const scheme = location.protocol === "https:" ? "wss" : "ws";
      try {
        return await RemoteKernel.connect(`${scheme}://${location.host}/ws`);
      } catch {
        // The probe was answered and the socket was not: the station went down
        // between the two. Wait for it like any other absent station.
      }
    }
    waiting();
    await new Promise((r) => setTimeout(r, PROBE_PAUSE));
  }
}

/** Where to put the next cube, so successive ones stand side by side.
 *  Placement is the interface's business; the kernel takes an origin. */
function nextOrigin(blocks: number): [number, number, number] {
  return [blocks * 1.5, 0, 0];
}

async function boot() {
  // Said out loud, and marked on the body so a test can wait for it: this is
  // the state the page used to hide by pretending to be a static build.
  const kernel = await detectMode(() => {
    document.body.dataset.waiting = "1";
    $("status").textContent = "en attente de la station Python…";
  });
  delete document.body.dataset.waiting;

  const canvas = $<HTMLCanvasElement>("view");
  const engine = new Engine(canvas, true, { preserveDrawingBuffer: true }, true);
  const viewer = new Viewer(engine);
  viewer.attachTo(canvas);

  let message = "";
  let revision = 0;

  // --- how it looks: viewer state, never the kernel's ---------------------
  const prefs = loadPrefs();
  viewer.applyPrefs(prefs);
  viewer.axes.setVisible(prefs.axes);

  /** One path for every control: change `prefs`, hand the lot to the viewer,
   *  remember it. Nothing reaches the scene any other way. */
  function redraw() {
    viewer.applyPrefs(prefs);
    savePrefs(prefs);
  }

  function setTheme(theme: Theme) {
    prefs.theme = { ...theme };
    redraw();
    drawSwatches();
    drawLayers();
  }

  /**
   * The faces and blocks controls, generated from this list.
   *
   * Both groups are drawn from the same quadrangles, so they take the same
   * four controls — shown, coloured how, how opaque, how shrunk — and only
   * faces have anything extra to say.
   */
  const LAYERS: ReadonlyArray<{ group: Group; label: string }> = [
    { group: "faces", label: "Faces" },
    { group: "blocks", label: "Blocs" },
  ];

  const slider = (test: string, label: string, min: number, value: number) =>
    `<label class="slider"><span>${label}</span>` +
    `<input type="range" min="${min}" max="1" step="0.01" value="${value}"` +
    ` data-test="${test}" /></label>`;

  function drawLayers() {
    $("layers").innerHTML =
      LAYERS.map(({ group, label }) => {
        const layer = prefs[group];
        const mode = (value: Colouring, text: string) =>
          `<label class="mode"><input type="radio" name="${group}-mode" value="${value}"` +
          ` data-test="${group}-${value}"${layer.colouring === value ? " checked" : ""} />` +
          `${text}</label>`;
        return (
          `<fieldset class="layer"><legend>${label}</legend>` +
          `<label class="check"><input type="checkbox" data-test="${group}-visible"` +
          `${layer.visible ? " checked" : ""} /> afficher</label>` +
          `<div class="row">${mode("uniform", "uniforme")}${mode("id", "par id")}</div>` +
          slider(`${group}-alpha`, "opacité", 0, layer.alpha) +
          slider(`${group}-shrink`, "rétraction", 0.1, layer.shrink) +
          (group === "faces"
            ? `<label class="check"><input type="checkbox" data-test="faces-internal"` +
              `${prefs.faces.internal ? " checked" : ""} /> faces internes</label>`
            : "")
        );
      }).join("") +
      `<fieldset class="layer"><legend>Arêtes</legend>` +
      slider("edges-shrink", "rétraction", 0.1, prefs.edges.shrink) +
      `</fieldset>`;

    for (const { group } of LAYERS) {
      const find = (test: string) =>
        $("layers").querySelector<HTMLInputElement>(`[data-test="${test}"]`)!;
      find(`${group}-visible`).addEventListener("change", (e) => {
        prefs[group].visible = (e.target as HTMLInputElement).checked;
        redraw();
      });
      for (const value of ["uniform", "id"] as Colouring[]) {
        find(`${group}-${value}`).addEventListener("change", () => {
          prefs[group].colouring = value;
          redraw();
        });
      }
      find(`${group}-alpha`).addEventListener("input", (e) => {
        prefs[group].alpha = Number((e.target as HTMLInputElement).value);
        redraw();
      });
      find(`${group}-shrink`).addEventListener("input", (e) => {
        prefs[group].shrink = Number((e.target as HTMLInputElement).value);
        redraw();
      });
    }
    const internal = $("layers").querySelector<HTMLInputElement>('[data-test="faces-internal"]')!;
    internal.addEventListener("change", (e) => {
      prefs.faces.internal = (e.target as HTMLInputElement).checked;
      redraw();
    });
    const edges = $("layers").querySelector<HTMLInputElement>('[data-test="edges-shrink"]')!;
    edges.addEventListener("input", (e) => {
      prefs.edges.shrink = Number((e.target as HTMLInputElement).value);
      redraw();
    });
  }
  drawLayers();

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
    display: () => viewer.display,
    surface: (group: Group) => {
      const s = viewer.surface(group);
      // Typed arrays do not survive the bridge out of the page; a test wants
      // the numbers, not the buffers.
      return s && { ...s, positions: Array.from(s.positions), names: Array.from(s.names) };
    },
    axesVisible: () => viewer.axes.visible,
    /** Jump the camera onto an axis without the animation, so a test asserts
     *  where it landed rather than racing it. */
    snapTo: (axis: Axis) => viewer.axes.snapTo(axis),
    pickAxis: (x: number, y: number) => viewer.axes.pick(x, y),
    axisScreen: (axis: Axis) => viewer.axes.axisScreen(axis),
    camera: () => ({
      alpha: viewer.camera.alpha,
      beta: viewer.camera.beta,
      radius: viewer.camera.radius,
      closest: viewer.camera.lowerRadiusLimit,
    }),
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
