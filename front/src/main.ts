/**
 * Boot, mode detection, and the test surface.
 *
 * The only thing that differs between the two modes is which `Kernel` gets
 * built here. Everything below this line is written against the interface and
 * has no idea where the kernel is.
 */

import { Engine } from "@babylonjs/core/Engines/engine";
import { decodeGeometry, decodeMesh, decodeView } from "./decode";
import type { Stats } from "./kernel";
import { initKernel, WasmKernel } from "./kernel-wasm";
import { Viewer, type Group, type Pick } from "./viewer";
import {
  isLight,
  loadPrefs,
  savePrefs,
  MAX_BOX_MARGIN,
  MAX_HISTORY,
  MAX_SEGMENTS,
  MAX_SMOOTH_ROUNDS,
  MAX_SNAP,
  PRESETS,
  SHOT_SCALES,
  SWATCHES,
  type Theme,
} from "./theme";
import { actionFor, helpRows, type Action } from "./shortcuts";
import { capture, fileName, save, saveBlob, shotName } from "./capture";
import { Interpreter, type Answer } from "./lua";
import * as files from "./files";
import type { Colouring } from "./surface";
import { GEOM_GROUPS, spotName, type GeomGroup, type Spot } from "./geometry";
import type { Axis } from "./axes";
import "./style.css";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

/** Where to put the next cube, so successive ones stand side by side.
 *  Placement is the interface's business; the kernel takes an origin. */
function nextOrigin(blocks: number): [number, number, number] {
  return [blocks * 1.5, 0, 0];
}

