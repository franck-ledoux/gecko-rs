/**
 * Saving a picture of the scene.
 *
 * Rendered to an offscreen target rather than read off the canvas, so the
 * size is ours to choose: a figure wants more pixels than a window has. The
 * interface is never in the picture either way — it is DOM over the canvas,
 * and this does not go near the canvas at all.
 */

import { CreateScreenshotUsingRenderTargetAsync } from "@babylonjs/core/Misc/screenshotTools";
import type { AbstractEngine } from "@babylonjs/core/Engines/abstractEngine";
import type { Camera } from "@babylonjs/core/Cameras/camera";
import { Color4 } from "@babylonjs/core/Maths/math.color";

export interface ShotOptions {
  /** 2 gives twice the pixels of the canvas in each direction. */
  scale: number;
  /** Drop the background, for a figure that has to sit on a page of unknown
   *  colour. */
  transparent: boolean;
}

/** `gecko-2026-10-09-183012.png`: sorts by date and never collides. */
export function shotName(at: Date): string {
  const two = (n: number) => String(n).padStart(2, "0");
  return (
    `gecko-${at.getFullYear()}-${two(at.getMonth() + 1)}-${two(at.getDate())}` +
    `-${two(at.getHours())}${two(at.getMinutes())}${two(at.getSeconds())}.png`
  );
}

/**
 * Render the scene to a PNG, and hand back a data URL.
 *
 * The clear colour is restored afterwards whatever happens: a failed capture
 * that left the scene transparent would be a strange thing to debug.
 */
export async function capture(
  engine: AbstractEngine,
  camera: Camera,
  options: ShotOptions,
): Promise<string> {
  const scene = camera.getScene();
  const was = scene.clearColor;
  if (options.transparent) {
    scene.clearColor = new Color4(was.r, was.g, was.b, 0);
  }
  try {
    // `true` asks for the canvas rather than whatever render target happens
    // to be bound, and the scale multiplies **that**: the picture you are
    // looking at, drawn with so many times the pixels. On a high-density
    // display the canvas is already twice its CSS size, so a scale of 2 is a
    // generously large figure.
    return await CreateScreenshotUsingRenderTargetAsync(engine, camera, {
      width: Math.round(engine.getRenderWidth(true) * options.scale),
      height: Math.round(engine.getRenderHeight(true) * options.scale),
    });
  } finally {
    scene.clearColor = was;
  }
}

/** The bytes behind a `data:` URL. */
export function dataUrlToBlob(dataUrl: string): Blob {
  const [head, body] = dataUrl.split(",");
  const type = head.slice(5).replace(";base64", "") || "application/octet-stream";
  const binary = atob(body);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return new Blob([bytes], { type });
}

/**
 * Hand the picture to the browser as a download.
 *
 * Through a blob rather than the data URL itself: a 4× picture is several
 * megabytes, and that is a several-megabyte string to put in an attribute
 * and parse back out again.
 */
export function save(dataUrl: string, name: string, document: Document): void {
  const url = URL.createObjectURL(dataUrlToBlob(dataUrl));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  // Firefox will not follow a link that is not in the document.
  document.body.appendChild(link);
  link.click();
  link.remove();
  // Not immediately: the click is handled asynchronously, and revoking the
  // url first gives a download of nothing at all.
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}
