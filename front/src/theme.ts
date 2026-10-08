/**
 * How the scene looks, as against what it contains.
 *
 * This is the one piece of state in the project that deliberately does **not**
 * go through the kernel. The kernel describes what exists; it has no business
 * knowing what colour you like it. So a theme is per viewer, kept in the
 * browser, and two tabs on one station session may legitimately look nothing
 * alike.
 *
 * Everything is driven off the `Theme` keys: the settings panel is generated
 * from them, so adding a colour later — a selected node, a face, a mesh — costs
 * one entry here and nothing in the interface.
 */

export interface Theme {
  background: string;
  nodes: string;
  edges: string;
}

/** Order and labels for the generated settings panel. */
export const SWATCHES: ReadonlyArray<{ key: keyof Theme; label: string }> = [
  { key: "background", label: "fond" },
  { key: "nodes", label: "sommets" },
  { key: "edges", label: "arêtes" },
];

export const PRESETS: Record<"dark" | "light", Theme> = {
  dark: { background: "#15171b", nodes: "#66b2ff", edges: "#f2f2ff" },
  light: { background: "#f4f5f7", nodes: "#1763b8", edges: "#2b2f37" },
};

const KEY = "gecko.view";

export interface ViewPrefs {
  theme: Theme;
  axes: boolean;
}

export const DEFAULTS: ViewPrefs = { theme: { ...PRESETS.dark }, axes: true };

/**
 * Read the stored preferences, falling back to the defaults for anything
 * missing or unreadable.
 *
 * Storage can throw outright — a private window, or a browser told to block
 * site data — and it can also hold something written by an older version. Both
 * are ordinary, so neither is allowed to stop the viewer from opening.
 */
export function loadPrefs(): ViewPrefs {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return structuredClone(DEFAULTS);
    const stored = JSON.parse(raw) as Partial<ViewPrefs>;
    return {
      theme: { ...DEFAULTS.theme, ...(stored.theme ?? {}) },
      axes: typeof stored.axes === "boolean" ? stored.axes : DEFAULTS.axes,
    };
  } catch {
    return structuredClone(DEFAULTS);
  }
}

export function savePrefs(prefs: ViewPrefs): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(prefs));
  } catch {
    // Not being able to remember a colour is not worth interrupting anyone.
  }
}
