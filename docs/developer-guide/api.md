# Rust API reference

Generated from the source by rustdoc, and published alongside this site.

<a href="../../api/gecko_core/index.html"><strong>gecko-core</strong></a>
— the block structure on a 3-map: cells, the two identities, cube corners.

<a href="../../api/gecko_cmd/index.html"><strong>gecko-cmd</strong></a>
— the session: commands, outcomes, the view buffer.

<a href="../../api/gecko_lua/index.html"><strong>gecko-lua</strong></a>
— the scripting API, and the four-method trait both its hosts answer.

<a href="../../api/gecko_wasm/index.html"><strong>gecko-wasm</strong></a>
— the kernel as the page sees it.

!!! note "Why it is a separate site"
    GECKO folds its C++ reference into these pages with `mkdoxy`. There is no
    equivalent for Rust worth the trouble: rustdoc generates its own site, with
    its own search and its own cross-linking, and flattening that into Markdown
    would lose both. It is published under `api/` and linked from here instead.

To read it locally:

```bash
cargo doc --workspace --no-deps --open
```

The Lua API is documented by hand, in [Scripting](../user-guide/scripting.md):
what a reader needs there is the call and what it does, not the Rust type
that carries it.
