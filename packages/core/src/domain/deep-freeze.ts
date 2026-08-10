/**
 * Deep-freeze a host-owned plain-data artifact.
 *
 * Revisioned domain objects are copied before they reach this helper. The
 * cycle guard keeps the operation total if an internal caller accidentally
 * introduces a cycle, while descriptor inspection avoids invoking getters.
 */
export function deepFreeze<T>(value: T): T {
  return freeze(value, new WeakSet<object>());
}

function freeze<T>(value: T, seen: WeakSet<object>): T {
  if (value === null || (typeof value !== "object" && typeof value !== "function")) {
    return value;
  }
  const object = value as object;
  if (seen.has(object)) {
    return value;
  }
  seen.add(object);
  for (const descriptor of Object.values(Object.getOwnPropertyDescriptors(object))) {
    if ("value" in descriptor) {
      freeze(descriptor.value, seen);
    }
  }
  return Object.freeze(value);
}
