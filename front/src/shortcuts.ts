/**
 * The keyboard, as one table.
 *
 * The same list drives what a key does and what the help card says it does,
 * so the two cannot drift. A shortcut nobody can find is the same as no
 * shortcut, and a help card that lies is worse than none.
 */

/**
 * Everything the interface can be asked to do.
 *
 * A list rather than a bare union, so it can be walked at run time: the test
 * below checks that each one has a key, and `Record<Action, …>` in `main.ts`
 * makes the compiler check that each one has an implementation. Adding an
 * action without a key, or without a handler, stops being possible.
 */
export const ACTIONS = [
  "cube",
  "undo",
  "open",
  "box",
  "picking",
  "classify",
  "attach",
  "mesh",
  "export",
  "finer",
  "coarser",
  "freeze",
  "unclassify",
  "glue",
  "remove",
  "pillow",
  "remove-chord",
  "pillow-chord",
  "clear",
  "recentre",
  "screenshot",
  "axis-x",
  "axis-y",
  "axis-z",
  "axis--x",
  "axis--y",
  "axis--z",
  "faces",
  "blocks",
  "edges",
  "internal",
  "corners",
  "curves",
  "surfaces",
  "quad",
  "triad",
  "theme",
  "drawer",
  "params",
  "console",
  "chrome",
  "help",
] as const;

export type Action = (typeof ACTIONS)[number];

export interface Shortcut {
  /** What the help card prints on the key cap. */
  show: string;
  does: string;
  action: Action;
  group: "Edit" | "View" | "Display" | "Window";
  /** Matched against `event.key`, lower-cased. */
  key?: string;
  /**
   * Needs ctrl, or ⌘ on a Mac.
   *
   * The one shortcut that takes a modifier, and it takes one because it is
   * the one everybody already knows: ⌘Z is undo in every application anyone
   * has ever used, and binding it to a bare letter instead would be a
   * surprise with nothing to gain. Everything else is left to the browser,
   * which is what the test below checks.
   */
  mod?: boolean;
  /**
   * Matched against `event.code` instead.
   *
   * For keys whose character moves with the layout. On a French keyboard the
   * digit row is shifted — `event.key` for the key printed `1` is `&` — so
   * matching the character would make the help card a lie there, while the
   * physical position is the same key under the same printed digit.
   */
  code?: string;
  shift?: boolean;
}

