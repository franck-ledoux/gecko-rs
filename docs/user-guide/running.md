# Running it

The page carries everything: the kernel compiled to wasm, and a Lua 5.4
interpreter beside it. There is no server in the architecture — what the
local binary adds is files, and nothing else.

## Three commands

```bash
./setup     # once per machine: toolchains, emsdk, node packages
./build     # the kernel, the interpreter, the page, the binary
./gecko     # serve the page and open it
```

`./setup` is safe to run again; each step looks before it leaps. `./build`
has to be run again whenever the Rust or the TypeScript changes —
`front/src/pkg` and `front/src/lua` are build artefacts and Vite will not
regenerate them.

To have it anywhere rather than from the clone:

```bash
cargo install --path crates/gecko-cli
gecko                      # in a folder of meshes
gecko run sweep.lua        # no browser at all
gecko --help
```

**The folder matters.** `gecko` lends the page the files of the directory it
was started in, so start it where your `.msh` files are and
`gecko.load_geometry("cylinder.msh")` works in the console.

## Working on it

```bash
./dev       # the page from Vite with hot reload, and a file server behind it
./view      # the page alone, shorter loop, no files
./check.sh  # format, lint, every suite, the page in a real browser
./docs.sh   # this site, into site/
```

A TypeScript edit reloads the page in place. A Rust edit means building the
wasm again, which is what `./dev` does when you start it.

## Without the binary at all

`front/dist` is a directory of static files, and the paths inside it are
relative, so it works at a site root or in a subdirectory — a web site, or a
URL you hand someone. Everything works there except files: a script that
reads one is told there are none, and one that writes gets a **download**
instead.

## Limits

The whole model lives in the tab, so it is bounded by what wasm can hold —
`wasm32` tops out at 4 GB of linear memory, and a browser tab will complain
long before that. For a model that does not fit, or a sweep that should not
be watched, `gecko run` has the same kernel outside the browser.
