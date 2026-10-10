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
  /** The kernel's stable name for each edge, in the same order. */
  edgeNames: Uint32Array;
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
  if (version !== 4) throw new Error(`view buffer: version ${version} not understood`);

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
  // Per node: three f32 of position and one u32 of name. Per edge: two node
  // indices and a name. Per quadrangle: four corner indices and the two names
  // it can be coloured by.
  const expected = HEADER + nNodes * 16 + nEdges * 12 + nQuads * 24;
  if (expected !== bytes.byteLength) {
    throw new Error(
      `view buffer: header declares ${expected} bytes, buffer holds ${bytes.byteLength}`,
    );
  }

  const base = bytes.byteOffset + HEADER;
  const afterEdges = base + nNodes * 16 + nEdges * 12;
  return {
    centre,
    positions: new Float32Array(bytes.buffer, base, nNodes * 3),
    names: new Uint32Array(bytes.buffer, base + nNodes * 12, nNodes),
    edges: new Uint32Array(bytes.buffer, base + nNodes * 16, nEdges * 2),
    edgeNames: new Uint32Array(bytes.buffer, base + nNodes * 16 + nEdges * 8, nEdges),
    quads: new Uint32Array(bytes.buffer, afterEdges, nQuads * 4),
    quadFaces: new Uint32Array(bytes.buffer, afterEdges + nQuads * 16, nQuads),
    quadBlocks: new Uint32Array(bytes.buffer, afterEdges + nQuads * 20, nQuads),
  };
}

/**
 * The geometry, decoded from its own buffer.
 *
 * The mirror of `GeometryBuffer::to_bytes`. A separate buffer from the view,
 * because the two change on different clocks — see that type.
 */
export interface GeometryView {
  /** The same rebasing origin the view buffer uses. */
  centre: [number, number, number];
  /** Three f32 per node, relative to `centre`, shared by everything below. */
  positions: Float32Array;
  /** Three node indices per triangle. */
  triangles: Uint32Array;
  /** The gmsh entity tag of the surface each triangle is on. */
  triangleTags: Int32Array;
  /** Two node indices per segment. */
  segments: Uint32Array;
  /** The entity tag of the curve each segment is on. */
  segmentTags: Int32Array;
  /** One node index per geometric corner. */
  corners: Uint32Array;
  /** The entity tag of each corner. */
  cornerTags: Int32Array;
}

const GEOMETRY_MAGIC = 0x3147_4b47; // "GKG1" little-endian
const GEOMETRY_HEADER = 48;

export function decodeGeometry(bytes: Uint8Array): GeometryView {
  if (bytes.byteLength < GEOMETRY_HEADER) {
    throw new Error(`geometry buffer: ${bytes.byteLength} bytes is shorter than a header`);
  }
  if (bytes.byteOffset % 8 !== 0) bytes = new Uint8Array(bytes);

  const head = new DataView(bytes.buffer, bytes.byteOffset, GEOMETRY_HEADER);
  if (head.getUint32(0, true) !== GEOMETRY_MAGIC) throw new Error("geometry buffer: bad magic");
  const version = head.getUint32(4, true);
  if (version !== 1) throw new Error(`geometry buffer: version ${version} not understood`);

  const centre: [number, number, number] = [
    head.getFloat64(8, true),
    head.getFloat64(16, true),
    head.getFloat64(24, true),
  ];
  const nNodes = head.getUint32(32, true);
  const nTriangles = head.getUint32(36, true);
  const nSegments = head.getUint32(40, true);
  const nCorners = head.getUint32(44, true);

  // Per node three f32. Per triangle three indices and a tag, per segment
  // two and a tag, per corner one and a tag.
  const expected =
    GEOMETRY_HEADER + nNodes * 12 + nTriangles * 16 + nSegments * 12 + nCorners * 8;
  if (expected !== bytes.byteLength) {
    throw new Error(
      `geometry buffer: header declares ${expected} bytes, buffer holds ${bytes.byteLength}`,
    );
  }

  let at = bytes.byteOffset + GEOMETRY_HEADER;
  const take = <T>(make: (buffer: ArrayBufferLike, at: number, n: number) => T, n: number): T => {
    const made = make(bytes.buffer, at, n);
    at += n * 4;
    return made;
  };
  const f32 = (b: ArrayBufferLike, o: number, n: number) => new Float32Array(b, o, n);
  const u32 = (b: ArrayBufferLike, o: number, n: number) => new Uint32Array(b, o, n);
  const i32 = (b: ArrayBufferLike, o: number, n: number) => new Int32Array(b, o, n);

  return {
    centre,
    positions: take(f32, nNodes * 3),
    triangles: take(u32, nTriangles * 3),
    triangleTags: take(i32, nTriangles),
    segments: take(u32, nSegments * 2),
    segmentTags: take(i32, nSegments),
    corners: take(u32, nCorners),
    cornerTags: take(i32, nCorners),
  };
}
