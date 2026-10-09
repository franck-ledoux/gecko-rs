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
  /** The kernel's stable name for each node, in the same order.
   *  A position in this array is not a name: edits reorder it. */
  names: Uint32Array;
  /** Two node indices per edge. */
  edges: Uint32Array;
  /** Four node indices per quadrangle, wound outward.
   *
   *  One quadrangle per (face, block) pair — the same surface is what gets
   *  drawn for faces and for blocks, and only the name it is coloured by
   *  differs. */
  quads: Uint32Array;
  /** The face each quadrangle belongs to. */
  quadFaces: Uint32Array;
  /** The block each quadrangle belongs to. */
  quadBlocks: Uint32Array;
}

const MAGIC = 0x3142_4b47; // "GKB1" little-endian
const HEADER = 44;

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
  if (version !== 3) throw new Error(`view buffer: version ${version} not understood`);

  const centre: [number, number, number] = [
    head.getFloat64(8, true),
    head.getFloat64(16, true),
    head.getFloat64(24, true),
  ];
  const nNodes = head.getUint32(32, true);
  const nEdges = head.getUint32(36, true);
  const nQuads = head.getUint32(40, true);

  // Check the declared sizes before making any view, so a truncated buffer is
  // a clear error here rather than an opaque one three lines down.
  // Per node: three f32 of position and one u32 of name. Per quadrangle: four
  // corner indices and the two names it can be coloured by.
  const expected = HEADER + nNodes * 16 + nEdges * 8 + nQuads * 24;
  if (expected !== bytes.byteLength) {
    throw new Error(
      `view buffer: header declares ${expected} bytes, buffer holds ${bytes.byteLength}`,
    );
  }

  const base = bytes.byteOffset + HEADER;
  const afterEdges = base + nNodes * 16 + nEdges * 8;
  return {
    centre,
    positions: new Float32Array(bytes.buffer, base, nNodes * 3),
    names: new Uint32Array(bytes.buffer, base + nNodes * 12, nNodes),
    edges: new Uint32Array(bytes.buffer, base + nNodes * 16, nEdges * 2),
    quads: new Uint32Array(bytes.buffer, afterEdges, nQuads * 4),
    quadFaces: new Uint32Array(bytes.buffer, afterEdges + nQuads * 16, nQuads),
    quadBlocks: new Uint32Array(bytes.buffer, afterEdges + nQuads * 20, nQuads),
  };
}
