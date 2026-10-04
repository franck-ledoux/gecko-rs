# Static mode

The page carries its own kernel. There is no server and no Python: the wasm
module is loaded into the tab, and everything happens there.

This is the mode to deploy on a web site, or to hand someone as a URL.

## Running it

```bash
./build-wasm.sh                  # kernel -> front/src/pkg
cd front && npm install
npm run dev                      # http://localhost:5173
```

`build-wasm.sh` has to be run again whenever the Rust changes: `front/src/pkg`
is a build artefact and Vite will not regenerate it.

For a production build:

```bash
cd front && npm run build        # -> front/dist
```

`front/dist` is a directory of static files. It can be served by anything, and
the paths inside it are relative, so it works at a site root or in a
subdirectory.

## What the page does

- **Create a cube** adds a unit cube, placed beside the previous one. Where to
  put it is the interface's business; the kernel takes an origin and a size.
- **Dragging a node** moves it. Press on a sphere, move, release — one gesture.
  The edges reaching that node follow the mouse, and the kernel is told once,
  at the drop.

The panel reports what the kernel holds: how many blocks, nodes, edges and
faces, and the last thing it said.

## How the page knows which mode it is in

On boot it asks for `mode`. A station answers `station` and the page connects
to its socket; anything else — a 404, a static host's `index.html` fallback,
no answer at all — and the page falls back to its own wasm kernel.

That is why the answer is checked for its *content* and not merely for a 200:
plenty of static hosts serve `index.html` for unknown paths, and a naive check
would read that as a station.

## Limits

The whole model lives in the tab, so it is bounded by what wasm can hold —
`wasm32` tops out at 4 GB of linear memory, and in practice a browser tab will
complain long before that. For anything that does not fit, or that needs the
file system, a cluster, or numpy, use [station mode](station.md).
