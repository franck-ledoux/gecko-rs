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
  center: [number, number, number];
  /** Three f32 per node, relative to `center`. */
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
   *  drawn for faces and for blocks, and only the name it is colored by
   *  differs. */
  quads: Uint32Array;
  /** The face each quadrangle belongs to. */
  quadFaces: Uint32Array;
  /** The block each quadrangle belongs to. */
  quadBlocks: Uint32Array;
  /** What each node is classified on: the entity's dimension — 0 a corner,
   *  1 a curve, 2 a surface — or −1 for nothing. */
  nodeClassDim: Int32Array;
  /** The gmsh tag of that entity, meaningless where the dimension is −1. */
  nodeClassTag: Int32Array;
  /** 1 where a node is frozen. Its own array, because a frozen node need
   *  not be classified at all. */
  nodeFrozen: Uint32Array;
  /** The same two per edge. */
  edgeClassDim: Int32Array;
  edgeClassTag: Int32Array;
  /** And per quadrangle, which is where a face is drawn. */
  quadClassDim: Int32Array;
  quadClassTag: Int32Array;
  /** Which groups each cell is in, one bit per group, in two halves — the
   *  low thirty-two bits and the high. Two numbers rather than a BigInt:
   *  these are tested per cell per frame while a filter is on. */
  nodeGroupsLo: Uint32Array;
  nodeGroupsHi: Uint32Array;
  edgeGroupsLo: Uint32Array;
  edgeGroupsHi: Uint32Array;
  quadGroupsLo: Uint32Array;
  quadGroupsHi: Uint32Array;
  /** 1 where a node or an edge bounds a block in **no** group. A cell is
   *  drawn when any block it bounds is drawn, and the mask alone cannot
   *  say that: an edge between a grouped block and an ungrouped one
   *  carries the group's bit. */
  nodeLoose: Uint32Array;
  edgeLoose: Uint32Array;
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
  if (version !== 7) throw new Error(`view buffer: version ${version} not understood`);

  const center: [number, number, number] = [
    head.getFloat64(8, true),
    head.getFloat64(16, true),
    head.getFloat64(24, true),
  ];
  const nNodes = head.getUint32(32, true);
  const nEdges = head.getUint32(36, true);
  const nQuads = head.getUint32(40, true);

  // Check the declared sizes before making any view, so a truncated buffer is
  // a clear error here rather than an opaque one three lines down.
  // Per node: three f32 of position, one u32 of name, three more for what
  // it is classified on and whether it is frozen, and two for the groups
  // it is in. Per edge: two node indices, a name, two for its
  // classification, two for its groups and one saying whether it touches
  // a block in no group. Per quadrangle: four corner indices, the two
  // names it can be colored by, two for its classification and two for
  // its groups — a quadrangle is drawn with one block, so its own mask
  // speaks for it.
  const expected = HEADER + nNodes * 40 + nEdges * 32 + nQuads * 40;
  if (expected !== bytes.byteLength) {
    throw new Error(
      `view buffer: header declares ${expected} bytes, buffer holds ${bytes.byteLength}`,
    );
  }

  // Walked forward rather than addressed by a table of offsets: the arrays
  // come in this order and nothing else needs to know where each starts.
  let at = bytes.byteOffset + HEADER;
  const take = <T>(make: (b: ArrayBufferLike, at: number, n: number) => T, n: number): T => {
    const made = make(bytes.buffer, at, n);
    at += n * 4;
    return made;
  };
  const f32 = (b: ArrayBufferLike, o: number, n: number) => new Float32Array(b, o, n);
  const u32 = (b: ArrayBufferLike, o: number, n: number) => new Uint32Array(b, o, n);
  const i32 = (b: ArrayBufferLike, o: number, n: number) => new Int32Array(b, o, n);

  return {
    center,
    positions: take(f32, nNodes * 3),
    names: take(u32, nNodes),
    edges: take(u32, nEdges * 2),
    edgeNames: take(u32, nEdges),
    quads: take(u32, nQuads * 4),
    quadFaces: take(u32, nQuads),
    quadBlocks: take(u32, nQuads),
    nodeClassDim: take(i32, nNodes),
    nodeClassTag: take(i32, nNodes),
    nodeFrozen: take(u32, nNodes),
    edgeClassDim: take(i32, nEdges),
    edgeClassTag: take(i32, nEdges),
    quadClassDim: take(i32, nQuads),
    quadClassTag: take(i32, nQuads),
    nodeGroupsLo: take(u32, nNodes),
    nodeGroupsHi: take(u32, nNodes),
    nodeLoose: take(u32, nNodes),
    edgeGroupsLo: take(u32, nEdges),
    edgeGroupsHi: take(u32, nEdges),
    edgeLoose: take(u32, nEdges),
    quadGroupsLo: take(u32, nQuads),
    quadGroupsHi: take(u32, nQuads),
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
  center: [number, number, number];
  /** Three f32 per node, relative to `center`, shared by everything below. */
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

  const center: [number, number, number] = [
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
    center,
    positions: take(f32, nNodes * 3),
    triangles: take(u32, nTriangles * 3),
    triangleTags: take(i32, nTriangles),
    segments: take(u32, nSegments * 2),
    segmentTags: take(i32, nSegments),
    corners: take(u32, nCorners),
    cornerTags: take(i32, nCorners),
  };
}

