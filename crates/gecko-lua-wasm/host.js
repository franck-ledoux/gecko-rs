// The four primitives, answered in JavaScript.
//
// Lua is in this module; the kernel is in the other one. Everything here
// is marshalling: a C string in, a C string out, allocated with the
// module's own malloc so that Rust can free it.
mergeInto(LibraryManager.library, {
  $geckoGive__internal: true,
  $geckoGive: function (text) {
    if (text === null || text === undefined) return 0;
    const len = lengthBytesUTF8(text) + 1;
    const buf = _malloc(len);
    stringToUTF8(text, buf, len);
    return buf;
  },
  gecko_apply__deps: ["$geckoGive"],
  gecko_apply: function (command) {
    return geckoGive(Module.host.apply(UTF8ToString(command)));
  },
  gecko_query__deps: ["$geckoGive"],
  gecko_query: function (kind, arg) {
    return geckoGive(Module.host.query(UTF8ToString(kind), UTF8ToString(arg)));
  },
  gecko_read__deps: ["$geckoGive"],
  gecko_read: function (path) {
    return geckoGive(Module.host.read(UTF8ToString(path)));
  },
  gecko_write__deps: ["$geckoGive"],
  gecko_write: function (path, text) {
    return geckoGive(Module.host.write(UTF8ToString(path), UTF8ToString(text)));
  },
});