async function boot() {
  // The one thing that has to be waited for: the kernel's own wasm.
  // Everything after it is in this tab and answers at once.
  await initKernel();
  const kernel = new WasmKernel();

  const canvas = $<HTMLCanvasElement>("view");
  const engine = new Engine(canvas, true, { preserveDrawingBuffer: true }, true);
  const viewer = new Viewer(engine);
  viewer.attachTo(canvas);

  let message = "";
  /** The components that are patches, as the last redraw found them. */
  let patchIds = new Set<number>();
  const isPatch = (pick: { block: number }) => patchIds.has(pick.block);
  let revision = 0;
  /** The geometry this tab has drawn. Compared with the kernel's, which is
   *  how a load made from the Python console reaches a browser that never
   *  asked for one. Starts below zero so the first refresh fetches. */
  let geometryDrawn = -1;
  /** And the mesh this tab has drawn, on the same terms. */
  let meshDrawn = -1;

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
    say("mesh", digest(prefs.mesh.colouring, prefs.mesh.alpha, prefs.mesh.visible));
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

  /**
   * The mesh's skin, drawn on the same three terms as a geometry group.
   *
   * A band of its own rather than one more cell group: a mesh is not part
   * of the blocking, it is what the blocking was for, and it comes and
   * goes on a clock of its own.
   */
  function drawMeshLayer() {
    const layer = prefs.mesh;
    $("mesh-layer").innerHTML =
      summary(
        "mesh",
        "mesh",
        "skin",
        digest(layer.colouring, layer.alpha, layer.visible),
        layer.visible,
      ) +
      modes("mesh", layer.colouring, false) +
      slider("mesh-alpha", "opacity", 0, layer.alpha) +
      `</details>`;

    const find = (test: string) =>
      $("mesh-layer").querySelector<HTMLInputElement>(`[data-test="${test}"]`)!;
    find("mesh-visible").addEventListener("change", (e) => {
      prefs.mesh.visible = (e.target as HTMLInputElement).checked;
      redraw();
    });
    for (const value of ["uniform", "id"] as Colouring[]) {
      find(`mesh-${value}`).addEventListener("change", () => {
        prefs.mesh.colouring = value;
        redraw();
      });
    }
    find("mesh-alpha").addEventListener("input", (e) => {
      prefs.mesh.alpha = Number((e.target as HTMLInputElement).value);
      redraw();
    });
    keepFolds("mesh-layer");
  }
  drawMeshLayer();

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

  // Two of them, because smoothing is "how many times" and "how far each
  // time" and neither alone says anything useful.
  const rounds = $<HTMLInputElement>("smooth-rounds");
  rounds.max = String(MAX_SMOOTH_ROUNDS);
  rounds.value = String(prefs.smooth.rounds);
  $("smooth-rounds-n").textContent = String(prefs.smooth.rounds);
  rounds.addEventListener("input", () => {
    prefs.smooth.rounds = Number(rounds.value);
    $("smooth-rounds-n").textContent = String(prefs.smooth.rounds);
    savePrefs(prefs);
  });

  const howFar = $<HTMLInputElement>("smooth-share");
  howFar.value = String(prefs.smooth.share);
  const showShare = () => {
    $("smooth-share-n").textContent = `${Math.round(prefs.smooth.share * 100)}%`;
  };
  showShare();
  howFar.addEventListener("input", () => {
    prefs.smooth.share = Number(howFar.value);
    showShare();
    savePrefs(prefs);
  });

  const segments = $<HTMLInputElement>("segments");
  segments.max = String(MAX_SEGMENTS);
  segments.value = String(prefs.segments);
  $("segments-n").textContent = String(prefs.segments);
  segments.addEventListener("input", () => {
    prefs.segments = Number(segments.value);
    $("segments-n").textContent = String(prefs.segments);
    savePrefs(prefs);
  });

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

  /**
   * Hold `W` — or shift — and drag: a rubber band over the handles.
   *
   * Registered before everything else that takes a press, and the others
   * stand aside while it is running — a shift-drag is a selection and
   * nothing else. The camera is suspended for the same reason it is during
   * a cut: Babylon would otherwise orbit underneath.
   *
   * It **adds** to what is already picked, so a model you cannot catch in
   * one rectangle can be caught in two.
   */
  canvas.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || !(banding || e.shiftKey) || viewer.aimedAtGeometry) return;
    if (aiming !== null || growing) return;
    lasso = [e.offsetX, e.offsetY];
    canvas.setPointerCapture(e.pointerId);
    viewer.suspendCamera();
    drawLasso(lasso, lasso);
  });

  canvas.addEventListener("pointerup", (e) => {
    if (!lasso) return;
    const from = lasso;
    lasso = undefined;
    canvas.releasePointerCapture(e.pointerId);
    viewer.resumeCamera();
    $("lasso").hidden = true;
    const caught = viewer.nodesInRect(from[0], from[1], e.offsetX, e.offsetY);
    const now = [...new Set([...viewer.selectedNodes, ...caught])];
    viewer.selectNodes(now);
    sayNodes(now);
  });

  /** Where the band is, in the CSS pixels the canvas is laid out in. */
  function drawLasso(from: [number, number], to: [number, number]) {
    const box = $("lasso");
    const rect = canvas.getBoundingClientRect();
    box.hidden = false;
    box.style.left = `${rect.left + Math.min(from[0], to[0])}px`;
    box.style.top = `${rect.top + Math.min(from[1], to[1])}px`;
    box.style.width = `${Math.abs(to[0] - from[0])}px`;
    box.style.height = `${Math.abs(to[1] - from[1])}px`;
  }

  // Clicking the triad swings the view onto that axis. Checked before
  // anything else: it is an overlay, so it is in front.
  canvas.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || lasso) return;
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
    if (e.button !== 0 || aiming !== null || viewer.aimedAtGeometry || lasso) return;
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
    if (e.button !== 0 || aiming !== null || growing || viewer.aimedAtGeometry || lasso) return;
    if (viewer.axes.pick(e.offsetX, e.offsetY)) return;
    if (viewer.pickNode(e.offsetX, e.offsetY) !== null) return;
    if (viewer.pickEdge(e.offsetX, e.offsetY) !== null) return;
    const surface = viewer.pickSurface(e.offsetX, e.offsetY);
    const [first] = viewer.selection;
    if (!surface || !first || first.face !== surface.face || first.block !== surface.block) {
      return;
    }
    // A face of an open sheet may be dragged either way: it has no
    // inside, and which way it is wound is not something the screen says.
    if (!viewer.beginExtrusion(surface, isPatch(surface))) return;
    growing = true;
    canvas.setPointerCapture(e.pointerId);
    viewer.suspendCamera();
  });

  // --- picking faces, and aiming a cut ------------------------------------
  /** How far the pointer may travel and still count as a click. Dragging from
   *  a face is how you orbit the view, and it must not also select it. */
  const CLICK_SLOP = 4;
  /** The key held to draw a band round handles, matched by its character
   *  so it is the key printed `W` whatever the layout. */
  const LASSO_KEY = "w";
  /** How close to an end a cut may be aimed. The kernel refuses 0 and 1 —
   *  a cut there is not a cut — so the pointer never asks for them. */
  const CUT_MARGIN = 0.02;
  let pressedAt: [number, number] | undefined;
  /** The edge a cut is being aimed along, while the pointer is down on it. */
  let aiming: number | null = null;
  let aimingWasSelected = false;
  /** Whether a face is being pulled out into a block. */
  let growing = false;
  /** Where a rubber band was begun, while the pointer is down on one. */
  let lasso: [number, number] | undefined;
  /**
   * Whether the key that opens a band is held.
   *
   * `W` or shift. A letter rather than a modifier alone because that is
   * the one your left hand is already on, and because it is tracked here
   * rather than read off the event: a window that loses focus mid-gesture
   * would otherwise leave it held for ever.
   */
  let banding = false;
  window.addEventListener("keydown", (e) => {
    if (e.key.toLowerCase() === LASSO_KEY) banding = true;
    if (e.key === "Shift") showChord(true);
  });
  window.addEventListener("keyup", (e) => {
    if (e.key.toLowerCase() === LASSO_KEY) banding = false;
    if (e.key === "Shift") showChord(false);
  });
  // A window that loses focus with a key held never sees the key go up, and
  // a chord still lit over a model nobody is looking at is a lie.
  window.addEventListener("blur", () => {
    banding = false;
    showChord(false);
  });

  canvas.addEventListener("pointermove", (e) => {
    if (lasso) drawLasso(lasso, [e.offsetX, e.offsetY]);
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
      const spot = viewer.pickGeometry(x, y);
      if (spot === null) clearSelection();
      else toggleEntity(spot);
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
    // A face of an open sheet raises the **whole** sheet: half a patch
    // cannot be raised, since a block welded to the other half is not
    // something the map can hold. Same gesture, and the kernel's own
    // answer about which face this is.
    const reply = await send(
      isPatch(from)
        ? { op: "extrude_patch", face: from.face, height }
        : {
            op: "extrude_face",
            face: from.face,
            block: from.block,
            height,
          },
    );
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
    const reply = await send({ op: "split_sheet", edge, at });
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
    const sheet = kernel.sheet(edge);
    if (!sheet.ok) {
      viewer.showSheet([]);
      viewer.selectedEdge = null;
      say(sheet.error);
      return;
    }
    viewer.selectedEdge = edge;
    viewer.showSheet(sheet.edges);
    lit = sheet.segments;
    $<HTMLButtonElement>("remove").disabled = false;
    // The block count, not just the edges: it is what a removal would take
    // away, and taking out more than you meant to looks like a hole. And
    // how many segments it is in, because that is the number `-` and `=`
    // are about to change.
    say(
      `sheet through edge ${edge} — ${sheet.edges.length} edges` +
        ` across ${sheet.blocks} block${sheet.blocks === 1 ? "" : "s"},` +
        ` ${sheet.segments} segments, drag along it to cut`,
    );
  }

  /** Nothing selected: no faces, no sheet. */
  /**
   * Which verbs the two selections make available.
   *
   * Attaching needs nodes to move and one entity to move them to, and a
   * button that tells you so before you press it beats a message that
   * tells you afterwards.
   */
  function showVerbs() {
    $<HTMLButtonElement>("attach").disabled =
      viewer.selectedNodes.length === 0 || viewer.geomSelection.length !== 1;
  }

  function clearSelection() {
    viewer.selectedEdge = null;
    viewer.showSheet([]);
    viewer.selectGeom([]);
    viewer.selectNodes([]);
    showVerbs();
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
    const picked = viewer.selectedNodes;
    const now = picked.includes(name) ? picked.filter((n) => n !== name) : [...picked, name];
    viewer.selectNodes(now);
    sayNodes(now);
  }

  /**
   * What the readout says about the nodes in hand.
   *
   * One of them is worth describing — what it is on, whether it moves —
   * because that is the question you clicked it to ask. Four of them are
   * worth counting, and how many are already on the geometry, because the
   * question then is whether the gesture you are about to make has
   * anything left to do.
   */
  function sayNodes(now: readonly number[]) {
    showVerbs();
    if (now.length === 0) {
      say("");
      return;
    }
    if (now.length === 1) {
      const state = viewer.nodeState(now[0]);
      const on = state?.on ? ` on ${spotName(state.on)}` : " on nothing";
      say(`node ${now[0]}${on}${state?.frozen ? ", frozen" : ""}`);
      return;
    }
    const pinned = now.filter((n) => viewer.nodeState(n)?.on).length;
    say(`${now.length} nodes — ${pinned} on the geometry`);
  }

  /**
   * Add a geometric entity to the selection, or take it out again.
   *
   * Its own list rather than the face selection's: a surface of the
   * geometry and a face of the blocking are different things, and the
   * verbs that act on them are different verbs.
   */
  function toggleEntity(spot: Spot) {
    const same = (a: Spot, b: Spot) => a.dim === b.dim && a.tag === b.tag;
    const picked = viewer.geomSelection;
    const now = picked.some((s) => same(s, spot))
      ? picked.filter((s) => !same(s, spot))
      : [...picked, spot];
    viewer.selectGeom(now);
    showVerbs();
    say(
      now.length === 0
        ? "nothing selected — a box would take the whole geometry"
        : now.length === 1
          ? `${spotName(now[0])} selected` +
            (now[0].dim === 2 ? " — Q lays a quadrangle on it" : "")
          : `${now.length} entities selected`,
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
    // Both selections stay. They used to be cleared on the way through,
    // on the grounds that a selection you cannot see is a mystery — but
    // the two are made to be held at once now: a handful of nodes and the
    // entity to put them on are picked in different modes and used
    // together. `Esc` is how you let go.
  }

  /** How many segments the lit sheet is in, as the kernel last said. */
  let lit = 0;

  /**
   * Make the lit sheet finer or coarser.
   *
   * On the sheet, which is the only thing it can be on: every edge of one
   * has to carry the same number or the mesh has nothing to match along
   * the faces between them. The kernel does the spreading; this says by
   * how much.
   */
  async function resegment(by: number) {
    const edge = viewer.selectedEdge;
    if (edge === null) {
      say("light a sheet first — click an edge");
      return;
    }
    const segments = Math.max(1, lit + by);
    const reply = await send({ op: "set_segments", edge, segments });
    message = reply.message;
    if (reply.ok) lit = segments;
    say(message);
  }

  /** Put the whole blocking on the geometry. */
  async function classify() {
    const reply = await send({ op: "classify", tolerance: prefs.box.snap });
    message = reply.message;
    await refresh();
  }

  /**
   * Put the selected nodes on the selected geometric entity, and move them
   * there.
   *
   * The fast way onto a model, and the only way onto an entity of your
   * choosing: `K` puts the whole blocking on whatever is nearest, which is
   * right until it is not — the four corners of a box round a cylinder are
   * nearer the tube than the circle they are meant to follow.
   *
   * One entity, several nodes: that is the shape of the act. Both
   * selections are let go afterwards, because the next four corners and
   * the next curve are the next gesture.
   */
  /**
   * Lay a quadrangle on the picked surface.
   *
   * The beginning of a surface blocking: the kernel chooses the four
   * corners of the surface the boundary turns a right angle at, and the
   * face comes back classified — corners on corners, sides on the curves
   * between them, face on the surface.
   */
  async function faceOnSurface() {
    const picked = viewer.geomSelection;
    const surface = picked.find((s) => s.dim === 2);
    if (picked.length !== 1 || surface === undefined) {
      say(
        picked.length === 0
          ? "pick a surface first — press M and click one"
          : "one surface at a time: a quadrangle is laid on one",
      );
      return;
    }
    const reply = await send({ op: "face_on_surface", surface: surface.tag });
    message = reply.message;
    if (reply.ok) viewer.selectGeom([]);
    showVerbs();
    // Not framed: the face is laid on the geometry, which is already in
    // view — that is how it was picked.
    await refresh();
  }

  /**
   * Smooth the blocking: the nodes towards the middle of their neighbours,
   * as far as what they follow allows.
   *
   * How many passes and how far each one goes are the two sliders in the
   * settings panel, sent with the command rather than kept by the kernel.
   */
  async function smooth() {
    const reply = await send({
      op: "smooth",
      rounds: prefs.smooth.rounds,
      share: prefs.smooth.share,
    });
    message = reply.message;
    // Not framed: the model is where it was, only tidier.
    await refresh();
  }

  async function attach() {
    const nodes = viewer.selectedNodes;
    const picked = viewer.geomSelection;
    if (nodes.length === 0) {
      say("pick some nodes first — click them, or hold shift and drag a box round them");
      return;
    }
    if (picked.length !== 1) {
      say(
        picked.length === 0
          ? "pick what to put them on — press M and click a curve, a surface or a corner"
          : "one entity at a time: a node goes on one thing",
      );
      return;
    }
    const reply = await send({ op: "classify_node", nodes, on: picked[0] });
    message = reply.message;
    if (reply.ok) {
      viewer.selectNodes([]);
      viewer.selectGeom([]);
    }
    await refresh();
  }

  /**
   * Lock the selected nodes, or let them go.
   *
   * On the selection rather than on a drag, because freezing is a thing
   * you say about a node and dragging is a thing you do to one — and a
   * frozen node cannot be dragged at all, so the gesture would have
   * nothing to attach to.
   */
  async function freezeNode() {
    const nodes = viewer.selectedNodes;
    if (nodes.length === 0) {
      say("pick a node first — click one");
      return;
    }
    // Thawed unless every one of them is frozen, so a mixed selection has
    // an obvious answer: the key makes them agree, and agree the way most
    // of a gesture is meant.
    const all = nodes.every((n) => viewer.nodeState(n)?.frozen);
    const reply = await send({ op: "freeze_node", nodes, frozen: !all });
    message = reply.message;
    await refresh();
  }

  /** Take the selected nodes off the geometry. They stay where they are;
   *  what changes is that they may now go anywhere. */
  async function unclassifyNode() {
    const picked = viewer.selectedNodes;
    if (picked.length === 0) {
      say("pick a node first — click one");
      return;
    }
    const nodes = picked.filter((n) => viewer.nodeState(n)?.on);
    if (nodes.length === 0) {
      say(picked.length === 1 ? `node ${picked[0]} is on nothing already` : "none of them is on anything");
      return;
    }
    const reply = await send({ op: "classify_node", nodes });
    message = reply.message;
    await refresh();
  }

  async function boxGeometry() {
    // The margin is sent, because it is adjustable: the kernel's constant
    // is what you get when nobody says, and the slider in the settings
    // panel is somebody saying.
    const reply = await send({
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
          ? isPatch(picked[0])
            ? `face ${picked[0].face} of a patch — drag it to raise the whole` +
              ` sheet, Del takes the face`
            : `face ${picked[0].face} (block ${picked[0].block}) selected` +
              ` — hold ⇧ for its chord`
          : picked.length === 2
            ? `faces ${picked[0].face} and ${picked[1].face} will be welded` +
              ` where they meet — or pillow both`
            : `${picked.length} faces — pillow takes them all`;
    $("status").textContent = message;
  }

  /**
   * The one selected face, or nothing — with a word about why.
   *
   * A chord is named by one face. Two faces are a glue and several are a
   * pillow, so there is no sensible reading of "the chord of these", and
   * guessing at the first one would act on something nobody pointed at.
   */
  function soleFace(): { face: number; block: number } | null {
    const picked = viewer.selection;
    if (picked.length === 0) {
      say("select a face first — click one");
      return null;
    }
    if (picked.length > 1) {
      say(`a chord runs through one face — ${picked.length} are selected, Esc clears them`);
      return null;
    }
    return { face: picked[0].face, block: picked[0].block };
  }

  /**
   * Light up the chord through the selected face, while ⇧ is held.
   *
   * One preview for both chord operations: what `⇧Del` would take away and
   * what `⇧P` would wrap are the same thread, and the faces it crosses drawn
   * as a ladder of slices is the whole of it. Held rather than toggled,
   * because it answers a question asked in passing.
   *
   * A lit sheet is left alone. It is already showing what a cut would touch,
   * and two threads lit at once would say neither.
   */
  function showChord(on: boolean) {
    if (viewer.selectedEdge !== null) return;
    const picked = viewer.selection;
    if (picked.length !== 1) return;
    if (!on) {
      viewer.showSheet([]);
      return;
    }
    const chord = kernel.chord(picked[0].face, picked[0].block);
    if (!chord.ok) {
      say(chord.error);
      return;
    }
    viewer.showSheet(chord.edges);
    say(
      `chord through face ${picked[0].face} — ${chord.blocks}` +
        ` block${chord.blocks === 1 ? "" : "s"}:` +
        ` ⇧Del takes it out, ⇧P wraps it in an o-grid`,
    );
  }

  /** Take out every block the chord through the selected face crosses. */
  async function removeChord() {
    const pick = soleFace();
    if (!pick) return;
    const reply = await send({ op: "remove_chord", face: pick.face, block: pick.block });
    clearSelection();
    message = reply.message;
    // Not framed: taking something away is no reason to move the camera.
    await refresh();
  }

  /** Lay an o-grid all along that chord. */
  async function pillowChord() {
    const pick = soleFace();
    if (!pick) return;
    const reply = await send({ op: "pillow_chord", face: pick.face, block: pick.block });
    // Every face of the chord is a different cell now, and the lit wire was
    // drawn from the names it had.
    clearSelection();
    message = reply.message;
    // Not framed: the model's surface is where it was, which is the point.
    await refresh();
  }

  async function pillowSelection() {
    const picked = viewer.selection;
    if (!picked.length) return;
    const reply = await send({
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
    const reply = await send({
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
        : first === undefined
          ? null
          : // A face of an open sheet is a face; a face of a block is the
            // way you point at the block. One key, and the kernel's own
            // answer about which it is.
            isPatch(first)
            ? { op: "remove_face", face: first.face }
            : { op: "remove_block", block: first.block };
    if (!command) return;
    const reply = await send(command);
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
    const reply = await send({ op: "load_geometry", msh, name });
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
      const text = await file.text();
      // Kept as well as loaded: from now on a script can open it by name,
      // which is the only way a page on a static host has of opening a
      // file at all.
      files.keep(file.name, text);
      await loadGeometry(file.name, text);
    } catch (problem) {
      say(`could not read ${file.name}: ${problem}`);
    }
  });

  /**
   * Files dropped on the window are kept, by name.
   *
   * The answer to "how do I open a file from a script here": hand it to
   * the page first. A browser cannot be given a path, and the picker that
   * could ask for one is asynchronous — Lua asks for a file in the middle
   * of a loop and cannot wait. A file that is already here needs no
   * waiting.
   *
   * A single `.msh` is also opened as the geometry, because that is
   * plainly what dropping one means.
   */
  async function takeFiles(dropped: File[]) {
    if (dropped.length === 0) return;
    const names: string[] = [];
    for (const file of dropped) {
      try {
        files.keep(file.name, await file.text());
        names.push(file.name);
      } catch (problem) {
        say(`could not read ${file.name}: ${problem}`);
        return;
      }
    }
    const only = dropped.length === 1 ? dropped[0] : undefined;
    if (only && only.name.toLowerCase().endsWith(".msh")) {
      files.keep(only.name, await only.text());
      await loadGeometry(only.name, await only.text());
      return;
    }
    message =
      `${names.join(", ")} ready — a script can open ` +
      `${names.length === 1 ? "it" : "them"} by name`;
    $("status").textContent = message;
  }

  for (const kind of ["dragenter", "dragover"]) {
    window.addEventListener(kind, (e) => {
      e.preventDefault();
      document.body.dataset.dropping = "1";
    });
  }
  for (const kind of ["dragleave", "drop"]) {
    window.addEventListener(kind, () => delete document.body.dataset.dropping);
  }
  window.addEventListener("drop", (e) => {
    const dropped = [...((e as DragEvent).dataTransfer?.files ?? [])];
    if (dropped.length === 0) return;
    e.preventDefault();
    void takeFiles(dropped);
  });

  /** Put everything back in view. Camera state, so it never goes near the
   *  kernel: in station mode two tabs on one session each have their own. */
  function recentre() {
    viewer.frameAll();
    say("view recentred");
  }

  /**
   * Every command the interface sends, in one place.
   *
   * The kernel hands back the line of Lua that would have made it, and
   * this is where that line reaches the console — so what you did with the
   * mouse and what you typed end up in the same history, in order.
   */
  async function send(command: Record<string, unknown>) {
    const reply = kernel.apply(command);
    if (reply.script) echo("did", reply.script);
    return reply;
  }

  // --- the console ---------------------------------------------------------
  /** Started the first time it is opened: the interpreter is two thirds of
   *  a megabyte, and a page nobody scripts should not pay for it. */
  let lua: Interpreter | undefined;
  const typed: string[] = [];
  let walking = 0;

  function echo(kind: "said" | "did" | "was" | "bad", text: string) {
    if (!text) return;
    const line = document.createElement("p");
    line.className = kind;
    line.textContent = (kind === "said" ? "> " : "") + text;
    $("console-out").append(line);
    $("console-out").scrollTop = $("console-out").scrollHeight;
  }

  async function showConsole(open: boolean) {
    document.body.classList.toggle("console-open", open);
    $("console-open").setAttribute("aria-pressed", String(open));
    if (!open) return;
    $("console-in").focus();
    if (lua) return;
    const direct = kernel.direct;
    if (!direct) {
      echo("bad", "the console needs the kernel in this tab");
      return;
    }
    echo("did", "loading Lua…");
    // Files, when the page was served by `gecko` on this machine; a
    // download when it was not.
    lua = await Interpreter.open(direct, files);
    echo("did", "ready — gecko.stats() to look, gecko.script() for the history");
  }

  /** Run what was typed, and show what came of it. */
  async function runLua(source: string) {
    if (!source.trim() || !lua) return;
    typed.push(source);
    walking = typed.length;
    echo("said", source);
    const answer: Answer = lua.run(source);
    if (answer.out) echo("was", answer.out.replace(/\n$/, ""));
    if (answer.ok) {
      if (answer.value !== undefined) echo("was", answer.value);
    } else {
      echo("bad", answer.error ?? "it went wrong");
    }
    // A script is any number of commands; the scene catches up once, at
    // the end of it.
    await refresh();
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
    // Which components are open sheets of faces. Asked once a redraw
    // rather than once a pick: it is a short list, and both the verbs and
    // the status line want it.
    patchIds = new Set(stats.patches === 0 ? [] : kernel.patches());
    // Only when it has actually changed. The geometry is the big buffer and
    // the one that never moves; re-sending it because a node was dragged is
    // the whole cost this generation number exists to avoid.
    if (stats.geometry.generation !== geometryDrawn) {
      geometryDrawn = stats.geometry.generation;
      viewer.setGeometry(decodeGeometry(kernel.geometry()));
    }
    // The mesh on the same terms, and for the same reason: it is the big
    // buffer, and the number counts the edits that threw one away as well
    // as the builds, so this is also how a stale picture goes.
    if (stats.mesh.generation !== meshDrawn) {
      meshDrawn = stats.mesh.generation;
      viewer.setMesh(stats.mesh.nodes === undefined ? null : decodeMesh(kernel.mesh()));
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
    // After the view: a cut can consume a node, and the selection is
    // filtered to what the view still holds as it arrives.
    showVerbs();
    showHistory(stats);
    // One line, out of the way. The counts are what you glance at; the mode
    // matters once, when you are working out which kernel you are talking to.
    $("stats").innerHTML =
      // Which gecko, before what is in it: the counts change every edit
      // and this does not, so it reads as a label rather than as a number
      // you have to keep looking at.
      `<span id="who">gecko-rs <b data-test="version">${kernel.version()}</b></span>` +
      `<span><b data-test="blocks">${stats.blocks}</b> blocks</span>` +
      // Only once there is one: a counter at nought for something nobody
      // has made is a question in the corner of the window.
      (stats.patches === 0
        ? ""
        : `<span><b data-test="patches">${stats.patches}</b> patches</span>`) +
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
      (stats.mesh.hexes === undefined
        ? ""
        : `<span><b data-test="mesh-hexes">${stats.mesh.hexes}</b> hexahedra</span>`) +
      "";
    $("status").textContent = message;
    revision++;
  }

  async function createCube() {
    const stats = kernel.stats();
    const reply = await send({
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
    const reply = await send({ op: "undo" });
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
    await send({ op: "set_history", steps: prefs.history });
    showHistory(kernel.stats());
  }

  /** The undo button is live exactly when there is something to undo. */
  function showHistory(stats: Stats) {
    $<HTMLButtonElement>("undo").disabled = stats.history.back === 0;
  }

  /**
   * Discretise every block, and draw the skin of what comes out.
   *
   * The mesh is the kernel's, not the viewer's: it is built there, kept
   * there until the next edit throws it away, and sent here as its skin —
   * the inside of a mesh being most of it and never seen.
   */
  async function mesh() {
    say("meshing…");
    const reply = await send({ op: "mesh", segments: prefs.segments });
    message = reply.message;
    await refresh();
  }

  /**
   * Write the mesh out, as VTK legacy.
   *
   * Meshes first if nothing has been meshed since the last edit, so the
   * file is always of the blocking as it now stands rather than of
   * whatever it looked like when somebody last pressed the other key.
   */
  async function exportMesh() {
    const stats = kernel.stats();
    if (stats.mesh.nodes === undefined) await mesh();
    const file = kernel.vtk();
    if (file === null) {
      say("there is nothing to mesh");
      return;
    }
    saveBlob(
      new Blob([file], { type: "text/plain" }),
      fileName(new Date(), "vtk"),
      document,
    );
    say(`${message} — written out`);
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
      // Which gecko this is. The card is where anybody looks for it, and
      // the number comes from the kernel rather than from a string here,
      // so it cannot disagree with the release it was built from.
      `<h1 id="help-who">gecko-rs <span>${kernel.version()}</span></h1>` +
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
      // In the columns with the key groups rather than under them: the
      // browser balances what it is given, and the legend is one more
      // thing to balance.
      legend() +
      `</div>` +
      `<p id="help-hint">Click a node, an edge or a face to pick it;` +
      ` hold <kbd>W</kbd> and drag to catch every handle in a band;` +
      ` <kbd>M</kbd> aims at the geometry instead. Any key closes this.</p>`;
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
    attach,
    mesh,
    export: exportMesh,
    finer: () => resegment(1),
    coarser: () => resegment(-1),
    freeze: freezeNode,
    unclassify: unclassifyNode,
    glue: glueSelection,
    remove: removeSelection,
    pillow: pillowSelection,
    smooth,
    "face-on-surface": faceOnSurface,
    "remove-chord": removeChord,
    "pillow-chord": pillowChord,
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
    console: () => void showConsole(!document.body.classList.contains("console-open")),
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
    ["attach", "attach"],
    ["mesh", "mesh"],
    ["glue", "glue"],
    ["remove", "remove"],
    ["pillow", "pillow"],
    ["undo", "undo"],
    ["quad", "quad"],
    ["recentre", "recentre"],
    ["shot", "screenshot"],
    ["display", "drawer"],
    ["settings", "params"],
    ["console-open", "console"],
    ["help-open", "help"],
  ] as [string, Action][]) {
    $(id).addEventListener("click", () => run(action));
  }

  // --- the console's own keys ---------------------------------------------
  //
  // Enter runs, shift-enter makes a line, and the arrows walk what you have
  // typed — the three things every console does and that are noticed only
  // when one of them is missing.
  $("console-in").addEventListener("keydown", (e) => {
    const event = e as KeyboardEvent;
    const field = $<HTMLTextAreaElement>("console-in");
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      const source = field.value;
      field.value = "";
      field.rows = 1;
      void runLua(source);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      void showConsole(false);
      return;
    }
    if ((event.key === "ArrowUp" || event.key === "ArrowDown") && typed.length) {
      // Only when the line is a single one: inside a block, the arrows are
      // how you move about in it.
      if (field.value.includes("\n")) return;
      event.preventDefault();
      walking = Math.min(typed.length, Math.max(0, walking + (event.key === "ArrowUp" ? -1 : 1)));
      field.value = typed[walking] ?? "";
      return;
    }
    // Grow with what is in it, up to the cap the stylesheet sets.
    requestAnimationFrame(() => {
      field.rows = Math.min(8, field.value.split("\n").length);
    });
  });
  $("console-clear").addEventListener("click", () => {
    $("console-out").replaceChildren();
    $("console-in").focus();
  });
  $("console-save").addEventListener("click", () => {
    const script = kernel.script();
    saveBlob(new Blob([script], { type: "text/plain" }), fileName(new Date(), "lua"), document);
    say("the session written out as a script");
  });

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
  viewer.onDrop(async (name, to, carrying) => {
    // With the reach the settings panel asks for: a node dropped near an
    // entity is taken by it, and a reach of nothing means it is not.
    const reply = await send({
      op: "move_node",
      node: name,
      to,
      // Whatever else was picked comes along by the same offset — one
      // command, so one step of undo for one gesture.
      carrying,
      snap: prefs.box.snap,
      // And the neighbours give way: this is a gesture, and a gesture is
      // where the ripple belongs. A script placing a node says nothing of
      // the kind and gets the plain move.
      smooth: true,
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
    stats: () => kernel.stats(),
    version: () => kernel.version(),
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
    geomSelection: () => viewer.geomSelection,
    selectGeom: (spots: Spot[]) => viewer.selectGeom(spots),
    pickGeometry: (x: number, y: number) => viewer.pickGeometry(x, y),
    geomCount: (group: GeomGroup) => viewer.geomCount(group),
    skinCount: () => viewer.skinCount,
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
    selectedNodes: () => viewer.selectedNodes,
    selectNodes: (names: number[]) => viewer.selectNodes(names),
    /** What a selection made from outside the pointer would have done to
     *  the toolbar. The gestures call it themselves. */
    showVerbs: () => showVerbs(),
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
