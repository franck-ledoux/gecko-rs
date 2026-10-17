/** The emscripten module's factory, as much of it as this page uses. */
declare const createLua: (options: {
  host: {
    apply(command: string): string;
    query(kind: string, arg: string): string;
    read(path: string): string | null;
    write(path: string, text: string): string;
  };
}) => Promise<{
  ccall(name: string, returns: string, takes: string[], args: unknown[]): string;
}>;
export default createLua;
