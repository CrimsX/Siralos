import { registerHooks } from "node:module";

// Remap relative .js specifiers to their .ts siblings for the
// TypeScript behavioral reference source (type stripping does not
// perform extension resolution by itself).
registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier === "@siralos/core") {
      // Value imports from the reference package resolve to the TypeScript
      // source index so every R4 probe exercises the real reference code
      // (dist artifacts are never used as an oracle).
      const source = new URL("./core-shim.mjs", import.meta.url);
      return { url: source.href, shortCircuit: true };
    }
    if (
      !specifier.startsWith("node:") &&
      !specifier.startsWith("file:") &&
      specifier.endsWith(".js")
    ) {
      const tsUrl = new URL(specifier.replace(/\.js$/, ".ts"), context.parentURL);
      return { url: tsUrl.href, shortCircuit: true };
    }
    return nextResolve(specifier, context);
  },
});
