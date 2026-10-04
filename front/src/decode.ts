/**
 * Decoder for the kernel's view buffer.
 *
 * The mirror of `ViewBuffer::to_bytes` in `gecko-cmd/src/view.rs`. Keep the two
 * in step: the Rust test `the_encoding_is_the_layout_a_client_will_decode` and
 * the Vitest one below fail loudly if they drift.
 *
 * Both modes deliver these same bytes, so this is the only decoder there is.
 */

export interface View {
  /** f64 offset the kernel subtracted; add it back for model coordinates. */
  centre: [number, number, number];
  /** Three f32 per node, relative to `centre`. */
  positions: Float32Array;
  /** Two node indices per edge. */
  edges: Uint32Array;
}

const MAGIC = 0x3142_4b47; // "GKB1" little-endian
const HEADER = 40;

export function decodeView(bytes: Uint8Array): View {
  if (bytes.byteLength < HEADER) {
    throw new Error(`view buffer: ${bytes.byteLength} bytes is shorter than a header`);
  }
  // Typed-array views need alignment; wasm-bindgen usually hands us a fresh
  // buffer, but a WebSocket frame need not be so obliging.
  if (bytes.byteOffset % 8 !== 0) bytes = new Uint8Array(bytes);

  const head = new DataView(bytes.buffer, bytes.byteOffset, HEADER);
  if (head.getUint32(0, true) !== MAGIC) throw new Error("view buffer: bad magic");
  const version = head.getUint32(4, true);
  if (version !== 1) throw new Error(`view buffer: version ${version} not understood`);

  const centre: [number, number, number] = [
    head.getFloat64(8, true),
    head.getFloat64(16, true),
    head.getFloat64(24, true),
  ];
  const nNodes = head.getUint32(32, true);
  const nEdges = head.getUint32(36, true);

  // Check the declared sizes before making any view, so a truncated buffer is
  // a clear error here rather than an opaque one three lines down.
  const expected = HEADER + nNodes * 12 + nEdges * 8;
  if (expected !== bytes.byteLength) {
    throw new Error(
      `view buffer: header declares ${expected} bytes, buffer holds ${bytes.byteLength}`,
    );
  }

  const base = bytes.byteOffset + HEADER;
  return {
    centre,
    positions: new Float32Array(bytes.buffer, base, nNodes * 3),
    edges: new Uint32Array(bytes.buffer, base + nNodes * 12, nEdges * 2),
  };
}
