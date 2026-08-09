/**
 * Independent-review provider resolution (ADR 0013 §26).
 *
 * The reviewer defaults to the active development provider profile; a
 * trusted user-level `quality.reviewProvider` may reference an existing
 * configured provider profile instead. There is no new credential system
 * and no reviewer model catalog: a missing profile fails clearly and never
 * silently falls back to an unrelated provider.
 */
export function resolveReviewProviderId(options: {
  readonly configured: string | null;
  readonly registered: ReadonlySet<string>;
  readonly defaultId: string;
}):
  | { readonly ok: true; readonly providerId: string }
  | {
      readonly ok: false;
      readonly message: string;
    } {
  if (options.configured === null) {
    return { ok: true, providerId: options.defaultId };
  }
  if (!options.registered.has(options.configured)) {
    return {
      ok: false,
      message: `Configured quality.reviewProvider "${options.configured}" does not match any registered provider profile; configure it or remove the setting.`,
    };
  }
  return { ok: true, providerId: options.configured };
}
