import {
  DEFAULT_MAX_PENDING_APPROVAL_MS,
  type ApprovalDecision,
  type ApprovalRequest,
  type ApprovalReviewer,
} from "@solaris/core";
import { formatApprovalPrompt } from "../output.js";
import type { InputQueue } from "../input/input-queue.js";

const ACCEPTED_ANSWERS: readonly string[] = ["y", "yes"];

/**
 * Interactive approval reviewer backed by the session's InputQueue. Reads
 * are cancellable: timeout resolves to a denial, abort to a cancellation,
 * and EOF to a denial, and no stale read can consume later main-loop input.
 */
export function createInteractiveApprovalReviewer(
  queue: InputQueue,
  timeoutMs: number = DEFAULT_MAX_PENDING_APPROVAL_MS,
): ApprovalReviewer {
  return {
    async review(request: ApprovalRequest, signal?: AbortSignal): Promise<ApprovalDecision> {
      queue.write(formatApprovalPrompt(request));
      if (signal?.aborted) {
        return { type: "cancelled" };
      }
      const firstAnswer = await queue.ask("Approve once? [y/N] (The sandbox remains active.) ", {
        ...(signal === undefined ? {} : { signal }),
        timeoutMs,
      });
      if (firstAnswer.kind === "aborted") {
        return { type: "cancelled" };
      }
      if (firstAnswer.kind === "timeout") {
        return { type: "deny", reason: "The approval prompt timed out; the change was denied." };
      }
      if (firstAnswer.kind !== "answer") {
        return { type: "deny", reason: "The approval prompt was discarded." };
      }
      if (firstAnswer.value === null) {
        return { type: "deny", reason: "The approval prompt was closed without an answer." };
      }
      if (isAcceptedAnswer(firstAnswer.value)) {
        return { type: "approve_once" };
      }
      const secondAnswer = await queue.ask("Please answer y or n. Approve once? [y/N] ", {
        ...(signal === undefined ? {} : { signal }),
        timeoutMs,
      });
      if (
        secondAnswer.kind === "answer" &&
        secondAnswer.value !== null &&
        isAcceptedAnswer(secondAnswer.value)
      ) {
        return { type: "approve_once" };
      }
      if (secondAnswer.kind === "aborted") {
        return { type: "cancelled" };
      }
      return { type: "deny", reason: "The change was not approved." };
    },
  };
}

function isAcceptedAnswer(answer: string): boolean {
  return ACCEPTED_ANSWERS.includes(answer);
}
