/**
 * Files, as a page can reach them.
 *
 * A page served from anywhere has none: a browser cannot open a path, and
 * should not be able to. A page served by `gecko` on your own machine can,
 * through two endpoints it lends — and those are the only circumstances in
 * which any of this answers.
 *
 * **Synchronously, and that is the whole difficulty.** Lua asks for a file
 * in the middle of a loop; the call cannot be unwound into a promise. So
 * these use `XMLHttpRequest` with `async: false`, which is deprecated on
 * the main thread and still the only synchronous door a browser has. The
 * alternative is Asyncify, which doubles the size of the interpreter and
 * slows every call in it to make two of them convenient.
 */

import { saveBlob } from "./capture";

/**
 * The header `gecko` puts on its answers, and the only reliable way to
 * know one when you see it.
 *
 * A static host answers an unknown path with **the application itself**,
 * status and all — so without this a script asking for `cylinder.msh` on
 * the published page was handed the page's own HTML and passed it to the
 * kernel as a gmsh mesh. The status cannot tell them apart either: a
 * missing file's 404 and a static host's 404 are the same 404.
 */
const FROM_GECKO = "x-gecko-file";

/**
 * The files the reader has handed this page, by name.
 *
 * A browser cannot open `/home/you/cylinder.msh`, and should not be able
 * to: that is a boundary, not a gap. What it *can* have is a file someone
 * gave it — chosen with the button, or dropped on the window — and from
 * then on a script can open that file **by its name**, exactly as it would
 * in a terminal.
 *
 * Which is why they are kept rather than read and thrown away. The modern
 * picker would be the other answer and cannot be: it is asynchronous, and
 * Lua asks for a file in the middle of a loop, with no way to hand the
 * event loop back.
 */
const given = new Map<string, string>();

/** Keep what the reader handed over, under the name it came with. */
export function keep(name: string, text: string): void {
  given.set(name, text);
}

/** The names this page is holding, in the order they arrived. */
export function kept(): string[] {
  return [...given.keys()];
}

/** The last part of a path, whichever kind of slash it uses. */
function basename(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/** The file at this path, or null — no such file, or no server to ask. */
export function read(path: string): string | null {
  // What the reader gave the page comes first, by its name and then by the
  // last part of whatever path the script wrote: a script that says
  // `data/cylinder.msh` means the cylinder, and on a page there is no
  // `data/` for it to be wrong about.
  const here = given.get(path) ?? given.get(basename(path));
  if (here !== undefined) return here;
  try {
    const ask = new XMLHttpRequest();
    ask.open("GET", `file?path=${encodeURIComponent(path)}`, false);
    ask.send();
    if (ask.getResponseHeader(FROM_GECKO) === null) return null;
    return ask.status === 200 ? ask.responseText : null;
  } catch {
    return null;
  }
}

/**
 * Put a file there, or hand the browser a download.
 *
 * The fallback is not a consolation prize: a page on a static host has
 * nowhere to write, and a download is where a browser puts things. What
 * matters is that the script does not fail over it.
 */
export function write(path: string, text: string): string {
  try {
    const ask = new XMLHttpRequest();
    ask.open("PUT", `file?path=${encodeURIComponent(path)}`, false);
    ask.send(text);
    // Again, only an answer that says it came from `gecko` counts as one:
    // a host that accepts a PUT it does not understand would otherwise
    // look like a file written.
    if (ask.getResponseHeader(FROM_GECKO) !== null) {
      if (ask.status === 200) return "";
      if (ask.status !== 404) return ask.responseText || `could not write ${path}`;
    }
  } catch {
    // No server at all: the page came from a static host.
  }
  saveBlob(
    new Blob([text], { type: "text/plain" }),
    path.split(/[\\/]/).pop() || "gecko.txt",
    document,
  );
  return "";
}
