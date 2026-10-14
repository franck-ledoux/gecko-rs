/**
 * Boot, mode detection, and the test surface.
 *
 * The only thing that differs between the two modes is which `Kernel` gets
 * built here. Everything below this line is written against the interface and
 * has no idea where the kernel is.
 */

import { Engine } from "@babylonjs/core/Engines/engine";
import { decodeGeometry, decodeView } from "./decode";
import { readProbe, type Probe } from "./mode";
import type { Kernel, Stats } from "./kernel";
import { initKernel, WasmKernel } from "./kernel-wasm";
import { RemoteKernel } from "./kernel-remote";
import { Viewer, type Group, type Pick } from "./viewer";
import {
  isLight,
  loadPrefs,
  savePrefs,
  MAX_BOX_MARGIN,
  MAX_HISTORY,
  MAX_SNAP,
  PRESETS,
  SHOT_SCALES,
  SWATCHES,
  type Theme,
} from "./theme";
import { actionFor, helpRows, type Action } from "./shortcuts";
import { capture, save, shotName } from "./capture";
import type { Colouring } from "./surface";
import { GEOM_GROUPS, type GeomGroup } from "./geometry";
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
  /** The geometry this tab has drawn. Compared with the kernel's, which is
   *  how a load made from the Python console reaches a browser that never
   *  asked for one. Starts below zero so the first refresh fetches. */
  let geometryDrawn = -1;

  // --- how it looks: viewer state, never the kernel's ---------------------
  const prefs = loadPrefs();
  viewer.applyPrefs(prefs);
  viewer.axes.setVisible(prefs.axes);
  /** The split, in the three places that have to agree about it. */
  function showQuad() {
    viewer.setQuad(prefs.quad);
    document.body.classList.toggle("quad", prefs.quad);
    $("quad").setAttribute("aria-pressed", String(prefs.quad));
  }
  showQuad();

  /** One path for every control: change `prefs`, hand the lot to the viewer,
   *  remember it. Nothing reaches the scene any other way. */
  function redraw() {
    viewer.applyPrefs(prefs);
    drawDigests();
    savePrefs(prefs);
  }

  /**
   * Every folded group's line, back in step with what it stands for.
   *
   * Here rather than in each control, and run wherever anything changes:
   * seven spans is nothing, and a line that says `uniform · 35%` about a
   * group that is now hidden is worse than no line at all.
   */
  function drawDigests() {
    const say = (group: string, html: string) => {
      const span = document.querySelector(`#grp-${group} .digest`);
      if (span) span.outerHTML = html;
    };
    for (const { group } of LAYERS) {
      say(group, digest(prefs[group].colouring, prefs[group].alpha, prefs[group].visible));
    }
    say("edges", digest(prefs.edges.colouring, undefined, prefs.edges.visible));
    say("nodes", digest(prefs.nodes.colouring));
    for (const group of GEOM_GROUPS) {
      say(group, digest(prefs[group].colouring, prefs[group].alpha, prefs[group].visible));
    }
  }

  function setTheme(theme: Theme) {
    prefs.theme = { ...theme };
    redraw();
    applyChrome();
    drawSwatches();
    drawLayers();
    drawGeometryLayers();
    // The legend is the theme, so it changes when the theme does.
    drawHelp();
  }

  /**
   * The faces and blocks controls, generated from this list.
   *
   * Both groups are drawn from the same quadrangles, so they take the same
   * four controls — shown, coloured how, how opaque, how shrunk — and only
   * faces have anything extra to say.
   */
  /**
   * The cells' controls, generated from this list.
   *
   * By falling dimension, as the geometry's are: a block, then its faces,
   * then their edges, then the nodes those meet at.
   *
   * `class` is offered where it means something. A block is classified on
   * nothing — a surface mesh has no volumes to classify one on — so it
   * keeps the two colourings it always had.
   */
  const LAYERS: ReadonlyArray<{ group: Group; label: string; follows: boolean }> = [
    { group: "blocks", label: "Blocks", follows: false },
    { group: "faces", label: "Faces", follows: true },
  ];

  const slider = (test: string, label: string, min: number, value: number) =>
    `<label class="slider"><span>${label}</span>` +
    `<input type="range" min="${min}" max="1" step="0.01" value="${value}"` +
    ` data-test="${test}" /></label>`;

  /** The uniform / id / follows radios for one group, on one line.
   *
   *  Short words on purpose: three of them have to fit across a drawer
   *  246 pixels wide, and "by what it follows" took a line of its own. The
   *  help card carries the long form, and so does the legend under it. */
  const modes = (group: string, now: Colouring, follows: boolean) => {
    const one = (value: Colouring, text: string) =>
      `<label class="mode"><input type="radio" name="${group}-mode" value="${value}"` +
      ` data-test="${group}-${value}"${now === value ? " checked" : ""} />${text}</label>`;
    return (
      `<div class="row modes">${one("uniform", "uniform")}${one("id", "id")}` +
      (follows ? one("class", "follows") : "") +
      `</div>`
    );
  };

  /** What a folded group says about itself: hidden, or how it is coloured
   *  and how far through it you can see. The line that stands for a group
   *  has to answer the question you opened it to ask. */
  const digest = (colouring: Colouring, alpha?: number, visible = true) => {
    if (!visible) return `<span class="digest">hidden</span>`;
    const how = colouring === "class" ? "follows" : colouring;
    const seen = alpha === undefined ? "" : ` · ${Math.round(alpha * 100)}%`;
    return `<span class="digest">${how}${seen}</span>`;
  };

  /**
   * The line a group folds to: its own show box, its name, its digest.
   *
   * `name` groups the folds of one band, so opening one closes the last —
   * which is what keeps the drawer the height of one open group however
   * many groups there are.
   */
  const summary = (band: string, group: string, label: string, inside: string, show?: boolean) =>
    `<details class="layer" name="${band}" id="grp-${group}"${
      prefs.panels.includes(`grp-${group}`) ? " open" : ""
    }><summary>` +
    (show === undefined
      ? ""
      : `<label class="check"><input type="checkbox" data-test="${group}-visible"` +
        `${show ? " checked" : ""} /></label>`) +
    `<span class="name">${label}</span>${inside}</summary>`;

  /** Hook up one group's radios. */
  function wireModes(group: string, set: (value: Colouring) => void) {
    for (const value of ["uniform", "id", "class"] as Colouring[]) {
      const input = $("layers").querySelector<HTMLInputElement>(
        `[data-test="${group}-${value}"]`,
      );
      input?.addEventListener("change", () => {
        set(value);
        redraw();
      });
    }
  }

  function drawLayers() {
    $("layers").innerHTML =
      LAYERS.map(({ group, label, follows }) => {
        const layer = prefs[group];
        return (
          summary(
            "cells",
            group,
            label,
            digest(layer.colouring, layer.alpha, layer.visible),
            layer.visible,
          ) +
          modes(group, layer.colouring, follows) +
          slider(`${group}-alpha`, "opacity", 0, layer.alpha) +
          slider(`${group}-shrink`, "shrink", 0.1, layer.shrink) +
          (group === "faces"
            ? `<label class="check"><input type="checkbox" data-test="faces-internal"` +
              `${prefs.faces.internal ? " checked" : ""} /> internal faces</label>`
            : "") +
          // Closed, and it matters: an unclosed element is not a parse
          // error, it is an element that swallows everything after it.
          `</details>`
        );
      }).join("") +
      summary(
        "cells",
        "edges",
        "Edges",
        digest(prefs.edges.colouring, undefined, prefs.edges.visible),
        prefs.edges.visible,
      ) +
      modes("edges", prefs.edges.colouring, true) +
      slider("edges-shrink", "shrink", 0.1, prefs.edges.shrink) +
      `</details>` +
      // Nodes are always drawn — they are what you grab — so all they
      // carry is how they are coloured.
      summary("cells", "nodes", "Nodes", digest(prefs.nodes.colouring)) +
      modes("nodes", prefs.nodes.colouring, true) +
      `</details>`;

    for (const { group } of LAYERS) {
      const find = (test: string) =>
        $("layers").querySelector<HTMLInputElement>(`[data-test="${test}"]`)!;
      find(`${group}-visible`).addEventListener("change", (e) => {
        prefs[group].visible = (e.target as HTMLInputElement).checked;
        redraw();
      });
      wireModes(group, (value) => {
        prefs[group].colouring = value;
      });
      find(`${group}-alpha`).addEventListener("input", (e) => {
        prefs[group].alpha = Number((e.target as HTMLInputElement).value);
        redraw();
      });
      find(`${group}-shrink`).addEventListener("input", (e) => {
        prefs[group].shrink = Number((e.target as HTMLInputElement).value);
        redraw();
      });
    }
    wireModes("edges", (value) => {
      prefs.edges.colouring = value;
    });
    wireModes("nodes", (value) => {
      prefs.nodes.colouring = value;
    });
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
    const wire = $("layers").querySelector<HTMLInputElement>('[data-test="edges-visible"]')!;
    wire.addEventListener("change", (e) => {
      prefs.edges.visible = (e.target as HTMLInputElement).checked;
      redraw();
    });
    keepFolds("layers");
  }
  drawLayers();

  /**
   * The geometry's three groups, generated the same way.
   *
   * The same three controls each — shown, coloured how, how opaque — and
   * deliberately no shrink: a geometry is not a decomposition you can
   * explode, its surfaces meet along the very curves also on screen.
   */
  function drawGeometryLayers() {
    $("geometry").innerHTML = GEOM_GROUPS.map((group) => {
      const layer = prefs[group];
      return (
        summary(
          "geometry",
          group,
          group,
          digest(layer.colouring, layer.alpha, layer.visible),
          layer.visible,
        ) +
        modes(group, layer.colouring, false) +
        slider(`${group}-alpha`, "opacity", 0, layer.alpha) +
        `</details>`
      );
    }).join("");

    for (const group of GEOM_GROUPS) {
      const find = (test: string) =>
        $("geometry").querySelector<HTMLInputElement>(`[data-test="${test}"]`)!;
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
    }
    keepFolds("geometry");
  }
  drawGeometryLayers();

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

  // --- the folds ----------------------------------------------------------
  for (const section of $("drawer").querySelectorAll<HTMLDetailsElement>(".section")) {
    section.open = prefs.panels.includes(section.id);
  }
  // `toggle` does not bubble, so it is caught on the way down. Read off the
  // DOM rather than tracked: opening one group of a band closes the last,
  // and asking what is open afterwards cannot get that wrong.
  $("drawer").addEventListener(
    "toggle",
    () => {
      prefs.panels = [...$("drawer").querySelectorAll("details[open]")].map((d) => d.id);
      savePrefs(prefs);
    },
    true,
  );

  /** A box in a fold's own line toggles the group, never the fold. */
  function keepFolds(where: string) {
    for (const input of $(where).querySelectorAll("summary input")) {
      input.addEventListener("click", (e) => e.stopPropagation());
    }
  }

  // --- what the operations do ---------------------------------------------
  //
  // Not state in the kernel: each of these is an argument to the command it
  // belongs to, sent when that command is sent. The kernel's own constants
  // stay the default for anyone who sends none — a script, or this page
  // before anybody touched the slider.
  const share = (id: string, key: "margin" | "snap", most: number) => {
    const input = $<HTMLInputElement>(id);
    input.max = String(most);
    input.value = String(prefs.box[key]);
    const show = () => {
      $(`${id}-n`).textContent = `${+(prefs.box[key] * 100).toFixed(1)}%`;
    };
    show();
    input.addEventListener("input", () => {
      prefs.box[key] = Number(input.value);
      show();
      savePrefs(prefs);
    });
  };
  share("box-margin", "margin", MAX_BOX_MARGIN);
  share("snap", "snap", MAX_SNAP);

  // --- how far back it can go ---------------------------------------------
  const history = $<HTMLInputElement>("history");
  history.max = String(MAX_HISTORY);
  history.value = String(prefs.history);
  $("history-n").textContent = String(prefs.history);
  history.addEventListener("input", () => {
    prefs.history = Number(history.value);
    $("history-n").textContent = String(prefs.history);
    savePrefs(prefs);
    void setHistory();
  });

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
    if (e.button !== 0 || aiming !== null || viewer.aimedAtGeometry) return;
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

  // Pressing a face that is **already selected** grows a block out of it.
  // Any other face is how you turn the view, which is why this asks for a
  // selection first: a face fills the screen, and a drag from one cannot be
  // both an orbit and an extrusion.
  canvas.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || aiming !== null || growing || viewer.aimedAtGeometry) return;
    if (viewer.axes.pick(e.offsetX, e.offsetY)) return;
    if (viewer.pickNode(e.offsetX, e.offsetY) !== null) return;
    if (viewer.pickEdge(e.offsetX, e.offsetY) !== null) return;
    const surface = viewer.pickSurface(e.offsetX, e.offsetY);
    const [first] = viewer.selection;
    if (!surface || !first || first.face !== surface.face || first.block !== surface.block) {
      return;
    }
    if (!viewer.beginExtrusion(surface)) return;
    growing = true;
    canvas.setPointerCapture(e.pointerId);
    viewer.suspendCamera();
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
  /** Whether a face is being pulled out into a block. */
  let growing = false;

  canvas.addEventListener("pointermove", (e) => {
    if (aiming !== null) aimAt(e.offsetX, e.offsetY);
    if (growing) {
      const height = viewer.aimExtrusion(e.offsetX, e.offsetY);
      if (height !== null) say(`extruding ${height.toFixed(2)} — let go to build`);
    }
  });

  canvas.addEventListener("pointerup", (e) => {
    if (e.button !== 0 || !pressedAt) return;
    const [x, y] = pressedAt;
    pressedAt = undefined;
    const moved = Math.hypot(e.offsetX - x, e.offsetY - y) > CLICK_SLOP;

    if (growing) {
      canvas.releasePointerCapture(e.pointerId);
      viewer.resumeCamera();
      growing = false;
      const from = viewer.growingFrom;
      const height = viewer.grownBy;
      viewer.endExtrusion();
      // Pressed and let go on the spot is a click, not an extrusion, and
      // falls through to the selection below — a selected face you click
      // again is a face you are letting go of.
      if (moved && from && height !== null) {
        void extrude(from, height);
        return;
      }
    }
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
    // Aimed at the geometry, the blocks are not there to be clicked at all
    // — which is the whole reason for the mode: the two are at the same
    // pixels, and whichever answered first would hide the other.
    if (viewer.aimedAtGeometry) {
      const tag = viewer.pickGeometry(x, y);
      if (tag === null) clearSelection();
      else toggleSurface(tag);
      return;
    }
    // Smallest target first: a handle, then a line, then the surface behind
    // them. Each is the thing you had to aim at to hit it. A press and
    // release on a handle without moving is a click on that node, which
    // picks it — the drag behaviour has already decided it was not a drag.
    const node = viewer.pickNode(x, y);
    if (node !== null) {
      pickNode(node);
      return;
    }
    const surface = viewer.pickSurface(x, y);
    // Nothing under the pointer at all: the background is how you let go of
    // everything, whichever kind of selection it was.
    if (surface === null) clearSelection();
    else choose(surface);
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

  async function extrude(from: Pick, height: number) {
    const reply = await kernel.apply({
      op: "extrude_face",
      face: from.face,
      block: from.block,
      height,
    });
    // That face has a block on each side now, so holding it selected would
    // leave the panel offering to grow from something that cannot be.
    clearSelection();
    message = reply.message;
    // Not framed: you are looking at the thing you just built.
    await refresh();
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
    $<HTMLButtonElement>("remove").disabled = false;
    // The block count, not just the edges: it is what a removal would take
    // away, and taking out more than you meant to looks like a hole.
    say(
      `sheet through edge ${edge} — ${sheet.edges.length} edges` +
        ` across ${sheet.blocks} block${sheet.blocks === 1 ? "" : "s"},` +
        ` drag along it to cut`,
    );
  }

  /** Nothing selected: no faces, no sheet. */
  function clearSelection() {
    viewer.selectedEdge = null;
    viewer.showSheet([]);
    viewer.selectSurfaces([]);
    viewer.selectNode(null);
    choose(null);
  }

  /**
   * Pick a node, or put it out again.
   *
   * There was no such thing until now: a node was something you dragged
   * and nothing else. Freezing and unclassifying are things you say about
   * one, so one has to be nameable without being moved.
   */
  function pickNode(name: number) {
    const already = viewer.selectedNode === name;
    viewer.selectNode(already ? null : name);
    if (already) {
      say("");
      return;
    }
    const state = viewer.nodeState(name);
    const on = state?.on ? ` on ${nameOf(state.on)}` : " on nothing";
    say(`node ${name}${on}${state?.frozen ? ", frozen" : ""}`);
  }

  /** How a classification reads, the same way the kernel says it. */
  function nameOf(on: { dim: number; tag: number }) {
    return `${["corner", "curve", "surface"][on.dim] ?? "?"} ${on.tag}`;
  }

  /**
   * Add a geometric surface to the selection, or take it out again.
   *
   * Its own list rather than the face selection's: a surface of the
   * geometry and a face of the blocking are different things, and the
   * verbs that act on them are different verbs.
   */
  function toggleSurface(tag: number) {
    const picked = viewer.surfaceSelection;
    const now = picked.includes(tag) ? picked.filter((t) => t !== tag) : [...picked, tag];
    viewer.selectSurfaces(now);
    say(
      now.length === 0
        ? "no surface selected — a box would take the whole geometry"
        : `${now.length} surface${now.length === 1 ? "" : "s"} selected`,
    );
  }

  /**
   * Whether the mouse is aimed at the geometry or at the blocks.
   *
   * Said in two places, because a mode you cannot see is a mode you click
   * twice to find out about: the button wears the icon of the mode it is
   * in, and the readout names it in the same blue the status line uses.
   * Not *also* announced in the status line — that line is for what just
   * happened, and a standing fact repeated there pushes out the message
   * about the thing you actually did.
   */
  function setPicking(at: "blocks" | "geometry") {
    viewer.setPicking(at);
    $("picking").setAttribute("aria-pressed", String(at === "geometry"));
    $("picking").title =
      at === "geometry" ? "Aim at the blocks — M" : "Aim at the geometry — M";
    $("aim").textContent = at === "geometry" ? "picking geometry" : "picking blocks";
    if (at === "blocks") viewer.selectSurfaces([]);
    else clearSelection();
  }

  /** Put the whole blocking on the geometry. */
  async function classify() {
    const reply = await kernel.apply({ op: "classify", tolerance: prefs.box.snap });
    message = reply.message;
    await refresh();
  }

  /**
   * Lock the picked node, or let it go.
   *
   * On the picked node rather than on a drag, because freezing is a thing
   * you say about a node and dragging is a thing you do to one — and a
   * frozen node cannot be dragged at all, so the gesture would have
   * nothing to attach to.
   */
  async function freezeNode() {
    const node = viewer.selectedNode;
    if (node === null) {
      say("pick a node first — click one");
      return;
    }
    const was = viewer.nodeState(node)?.frozen ?? false;
    const reply = await kernel.apply({ op: "freeze_node", node, frozen: !was });
    message = reply.message;
    await refresh();
  }

  /** Take the picked node off the geometry. It stays where it is; what
   *  changes is that it may now go anywhere. */
  async function unclassifyNode() {
    const node = viewer.selectedNode;
    if (node === null) {
      say("pick a node first — click one");
      return;
    }
    if (!viewer.nodeState(node)?.on) {
      say(`node ${node} is on nothing already`);
      return;
    }
    const reply = await kernel.apply({ op: "classify_node", node });
    message = reply.message;
    await refresh();
  }

  async function boxGeometry() {
    // The margin is sent, because it is adjustable: the kernel's constant
    // is what you get when nobody says, and the slider in the settings
    // panel is somebody saying.
    const reply = await kernel.apply({
      op: "box_geometry",
      surfaces: viewer.surfaceSelection,
      margin: prefs.box.margin,
    });
    message = reply.message;
    // Framed, because the box is almost always bigger than anything that
    // was on screen before it.
    await refresh(reply.ok ? "glide" : false);
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
    else picked = [...picked, pick];

    viewer.select(picked);
    // No cap: a glue wants two and a pillow wants as many as you like, so
    // the selection just grows and each verb says what it can do with it.
    $<HTMLButtonElement>("glue").disabled = picked.length !== 2;
    $<HTMLButtonElement>("remove").disabled = picked.length === 0 && viewer.selectedEdge === null;
    $<HTMLButtonElement>("pillow").disabled = picked.length === 0;
    // The block is named as well as the face: it is what remove and pillow
    // both act on, and a click that changes something unnamed is no good.
    // Two is a glue, and which way round it goes decides where the seam
    // lands, so that is still said in full. More than two is a pillow.
    message =
      picked.length === 0
        ? ""
        : picked.length === 1
          ? `face ${picked[0].face} (block ${picked[0].block}) selected`
          : picked.length === 2
            ? `face ${picked[1].face} will be glued onto face ${picked[0].face}` +
              ` — or pillow both`
            : `${picked.length} faces — pillow takes them all`;
    $("status").textContent = message;
  }

  async function pillowSelection() {
    const picked = viewer.selection;
    if (!picked.length) return;
    const reply = await kernel.apply({
      op: "pillow",
      faces: picked.map((p) => ({ face: p.face, block: p.block })),
    });
    // Every one of those faces is a different cell now.
    clearSelection();
    message = reply.message;
    // Not framed: the model's surface is exactly where it was, which is the
    // point of the operation.
    await refresh();
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

  /**
   * Take away whatever is selected.
   *
   * A lit sheet means the whole layer; otherwise the block the first pick
   * landed on. Not the face's block in general — once two blocks are glued
   * a face has one on each side — but the pick knows which side it hit.
   */
  async function removeSelection() {
    const edge = viewer.selectedEdge;
    const [first] = viewer.selection;
    const command =
      edge !== null
        ? { op: "remove_sheet", edge }
        : first !== undefined
          ? { op: "remove_block", block: first.block }
          : null;
    if (!command) return;
    const reply = await kernel.apply(command);
    clearSelection();
    message = reply.message;
    // Not framed: taking something away is no reason to move the camera.
    await refresh();
  }

  /**
   * Open a gmsh surface mesh as the geometry.
   *
   * The file is read **here** and its contents go into the command. A
   * browser has no path to hand the kernel, and the one command set has to
   * be the one both modes can send — so there is no path anywhere in it,
   * not even in the station where there would be a filesystem to name.
   */
  function openGeometry() {
    $<HTMLInputElement>("msh").click();
  }

  async function loadGeometry(name: string, msh: string) {
    say(`reading ${name}…`);
    const reply = await kernel.apply({ op: "load_geometry", msh, name });
    message = reply.message;
    // Framed: a geometry you have just opened is the thing to look at, and
    // it may be nowhere near where the camera was pointing.
    await refresh("glide");
  }

  $("msh").addEventListener("change", async (e) => {
    const input = e.target as HTMLInputElement;
    const file = input.files?.[0];
    // Cleared whatever happens, so opening the same file twice in a row
    // still fires a change event the second time.
    input.value = "";
    if (!file) return;
    try {
      await loadGeometry(file.name, await file.text());
    } catch (problem) {
      say(`could not read ${file.name}: ${problem}`);
    }
  });

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
    // Only when it has actually changed. The geometry is the big buffer and
    // the one that never moves; re-sending it because a node was dragged is
    // the whole cost this generation number exists to avoid.
    if (stats.geometry.generation !== geometryDrawn) {
      geometryDrawn = stats.geometry.generation;
      viewer.setGeometry(decodeGeometry(await kernel.geometry()));
    }
    // After the geometry, not before: framing has to see everything there
    // is to frame.
    if (frame) viewer.frameAll(frame === "jump");
    // Nothing to aim at, box or classify onto without one.
    const loaded = stats.geometry.surfaces !== undefined;
    for (const id of ["picking", "box", "classify"]) {
      $<HTMLButtonElement>(id).disabled = !loaded;
    }
    if (!loaded && viewer.aimedAtGeometry) setPicking("blocks");
    showHistory(stats);
    // One line, out of the way. The counts are what you glance at; the mode
    // matters once, when you are working out which kernel you are talking to.
    $("stats").innerHTML =
      `<span><b data-test="blocks">${stats.blocks}</b> blocks</span>` +
      `<span><b data-test="nodes">${stats.nodes}</b> nodes</span>` +
      `<span><b data-test="edges">${stats.edges}</b> edges</span>` +
      `<span><b data-test="faces">${stats.faces}</b> faces</span>` +
      (stats.geometry.surfaces === undefined
        ? ""
        : `<span><b data-test="geom-surfaces">${stats.geometry.surfaces}</b> surfaces</span>` +
          `<span><b data-test="geom-curves">${stats.geometry.curves}</b> curves</span>` +
          `<span><b data-test="geom-corners">${stats.geometry.corners}</b> corners</span>` +
          // How much of the blocking is on the model, which is the thing
          // you are working towards and so the thing to glance at.
          `<span><b data-test="pinned">${stats.classified.nodes}</b>/` +
          `${stats.nodes} pinned</span>`) +
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

  /**
   * Take back the last change.
   *
   * The selection goes with it. What is selected is named by number, and a
   * block that no longer exists cannot stay lit — nor can the node a cut
   * made, which after the cut is taken back is a name nothing answers to.
   */
  async function undoLast() {
    const reply = await kernel.apply({ op: "undo" });
    clearSelection();
    message = reply.message;
    await refresh();
  }

  /**
   * Tell the kernel how many steps to keep.
   *
   * The states live in the kernel — it is the kernel that has the model to
   * copy — so this is the one preference that is not purely this tab's. In
   * the station that means the two tabs on one session share it, last one
   * to move the slider winning, which is the same bargain as every other
   * thing the session holds.
   */
  async function setHistory() {
    await kernel.apply({ op: "set_history", steps: prefs.history });
    showHistory(await kernel.stats());
  }

  /** The undo button is live exactly when there is something to undo. */
  function showHistory(stats: Stats) {
    $<HTMLButtonElement>("undo").disabled = stats.history.back === 0;
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
    document.body.classList.toggle("params-open", prefs.params);
    document.body.classList.toggle("chrome-hidden", !prefs.chrome);
    document.body.dataset.chrome = isLight(prefs.theme.background) ? "light" : "dark";
    $("display").setAttribute("aria-expanded", String(prefs.drawer));
    $("settings").setAttribute("aria-expanded", String(prefs.params));
  }

  /**
   * Open one of the two panels, or shut it.
   *
   * Never both: they are drawn in the same place, and two sheets of glass
   * on top of each other is a bug that looks like a rendering fault.
   */
  function showPanel(which: "drawer" | "params", open: boolean) {
    prefs.drawer = which === "drawer" && open;
    prefs.params = which === "params" && open;
    applyChrome();
    savePrefs(prefs);
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
      // In the grid with the key groups rather than under it: the Edit
      // column is twice the height of the others, and the legend fits in
      // the space that leaves.
      legend() +
      `</div>` +
      `<p id="help-hint">Click a node, an edge or a face to pick it; <kbd>M</kbd> aims at the geometry instead. Any key closes this.</p>`;
  }

  /**
   * What the classification colours mean.
   *
   * Read off the theme rather than written out, so the card cannot come
   * to disagree with the screen — the same reason the keys above it are
   * generated from the table they are matched against.
   */
  function legend() {
    const swatch = (colour: string, says: string) =>
      `<span class="key"><i style="background:${colour}"></i>${says}</span>`;
    return (
      `<section><h2>What a cell follows</h2>` +
      `<div id="help-legend">` +
      swatch(prefs.theme.onCorner, "on a corner") +
      swatch(prefs.theme.onCurve, "on a curve") +
      swatch(prefs.theme.onSurface, "on a surface") +
      swatch(prefs.theme.nodes, "on nothing") +
      swatch(prefs.theme.frozen, "frozen: it does not move") +
      `</div>` +
      `<p class="aside">Nodes, edges and faces take these when their box` +
      ` in the display panel is set to <b>follows</b>.</p></section>`
    );
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
    undo: undoLast,
    open: openGeometry,
    picking: () => setPicking(viewer.aimedAtGeometry ? "blocks" : "geometry"),
    box: () => boxGeometry(),
    classify,
    freeze: freezeNode,
    unclassify: unclassifyNode,
    glue: glueSelection,
    remove: removeSelection,
    pillow: pillowSelection,
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
      prefs.edges.visible = !prefs.edges.visible;
      redraw();
      drawLayers();
      say(`edges ${prefs.edges.visible ? "shown" : "hidden"}`);
    },
    corners: () => toggleGeomLayer("corners"),
    curves: () => toggleGeomLayer("curves"),
    surfaces: () => toggleGeomLayer("surfaces"),
    internal: () => {
      prefs.faces.internal = !prefs.faces.internal;
      redraw();
      drawLayers();
      say(`internal faces ${prefs.faces.internal ? "shown" : "hidden"}`);
    },
    quad: () => {
      prefs.quad = !prefs.quad;
      showQuad();
      savePrefs(prefs);
      say(prefs.quad ? "split into four views" : "one view");
    },
    triad: () => {
      prefs.axes = !prefs.axes;
      viewer.axes.setVisible(prefs.axes);
      $<HTMLInputElement>("show-axes").checked = prefs.axes;
      savePrefs(prefs);
    },
    theme: () => setTheme(isLight(prefs.theme.background) ? PRESETS.dark : PRESETS.light),
    drawer: () => showPanel("drawer", !prefs.drawer),
    params: () => showPanel("params", !prefs.params),
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

  function toggleGeomLayer(group: GeomGroup) {
    prefs[group].visible = !prefs[group].visible;
    redraw();
    drawGeometryLayers();
    say(`geometric ${group} ${prefs[group].visible ? "shown" : "hidden"}`);
  }

  for (const [id, action] of [
    ["cube", "cube"],
    ["open", "open"],
    ["picking", "picking"],
    ["box", "box"],
    ["classify", "classify"],
    ["glue", "glue"],
    ["remove", "remove"],
    ["pillow", "pillow"],
    ["undo", "undo"],
    ["quad", "quad"],
    ["recentre", "recentre"],
    ["shot", "screenshot"],
    ["display", "drawer"],
    ["settings", "params"],
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
    // With the reach the settings panel asks for: a node dropped near an
    // entity is taken by it, and a reach of nothing means it is not.
    const reply = await kernel.apply({
      op: "move_node",
      node: name,
      to,
      snap: prefs.box.snap,
    });
    message = reply.message;
    await refresh();
  });
  window.addEventListener("resize", () => {
    engine.resize();
    viewer.resize();
  });

  applyChrome();
  // The kernel starts with its own default; this is the one in the drawer,
  // which is the one the person using it last chose.
  await setHistory();
  $("aim").textContent = "picking blocks";
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
      params: document.body.classList.contains("params-open"),
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
    split: () => viewer.split,
    pane: () => viewer.aimedAtPane,
    paneCamera: () => viewer.paneCamera(),
    dragPlane: () => viewer.dragPlane(),
    grownBy: () => viewer.grownBy,
    picking: () => (viewer.aimedAtGeometry ? "geometry" : "blocks"),
    surfaceSelection: () => viewer.surfaceSelection,
    selectSurfaces: (tags: number[]) => viewer.selectSurfaces(tags),
    pickGeometry: (x: number, y: number) => viewer.pickGeometry(x, y),
    geomCount: (group: GeomGroup) => viewer.geomCount(group),
    geomTags: (group: GeomGroup) => viewer.geomTags(group),
    geomLook: (group: GeomGroup, tag: number) => viewer.geomLook(group, tag),
    /** Load a geometry without a file picker, which a test cannot drive
     *  from inside the page. The path through the kernel is the same one. */
    loadGeometry: (name: string, msh: string) => loadGeometry(name, msh),
    /** Jump the camera onto an axis without the animation, so a test asserts
     *  where it landed rather than racing it. */
    snapTo: (axis: Axis) => viewer.axes.snapTo(axis),
    pickAxis: (x: number, y: number) => viewer.axes.pick(x, y),
    axisScreen: (axis: Axis) => viewer.axes.axisScreen(axis),
    frameTime: () => engine.getDeltaTime(),
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
    selectedNode: () => viewer.selectedNode,
    nodeState: (name: number) => viewer.nodeState(name),
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
    /** Put the camera at a distance, without the wheel's thirty-five notches.
     *  What lets a test say "and from twice as far away it looks the same". */
    setRadius: (radius: number) => {
      viewer.camera.radius = radius;
    },
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