/** What the kernel draws of the mesh: its skin, and nothing inside it. */
export interface MeshView {
  center: [number, number, number];
  /** Three f32 per node of the skin, relative to `center`. */
  positions: Float32Array;
  /** Four node indices per quadrangle. */
  quads: Uint32Array;
  /** The block each quadrangle came out of. */
  blocks: Uint32Array;
}

const MESH_MAGIC = 0x314d_4b47; // "GKM1" little-endian
const MESH_HEADER = 40;

/**
 * The mesh buffer, in the same flat little-endian encoding as the other two.
 *
 * Empty until something has been meshed, and empty again as soon as an edit
 * throws the last mesh away — which is a buffer with no quadrangles in it
 * rather than a special case for the caller.
 */
export function decodeMesh(bytes: Uint8Array): MeshView {
  if (bytes.byteLength < MESH_HEADER) {
    throw new Error(`mesh buffer: ${bytes.byteLength} bytes is shorter than a header`);
  }
  if (bytes.byteOffset % 8 !== 0) bytes = new Uint8Array(bytes);

  const head = new DataView(bytes.buffer, bytes.byteOffset, MESH_HEADER);
  if (head.getUint32(0, true) !== MESH_MAGIC) throw new Error("mesh buffer: bad magic");
  const version = head.getUint32(4, true);
  if (version !== 1) throw new Error(`mesh buffer: version ${version} not understood`);

  const center: [number, number, number] = [
    head.getFloat64(8, true),
    head.getFloat64(16, true),
    head.getFloat64(24, true),
  ];
  const nNodes = head.getUint32(32, true);
  const nQuads = head.getUint32(36, true);

  // Three f32 a node; four indices and a block apiece for the quadrangles.
  const expected = MESH_HEADER + nNodes * 12 + nQuads * 20;
  if (expected !== bytes.byteLength) {
    throw new Error(
      `mesh buffer: header declares ${expected} bytes, buffer holds ${bytes.byteLength}`,
    );
  }

  let at = bytes.byteOffset + MESH_HEADER;
  const take = <T>(make: (buffer: ArrayBufferLike, at: number, n: number) => T, n: number): T => {
    const made = make(bytes.buffer, at, n);
    at += n * 4;
    return made;
  };
  return {
    center,
    positions: take((b, o, n) => new Float32Array(b, o, n), nNodes * 3),
    quads: take((b, o, n) => new Uint32Array(b, o, n), nQuads * 4),
    blocks: take((b, o, n) => new Uint32Array(b, o, n), nQuads),
  };
}