export const SHORTCUTS: readonly Shortcut[] = [
  { show: "C", key: "c", action: "cube", does: "create a cube", group: "Edit" },
  { show: "O", key: "o", action: "open", does: "open a geometry (.msh)", group: "Edit" },
  {
    show: "M",
    key: "m",
    action: "picking",
    does: "aim at the geometry, or back at the blocks",
    group: "Edit",
  },
  {
    show: "B",
    key: "b",
    action: "box",
    does: "box the selected surfaces, or the whole geometry",
    group: "Edit",
  },
  { show: "G", key: "g", action: "glue", does: "glue the two selected faces", group: "Edit" },
  {
    show: "Del",
    key: "delete",
    action: "remove",
    does: "remove the selected block, or the lit sheet",
    group: "Edit",
  },
  { show: "⌫", key: "backspace", action: "remove", does: "", group: "Edit" },
  { show: "P", key: "p", action: "pillow", does: "pillow the selected faces", group: "Edit" },
  {
    show: "⇧Del",
    key: "delete",
    shift: true,
    action: "remove-chord",
    does: "remove the whole chord of blocks through the selected face",
    group: "Edit",
  },
  { show: "⇧⌫", key: "backspace", shift: true, action: "remove-chord", does: "", group: "Edit" },
  {
    show: "⇧P",
    key: "p",
    shift: true,
    action: "pillow-chord",
    does: "o-grid all along that chord",
    group: "Edit",
  },
  {
    show: "K",
    key: "k",
    action: "classify",
    does: "put the whole blocking on the geometry",
    group: "Edit",
  },
  {
    show: "A",
    key: "a",
    action: "attach",
    does: "put the selected nodes on the selected entity",
    group: "Edit",
  },
  {
    show: "L",
    key: "l",
    action: "freeze",
    does: "lock the selected node, or let it go",
    group: "Edit",
  },
  {
    show: "U",
    key: "u",
    action: "unclassify",
    does: "take the selected node off the geometry",
    group: "Edit",
  },
  {
    show: "⌘Z",
    key: "z",
    mod: true,
    action: "undo",
    does: "take back the last change (ctrl Z on a PC)",
    group: "Edit",
  },
  {
    show: "E",
    key: "e",
    action: "mesh",
    does: "discretise every block",
    group: "Edit",
  },
  {
    show: "⇧E",
    key: "e",
    shift: true,
    action: "export",
    does: "write the mesh out, as VTK",
    group: "Edit",
  },
  {
    show: "=",
    key: "=",
    action: "finer",
    does: "one more segment along the lit sheet",
    group: "Edit",
  },
  { show: "-", key: "-", action: "coarser", does: "one fewer", group: "Edit" },
  { show: "Esc", key: "escape", action: "clear", does: "clear the selection", group: "Edit" },

  { show: "F", key: "f", action: "recentre", does: "frame everything", group: "View" },
  { show: "X", key: "x", action: "axis-x", does: "look down X", group: "View" },
  { show: "Y", key: "y", action: "axis-y", does: "look down Y", group: "View" },
  { show: "Z", key: "z", action: "axis-z", does: "look down Z", group: "View" },
  {
    show: "⇧X",
    key: "x",
    shift: true,
    action: "axis--x",
    does: "look down −X, and so for Y and Z",
    group: "View",
  },
  { show: "⇧Y", key: "y", shift: true, action: "axis--y", does: "", group: "View" },
  { show: "⇧Z", key: "z", shift: true, action: "axis--z", does: "", group: "View" },
  {
    show: "V",
    key: "v",
    action: "quad",
    does: "split the window into 3D, X, Y and Z",
    group: "View",
  },
  { show: "T", key: "t", action: "triad", does: "show the X/Y/Z triad", group: "View" },
  { show: "D", key: "d", action: "theme", does: "dark or light", group: "View" },

  { show: "1", code: "Digit1", action: "faces", does: "show faces", group: "Display" },
  { show: "2", code: "Digit2", action: "blocks", does: "show blocks", group: "Display" },
  { show: "3", code: "Digit3", action: "edges", does: "show edges", group: "Display" },
  { show: "I", key: "i", action: "internal", does: "show internal faces", group: "Display" },
  { show: "4", code: "Digit4", action: "surfaces", does: "show geometric surfaces", group: "Display" },
  { show: "5", code: "Digit5", action: "curves", does: "show geometric curves", group: "Display" },
  { show: "6", code: "Digit6", action: "corners", does: "show geometric corners", group: "Display" },

  { show: "Tab", key: "tab", action: "drawer", does: "how it is drawn", group: "Window" },
  { show: ",", key: ",", action: "params", does: "what the operations do", group: "Window" },
  { show: ";", key: ";", action: "console", does: "the script console", group: "Window" },
  { show: "⇧Tab", key: "tab", shift: true, action: "chrome", does: "hide everything", group: "Window" },
  { show: "S", key: "s", action: "screenshot", does: "save a picture", group: "Window" },
  { show: "H", key: "h", action: "help", does: "this card", group: "Window" },
  { show: "?", key: "?", action: "help", does: "", group: "Window" },
];

/** Where a key event came from, as much of it as matching needs. */
export interface KeyPress {
  key: string;
  code: string;
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  /** The element the event was aimed at, if there was one. */
  target?: { tagName?: string; isContentEditable?: boolean } | null;
}

/**
 * What a key press asks for, or nothing.
 *
 * Two things are deliberately left alone. A combination nothing in the table
 * claims belongs to the browser or the system — a shortcut that swallowed ⌘S
 * would be a bug, not a feature — and alt is left out of the table
 * altogether, being how several keyboards type the characters above the
 * digits. And anything typed into a field is typing: the colour inputs and
 * the sliders both take arrow keys and characters, and a slider that jumped
 * to a new view instead of nudging would be unusable.
 */
export function actionFor(press: KeyPress): Action | null {
  if (press.altKey) return null;
  if (isTyping(press.target)) return null;

  // Ctrl and ⌘ are one thing here. The table says which shortcuts want it,
  // and a press that has it can only match one of those: ⌘Z is undo and Z
  // is a view down the Z axis, and neither is ever the other.
  const held = press.ctrlKey || press.metaKey;
  const key = press.key.toLowerCase();
  for (const shortcut of SHORTCUTS) {
    if (!!shortcut.mod !== held) continue;
    if (!!shortcut.shift !== press.shiftKey) continue;
    if (shortcut.code ? shortcut.code === press.code : shortcut.key === key) {
      return shortcut.action;
    }
  }
  return null;
}

function isTyping(target: KeyPress["target"]): boolean {
  if (!target) return false;
  if (target.isContentEditable) return true;
  const tag = (target.tagName ?? "").toLowerCase();
  return tag === "input" || tag === "textarea" || tag === "select";
}

/** The table, grouped, for the help card to print. Rows with nothing to say
 *  are folded onto the one above them. */
export function helpRows(): { group: Shortcut["group"]; keys: string[]; does: string }[] {
  const rows: { group: Shortcut["group"]; keys: string[]; does: string }[] = [];
  for (const shortcut of SHORTCUTS) {
    if (!shortcut.does && rows.length) {
      rows[rows.length - 1].keys.push(shortcut.show);
      continue;
    }
    rows.push({ group: shortcut.group, keys: [shortcut.show], does: shortcut.does });
  }
  return rows;
}
