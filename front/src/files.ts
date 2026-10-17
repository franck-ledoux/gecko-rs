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

/** The file at this path, or null — no such file, or no server to ask. */
export function read(path: string): string | null {
  try {
    const ask = new XMLHttpRequest();
    ask.open("GET", `file?path=${encodeURIComponent(path)}`, false);
    ask.send();
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
    if (ask.status === 200) return "";
    if (ask.status !== 404) return ask.responseText || `could not write ${path}`;
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
