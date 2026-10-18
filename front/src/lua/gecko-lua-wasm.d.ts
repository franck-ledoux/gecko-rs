/** The emscripten module's factory, as much of it as this page uses. */
declare const createLua: (options: {
  host: {
    apply(command: string): string;
    query(kind: string, arg: string): string;
    read(path: string): string | null;
    /** The names a path-less `read` would find: what the page was handed. */
    files(): string[];
    write(path: string, text: string): string;
  };
}) => Promise<{
  ccall(name: string, returns: string, takes: string[], args: unknown[]): string;
}>;
export default createLua;
