import { registerHooks } from "node:module";

// Remap relative .js specifiers to their .ts siblings for the
// TypeScript behavioral reference source (type stripping does not
// perform extension resolution by itself). Package specifiers resolve
// to oracle-only shims that alias real source modules (never dist).
registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier === "@siralos/core") {
      const source = new URL("./core-shim.mjs", import.meta.url);
      return { url: source.href, shortCircuit: true };
    }
    if (specifier === "@siralos/adapters") {
      const source = new URL("./adapters-shim.mjs", import.meta.url);
      return { url: source.href, shortCircuit: true };
    }
    if (
      specifier.startsWith(".") &&
      specifier.endsWith(".js") &&
      !context.parentURL.includes("/node_modules/")
    ) {
      const tsUrl = new URL(specifier.replace(/\.js$/, ".ts"), context.parentURL);
      return { url: tsUrl.href, shortCircuit: true };
    }
    return nextResolve(specifier, context);
  },
  load(url, context, nextLoad) {
    // The one parameter-property in the tree (capability-doctor.ts) is
    // unsupported by strip-only mode; rewrite it to an explicit
    // constructor assignment before stripping. Behavior is identical.
    if (url.endsWith("doctor/capability-doctor.ts")) {
      const loaded = nextLoad(url, context);
      loaded.source = String(loaded.source).replace(
        "constructor(readonly timeoutMs: number) {",
        "constructor(timeoutMs: number) {\n    this.timeoutMs = timeoutMs;",
      );
      return loaded;
    }
    return nextLoad(url, context);
  },
});
