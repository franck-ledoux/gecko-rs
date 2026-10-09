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
import { Viewer, type Group, type Pick } from "./viewer";
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
    $("status").textContent = "waiting for the Python station…";
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
    { group: "blocks", label: "Blocks" },
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
          `${layer.visible ? " checked" : ""} /> show</label>` +
          `<div class="row">${mode("uniform", "uniform")}${mode("id", "by id")}</div>` +
          slider(`${group}-alpha`, "opacity", 0, layer.alpha) +
          slider(`${group}-shrink`, "shrink", 0.1, layer.shrink) +
          (group === "faces"
            ? `<label class="check"><input type="checkbox" data-test="faces-internal"` +
              `${prefs.faces.internal ? " checked" : ""} /> internal faces</label>`
            : "")
        );
      }).join("") +
      `<fieldset class="layer"><legend>Edges</legend>` +
      slider("edges-shrink", "shrink", 0.1, prefs.edges.shrink) +
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
    pressedAt = [e.offsetX, e.offsetY];
    const axis = viewer.axes.pick(e.offsetX, e.offsetY);
    if (!axis) return;
    pressedAt = undefined;
    viewer.axes.lookDown(axis);
    message = `looking down ${axis}`;
    $("status").textContent = message;
  });

  // --- picking faces to glue ---------------------------------------------
  /** How far the pointer may travel and still count as a click. Dragging from
   *  a face is how you orbit the view, and it must not also select it. */
  const CLICK_SLOP = 4;
  let pressedAt: [number, number] | undefined;

  canvas.addEventListener("pointerup", (e) => {
    if (e.button !== 0 || !pressedAt) return;
    const [x, y] = pressedAt;
    pressedAt = undefined;
    if (Math.hypot(e.offsetX - x, e.offsetY - y) > CLICK_SLOP) return;
    // A node under the cursor is a drag that is about to start, or has just
    // ended — not a face being chosen. The node wins: it is the smaller
    // target, and the one you had to aim at.
    if (viewer.pickNode(x, y) !== null) return;
    choose(viewer.pickSurface(x, y));
  });

  /**
   * Add, remove, or clear.
   *
   * Two is the whole selection, so a third pick starts again rather than
   * being ignored: being stuck with two wrong faces and no way to say so is
   * worse than losing a click.
   */
  function choose(pick: Pick | null) {
    let picked = viewer.selection;
    if (pick === null) picked = [];
    else if (picked.some((p) => p.face === pick.face))
      picked = picked.filter((p) => p.face !== pick.face);
    else if (picked.length >= 2) picked = [pick];
    else picked = [...picked, pick];

    viewer.select(picked);
    $<HTMLButtonElement>("glue").disabled = picked.length !== 2;
    $<HTMLButtonElement>("remove").disabled = picked.length === 0;
    // The block is named as well as the face: it is what the remove button
    // would act on, and a click that deletes something unnamed is no good.
    message =
      picked.length === 0
        ? ""
        : picked.length === 1
          ? `face ${picked[0].face} (block ${picked[0].block}) — the next one will come to it`
          : `face ${picked[1].face} will be glued onto face ${picked[0].face}`;
    $("status").textContent = message;
  }

  $("glue").addEventListener("click", async () => {
    const [first, second] = viewer.selection;
    if (first === undefined || second === undefined) return;
    const reply = await kernel.apply({
      op: "glue_faces",
      first: first.face,
      second: second.face,
    });
    // Whatever happened, those two names mean something else now — or
    // nothing at all. Keeping them selected would leave the panel offering to
    // glue a face that no longer exists. Clearing resets the message, so the
    // outcome is said after.
    choose(null);
    message = reply.message;
    // Deliberately not framed. `viewChanged` says the bytes are stale, which
    // is a different question from whether the camera should move — and
    // welding two blocks you are already looking at is no reason to move it.
    await refresh();
  });

  // The block the first pick landed on. Not the face's block in general —
  // once two blocks are glued a face has one on each side — but the pick
  // knows which side it hit.
  $("remove").addEventListener("click", async () => {
    const [first] = viewer.selection;
    if (first === undefined) return;
    const reply = await kernel.apply({ op: "remove_block", block: first.block });
    choose(null);
    message = reply.message;
    // Not framed: taking a block away is no reason to move the camera either.
    await refresh();
  });

  /** Put everything back in view. Camera state, so it never goes near the
   *  kernel: in station mode two tabs on one session each have their own. */
  $("recentre").addEventListener("click", () => {
    viewer.frameAll();

    message = "view recentred";
    $("status").textContent = message;
  });

  /** `frame` is how the camera should get there, not whether the bytes are
   *  stale — they always are by the time this is called. */
  async function refresh(frame: false | "jump" | "glide" = false) {
    const [bytes, stats] = await Promise.all([kernel.view(), kernel.stats()]);
    viewer.setView(decodeView(bytes));
    if (frame) viewer.frameAll(frame === "jump");
    $("stats").innerHTML = `
      <dl>
        <dt>mode</dt><dd data-test="mode">${kernel.mode}</dd>
        <dt>blocks</dt><dd data-test="blocks">${stats.blocks}</dd>
        <dt>nodes</dt><dd data-test="nodes">${stats.nodes}</dd>
        <dt>edges</dt><dd data-test="edges">${stats.edges}</dd>
        <dt>faces</dt><dd data-test="faces">${stats.faces}</dd>
      </dl>`;
    $("status").textContent = message;
    revision++;
  }

  // In the station the kernel moves without this tab asking: someone typed in
  // the Python console. This is the whole point of the mode.
  kernel.onChanged(() => {
    message = "changed from Python";
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
    // Framed: a new cube is put down beside the others, and a model that
    // grows past the edge of the window is no use. Gliding, so the view does
    // not lurch every time you press the button.
    await refresh("glide");
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
  // Straight there: there is nothing on screen yet to glide away from.
  await refresh("jump");

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
      return (
        s && {
          ...s,
          positions: Array.from(s.positions),
          faces: Array.from(s.faces),
          blocks: Array.from(s.blocks),
        }
      );
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
    pickSurface: (x: number, y: number) => viewer.pickSurface(x, y),
    faceScreen: (name: number) => viewer.faceScreen(name),
    selection: () => viewer.selection,
    choose: (pick: Pick | null) => choose(pick),
    nodeScreen: (name: number) => viewer.nodeScreen(name),
    nodePosition: (name: number) => viewer.nodePosition(name),
    centre: () => viewer.centre,
    apply: (command: Record<string, unknown>) => kernel.apply(command),
    refresh: () => refresh(),
    recentre: () => viewer.frameAll(),
    /** Stop the camera drifting. Babylon keeps applying inertia for a while
     *  after a gesture, so a test that wants to show nothing *else* moved the
     *  camera has to be able to say that nothing was still moving it. */
    stopCamera: () => {
      viewer.camera.inertialAlphaOffset = 0;
      viewer.camera.inertialBetaOffset = 0;
      viewer.camera.inertialRadiusOffset = 0;
    },
    /**
     * Resolves once the camera has stopped and the scene has drawn twice
     * more.
     *
     * Framing glides, so without the first half a test that aims a mouse
     * straight after one aims at where the model used to be — and is right
     * often enough to look like it works.
     */
    settled: () =>
      new Promise<void>((resolve) => {
        const tick = () => {
          if (viewer.cameraMoving) {
            requestAnimationFrame(tick);
            return;
          }
          requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
        };
        tick();
      }),
  };
  document.body.dataset.ready = "1";
}

boot().catch((e) => {
  document.body.dataset.error = String(e);
  const status = document.getElementById("status");
  if (status) status.textContent = String(e);
  console.error(e);
});
