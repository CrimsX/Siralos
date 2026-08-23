/**
 * icm.dependency-manifests oracle probe (differential harness, ADR
 * 0033, Stage 3R R10b).
 *
 * Spawned by the oracle runner with the scenario input JSON on stdin.
 * Exercises the staleness rules and provenance-chain helpers from the
 * REAL TypeScript reference (packages/core/src/context/{staleness,
 * provenance,artifacts}.ts).
 */
import { readFileSync } from "node:fs";
import {
  createArtifactDependencyManifest,
  buildDependencyManifest,
} from "../../../packages/core/src/context/artifacts.js";
import {
  computeStalenessDigest,
  deriveArtifactStaleness,
  isPreparedMutationStale,
} from "../../../packages/core/src/context/staleness.js";
import {
  computeProvenanceDigest,
  createContextProvenanceRef,
  renderWhyValidationRequired,
  whyValidationRequired,
} from "../../../packages/core/src/context/provenance.js";

const MAX_INPUT_BYTES = 64 * 1024;

const input = (() => {
  const bytes = readFileSync(0);
  if (bytes.length === 0 || bytes.length > MAX_INPUT_BYTES) {
    throw new Error("probe input must be a bounded non-empty JSON document");
  }
  return JSON.parse(bytes.toString("utf8"));
})();

const op = input.op;

if (op === "staleness") {
  const manifests = (input.manifests ?? []).map((manifest) =>
    createArtifactDependencyManifest({
      artifactType: manifest.artifactType,
      artifactId: manifest.artifactId,
      dependsOn: (manifest.dependsOn ?? []).map((entry) => ({
        artifactType: entry.artifactType,
        digest: entry.digest,
      })),
    }),
  );
  const currentInputDigests = input.currentInputDigests ?? {};
  const result = deriveArtifactStaleness({
    manifests,
    currentInputDigests,
  });
  process.stdout.write(
    JSON.stringify({
      stale: result.stale,
      current: result.current,
      unrelatedChanges: result.unrelatedChanges,
      digest: computeStalenessDigest(result),
    }),
  );
} else if (op === "prepared-mutation-stale") {
  const result = isPreparedMutationStale({
    preparedSourceRevisions: input.preparedSourceRevisions ?? [],
    currentSourceRevisions: input.currentSourceRevisions ?? {},
  });
  process.stdout.write(JSON.stringify({ stale: result.stale, stalePaths: result.stalePaths }));
} else if (op === "manifest") {
  try {
    if (input.mode === "build") {
      const manifest = buildDependencyManifest({
        artifactType: input.artifactType,
        artifactId: input.artifactId,
        currentDigests: input.currentDigests ?? {},
      });
      process.stdout.write(JSON.stringify({ ok: true, manifest }));
    } else {
      const manifest = createArtifactDependencyManifest({
        artifactType: input.artifactType,
        artifactId: input.artifactId,
        dependsOn: input.dependsOn ?? [],
      });
      process.stdout.write(
        JSON.stringify({
          ok: true,
          manifest: {
            artifactType: manifest.artifactType,
            artifactId: manifest.artifactId,
            dependsOn: manifest.dependsOn.map((entry) => ({
              artifactType: entry.artifactType,
              digest: entry.digest,
            })),
            digest: manifest.digest,
          },
        }),
      );
    }
  } catch (error) {
    process.stdout.write(JSON.stringify({ ok: false, error: String(error.message) }));
  }
} else if (op === "provenance") {
  try {
    const created = createContextProvenanceRef({
      item: input.newRef.item,
      kind: input.newRef.kind,
      id: input.newRef.id,
      digest: input.newRef.digest === undefined ? null : input.newRef.digest,
    });
    const refs = (input.refs ?? [])
      .map((reference) => ({
        item: reference.item,
        source: {
          kind: reference.source.kind,
          id: reference.source.id,
          digest: reference.source.digest === undefined ? null : reference.source.digest,
        },
      }))
      .concat([created]);
    process.stdout.write(
      JSON.stringify({
        created: {
          item: created.item,
          source: {
            kind: created.source.kind,
            id: created.source.id,
            digest: created.source.digest,
          },
        },
        digest: computeProvenanceDigest(refs),
      }),
    );
  } catch (error) {
    process.stdout.write(JSON.stringify({ ok: false, error: String(error.message) }));
  }
} else if (op === "why-validation-required") {
  const diagnostic = whyValidationRequired({
    itemId: input.itemId,
    plan: { items: input.planItems ?? [] },
    changedSurfaces: input.changedSurfaces ?? [],
    impactRelations: input.impactRelations ?? [],
    acceptanceCriteria: input.acceptanceCriteria ?? [],
  });
  process.stdout.write(
    JSON.stringify({
      found: diagnostic !== null,
      itemId: input.itemId,
      rendered: diagnostic === null ? "" : renderWhyValidationRequired(diagnostic),
    }),
  );
} else {
  throw new Error(`unknown icm.dependency-manifests op ${JSON.stringify(op)}`);
}
