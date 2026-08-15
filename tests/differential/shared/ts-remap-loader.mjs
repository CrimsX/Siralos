import { registerHooks } from "node:module";

// Remap relative .js specifiers to their .ts siblings for the
// TypeScript behavioral reference source (type stripping does not
// perform extension resolution by itself).
registerHooks({
  resolve(specifier, context, nextResolve) {
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
