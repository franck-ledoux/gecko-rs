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
import {
  isLight,
  loadPrefs,
  savePrefs,
  PRESETS,
  SHOT_SCALES,
  SWATCHES,
  type Theme,
} from "./theme";
import { actionFor, helpRows, type Action } from "./shortcuts";
import { capture, save, shotName } from "./capture";
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
    applyChrome();
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

  // --- how a saved picture comes out --------------------------------------
  const scale = $<HTMLSelectElement>("shot-scale");
  scale.value = String(
    SHOT_SCALES.includes(prefs.shot.scale) ? prefs.shot.scale : SHOT_SCALES[1],
  );
  scale.addEventListener("change", () => {
    prefs.shot.scale = Number(scale.value);
    savePrefs(prefs);
  });
  const transparent = $<HTMLInputElement>("shot-transparent");
  transparent.checked = prefs.shot.transparent;
  transparent.addEventListener("change", () => {
    prefs.shot.transparent = transparent.checked;
    savePrefs(prefs);
  });

  // Clicking the triad swings the view onto that axis. Checked before
  // anything else: it is an overlay, so it is in front.
  canvas.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    pressedAt = [e.offsetX, e.offsetY];
    const axis = viewer.axes.pick(e.offsetX, e.offsetY);
    if (!axis) return;
    pressedAt = undefined;
    viewer.axes.lookDown(axis);
    say(`looking down ${axis}`);
  });

  // Pressing an edge begins a cut: the sheet lights up, and the pointer is
  // taken off the camera so that dragging sets the parameter rather than
  // turning the view.
  canvas.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || aiming !== null) return;
    if (viewer.axes.pick(e.offsetX, e.offsetY)) return;
    if (viewer.pickNode(e.offsetX, e.offsetY) !== null) return;
    const edge = viewer.pickEdge(e.offsetX, e.offsetY);
    if (edge === null) return;

    aiming = edge;
    aimingWasSelected = viewer.selectedEdge === edge;
    canvas.setPointerCapture(e.pointerId);
    viewer.suspendCamera();
    // Lit, but not yet aimed: the parameter appears on the first movement.
    // Aiming on the press would leave a click telling you to let go of a
    // button you have already let go of.
    void selectEdge(edge);
  });

  // --- picking faces, and aiming a cut ------------------------------------
  /** How far the pointer may travel and still count as a click. Dragging from
   *  a face is how you orbit the view, and it must not also select it. */
  const CLICK_SLOP = 4;
  /** How close to an end a cut may be aimed. The kernel refuses 0 and 1 —
   *  a cut there is not a cut — so the pointer never asks for them. */
  const CUT_MARGIN = 0.02;
  let pressedAt: [number, number] | undefined;
  /** The edge a cut is being aimed along, while the pointer is down on it. */
  let aiming: number | null = null;
  let aimingWasSelected = false;

  canvas.addEventListener("pointermove", (e) => {
    if (aiming !== null) aimAt(e.offsetX, e.offsetY);
  });

  canvas.addEventListener("pointerup", (e) => {
    if (e.button !== 0 || !pressedAt) return;
    const [x, y] = pressedAt;
    pressedAt = undefined;
    const moved = Math.hypot(e.offsetX - x, e.offsetY - y) > CLICK_SLOP;

    if (aiming !== null) {
      canvas.releasePointerCapture(e.pointerId);
      viewer.resumeCamera();
      const edge = aiming;
      aiming = null;
      // Moved along it: that was a cut being aimed. Pressed and released on
      // the spot: that was a click, and a click on the edge that was already
      // lit puts it out.
      if (moved) void cut(edge);
      else if (aimingWasSelected) clearSelection();
      else viewer.aimCut(null);
      return;
    }
    if (moved) return;
    // Smallest target first: a handle, then a line, then the surface behind
    // them. Each is the thing you had to aim at to hit it. A node under the
    // cursor is a drag that has just ended, not a face being chosen.
    if (viewer.pickNode(x, y) !== null) return;
    choose(viewer.pickSurface(x, y));
  });

  /** Turn a pointer position into a parameter along the edge being aimed. */
  function aimAt(x: number, y: number) {
    if (aiming === null) return;
    const ends = viewer.edgeEndsOnScreen(aiming);
    if (!ends) return;
    const [[ax, ay], [bx, by]] = ends;
    const [dx, dy] = [bx - ax, by - ay];
    const along = dx * dx + dy * dy;
    // Seen end-on, an edge has no direction to read a parameter from.
    if (along < 1) return;
    const t = Math.min(
      1 - CUT_MARGIN,
      Math.max(CUT_MARGIN, ((x - ax) * dx + (y - ay) * dy) / along),
    );
    viewer.aimCut(t);
    say(`cut at ${t.toFixed(2)} — let go to cut`);
  }

  async function cut(edge: number) {
    const at = viewer.aimedAt;
    viewer.aimCut(null);
    if (at === null) return;
    const reply = await kernel.apply({ op: "split_sheet", edge, at });
    // Every name on the sheet means something else now, this edge included.
    clearSelection();
    message = reply.message;
    await refresh();
  }

  /**
   * Light up the sheet of hexahedra through an edge.
   *
   * Nothing is cut yet: this is the sheet, shown, so that what a cut would
   * touch is visible before there is a cut. Clicking the same edge again
   * puts it out.
   */
  async function chooseEdge(edge: number) {
    if (viewer.selectedEdge === edge) {
      clearSelection();
      return;
    }
    await selectEdge(edge);
  }

  /** Light the sheet through an edge, whether or not it was already lit. */
  async function selectEdge(edge: number) {
    choose(null);
    const sheet = await kernel.sheet(edge);
    if (!sheet.ok) {
      viewer.showSheet([]);
      viewer.selectedEdge = null;
      say(sheet.error);
      return;
    }
    viewer.selectedEdge = edge;
    viewer.showSheet(sheet.edges);
    say(`sheet through edge ${edge} — ${sheet.edges.length} edges, drag along it to cut`);
  }

  /** Nothing selected: no faces, no sheet. */
  function clearSelection() {
    viewer.selectedEdge = null;
    viewer.showSheet([]);
    choose(null);
  }

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

  async function glueSelection() {
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
  }

  /** The block the first pick landed on. Not the face's block in general —
   *  once two blocks are glued a face has one on each side — but the pick
   *  knows which side it hit. */
  async function removeSelection() {
    const [first] = viewer.selection;
    if (first === undefined) return;
    const reply = await kernel.apply({ op: "remove_block", block: first.block });
    choose(null);
    message = reply.message;
    // Not framed: taking a block away is no reason to move the camera either.
    await refresh();
  }

  /** Put everything back in view. Camera state, so it never goes near the
   *  kernel: in station mode two tabs on one session each have their own. */
  function recentre() {
    viewer.frameAll();
    say("view recentred");
  }

  /** Say something in the readout. */
  function say(what: string) {
    message = what;
    $("status").textContent = what;
  }

  /** `frame` is how the camera should get there, not whether the bytes are
   *  stale — they always are by the time this is called. */
  async function refresh(frame: false | "jump" | "glide" = false) {
    const [bytes, stats] = await Promise.all([kernel.view(), kernel.stats()]);
    viewer.setView(decodeView(bytes));
    if (frame) viewer.frameAll(frame === "jump");
    // One line, out of the way. The counts are what you glance at; the mode
    // matters once, when you are working out which kernel you are talking to.
    $("stats").innerHTML =
      `<span><b data-test="blocks">${stats.blocks}</b> blocks</span>` +
      `<span><b data-test="nodes">${stats.nodes}</b> nodes</span>` +
      `<span><b data-test="edges">${stats.edges}</b> edges</span>` +
      `<span><b data-test="faces">${stats.faces}</b> faces</span>` +
      `<span data-test="mode">${kernel.mode}</span>`;
    $("status").textContent = message;
    revision++;
  }

  // In the station the kernel moves without this tab asking: someone typed in
  // the Python console. This is the whole point of the mode.
  kernel.onChanged(() => {
    message = "changed from Python";
    void refresh();
  });

  async function createCube() {
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
  }

  // --- the chrome ---------------------------------------------------------

  /**
   * Put the interface into the state the preferences describe.
   *
   * Body classes rather than inline styles, so the showing and hiding is the
   * stylesheet's business and can be animated there. `data-chrome` is how the
   * panels learn whether they are floating on a light scene or a dark one.
   */
  function applyChrome() {
    document.body.classList.toggle("drawer-open", prefs.drawer);
    document.body.classList.toggle("chrome-hidden", !prefs.chrome);
    document.body.dataset.chrome = isLight(prefs.theme.background) ? "light" : "dark";
    $("settings").setAttribute("aria-expanded", String(prefs.drawer));
  }

  function showHelp(open: boolean) {
    document.body.classList.toggle("help-open", open);
  }

  /** The help card, written out of the same table the keys are read from. */
  function drawHelp() {
    const groups = [...new Set(helpRows().map((r) => r.group))];
    $("help-card").innerHTML =
      `<div id="help-groups">` +
      groups
        .map(
          (group) =>
            `<section><h2>${group}</h2><dl>` +
            helpRows()
              .filter((r) => r.group === group)
              .map(
                (r) =>
                  `<dt>${r.keys.map((k) => `<kbd>${k}</kbd>`).join("")}</dt>` +
                  `<dd>${r.does}</dd>`,
              )
              .join("") +
            `</dl></section>`,
        )
        .join("") +
      `</div><p id="help-hint">Click a face to select it. Any key closes this.</p>`;
  }
  drawHelp();

  async function screenshot() {
    say("saving…");
    try {
      const picture = await capture(engine, viewer.camera, prefs.shot);
      save(picture, shotName(new Date()), document);
      say("picture saved");
    } catch (e) {
      say(`could not save a picture: ${e}`);
    }
  }

  // --- one definition of what each action does -----------------------------
  //
  // The toolbar and the keyboard both come through here, so a button and its
  // shortcut cannot drift apart: there is only one of each action.
  const ACTIONS: Record<Action, () => void | Promise<void>> = {
    cube: createCube,
    glue: glueSelection,
    remove: removeSelection,
    clear: clearSelection,
    recentre,
    screenshot,
    "axis-x": () => lookDown("+x"),
    "axis-y": () => lookDown("+y"),
    "axis-z": () => lookDown("+z"),
    "axis--x": () => lookDown("-x"),
    "axis--y": () => lookDown("-y"),
    "axis--z": () => lookDown("-z"),
    faces: () => toggleLayer("faces"),
    blocks: () => toggleLayer("blocks"),
    edges: () => {
      // Edges have no visibility of their own; fully shrunk is as good as
      // gone, and it is one slider rather than a second kind of control.
      prefs.edges.shrink = prefs.edges.shrink > 0.1 ? 0.1 : 1;
      redraw();
      drawLayers();
      say(`edges ${prefs.edges.shrink > 0.1 ? "shown" : "hidden"}`);
    },
    internal: () => {
      prefs.faces.internal = !prefs.faces.internal;
      redraw();
      drawLayers();
      say(`internal faces ${prefs.faces.internal ? "shown" : "hidden"}`);
    },
    triad: () => {
      prefs.axes = !prefs.axes;
      viewer.axes.setVisible(prefs.axes);
      $<HTMLInputElement>("show-axes").checked = prefs.axes;
      savePrefs(prefs);
    },
    theme: () => setTheme(isLight(prefs.theme.background) ? PRESETS.dark : PRESETS.light),
    drawer: () => {
      prefs.drawer = !prefs.drawer;
      applyChrome();
      savePrefs(prefs);
    },
    chrome: () => {
      prefs.chrome = !prefs.chrome;
      applyChrome();
      savePrefs(prefs);
    },
    help: () => showHelp(!document.body.classList.contains("help-open")),
  };

  function run(action: Action) {
    void ACTIONS[action]();
  }

  function lookDown(axis: Axis) {
    viewer.axes.lookDown(axis);
    say(`looking down ${axis}`);
  }

  function toggleLayer(group: Group) {
    prefs[group].visible = !prefs[group].visible;
    redraw();
    drawLayers();
    say(`${group} ${prefs[group].visible ? "shown" : "hidden"}`);
  }

  for (const [id, action] of [
    ["cube", "cube"],
    ["glue", "glue"],
    ["remove", "remove"],
    ["recentre", "recentre"],
    ["shot", "screenshot"],
    ["settings", "drawer"],
    ["help-open", "help"],
  ] as [string, Action][]) {
    $(id).addEventListener("click", () => run(action));
  }

  // Any key closes the help card, which is why it is checked before the
  // table: a card you have to find the right key to dismiss is a trap.
  window.addEventListener("keydown", (e) => {
    if (document.body.classList.contains("help-open")) {
      e.preventDefault();
      showHelp(false);
      return;
    }
    // Tab inside the drawer is Tab: once it is open it has fields of its own
    // to walk through, and taking that away would make it unreachable by
    // keyboard.
    if (e.key === "Tab" && $("drawer").contains(document.activeElement)) return;
    const action = actionFor({
      key: e.key,
      code: e.code,
      shiftKey: e.shiftKey,
      ctrlKey: e.ctrlKey,
      metaKey: e.metaKey,
      altKey: e.altKey,
      target: e.target as unknown as { tagName?: string; isContentEditable?: boolean },
    });
    if (!action) return;
    e.preventDefault();
    run(action);
  });
  $("help").addEventListener("click", () => showHelp(false));

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

  applyChrome();
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
    prefs: () => structuredClone(prefs),
    run: (action: Action) => run(action),
    capture: (options: { scale: number; transparent: boolean }) =>
      capture(engine, viewer.camera, options),
    chrome: () => ({
      drawer: document.body.classList.contains("drawer-open"),
      hidden: document.body.classList.contains("chrome-hidden"),
      help: document.body.classList.contains("help-open"),
      tone: document.body.dataset.chrome,
    }),
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
    edgeScreen: (name: number) => viewer.edgeScreen(name),
    selection: () => viewer.selection,
    sheet: () => viewer.highlighted,
    selectedEdge: () => viewer.selectedEdge,
    pickEdge: (x: number, y: number) => viewer.pickEdge(x, y),
    pickEdgeByName: (edge: number) => chooseEdge(edge),
    aimedAt: () => viewer.aimedAt,
    edgeEndsOnScreen: (edge: number) => viewer.edgeEndsOnScreen(edge),
    edgeNames: () => viewer.edgeNames(),
    edgeMidpoint: (edge: number) => viewer.edgeMidpoint(edge),
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
