/**
 * Which mode the page is in: the one decision that differs between the two
 * deployments, kept apart from the boot that acts on it so it can be tested
 * without a browser, a kernel or a station.
 */

/**
 * What an answer to the `mode` probe means.
 *
 * Three answers, not two, and the third is the one that matters. `/mode` is
 * forwarded to the station by the dev server, so a 5xx is not "there is no
 * station here" — it is "a station is meant to be here and is not answering
 * yet", which is exactly what a page opened before the Python session sees.
 * Reading that as "no station" starts a kernel of our own, and the page then
 * works perfectly while quietly ignoring every command typed in Python.
 *
 * A static host that falls back to `index.html` for unknown paths would make a
 * plain "did it 404?" test say yes, so a success is checked for its content.
 */
export type Probe = "station" | "static" | "waiting";

export function readProbe(status: number, body: string): Probe {
  // 0 is a fetch that threw: the page came from this origin, so its own
  // sibling path failing means the server in front of us is in trouble.
  if (status === 0 || status >= 500) return "waiting";
  if (status < 400 && body.trim() === "station") return "station";
  return "static";
}
