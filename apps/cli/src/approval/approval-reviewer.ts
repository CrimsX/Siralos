import {
  DEFAULT_MAX_PENDING_APPROVAL_MS,
  type ApprovalDecision,
  type ApprovalRequest,
  type ApprovalReviewer,
} from "@solaris/core";
import { formatApprovalPrompt } from "../output.js";
import type { SessionIO } from "../interactive-session.js";

const ACCEPTED_ANSWERS: readonly string[] = ["y", "yes"];

type PromptOutcome =
  | {
      readonly kind: "answer";
      readonly value: string | null;
    }
  | {
      readonly kind: "timeout";
    }
  | {
      readonly kind: "aborted";
    };

export function createInteractiveApprovalReviewer(
  io: SessionIO,
  timeoutMs: number = DEFAULT_MAX_PENDING_APPROVAL_MS,
): ApprovalReviewer {
  return {
    async review(request: ApprovalRequest, signal?: AbortSignal): Promise<ApprovalDecision> {
      io.write(formatApprovalPrompt(request));
      if (signal?.aborted) {
        return { type: "cancelled" };
      }
      const firstAnswer = await askOnce(
        io,
        "Approve once? [y/N] (The sandbox remains active.) ",
        signal,
        timeoutMs,
      );
      if (firstAnswer.kind === "aborted") {
        return { type: "cancelled" };
      }
      if (firstAnswer.kind === "timeout") {
        return { type: "deny", reason: "The approval prompt timed out; the change was denied." };
      }
      if (firstAnswer.value === null) {
        return { type: "deny", reason: "The approval prompt was closed without an answer." };
      }
      if (isAcceptedAnswer(firstAnswer.value)) {
        return { type: "approve_once" };
      }
      const secondAnswer = await askOnce(
        io,
        "Please answer y or n. Approve once? [y/N] ",
        signal,
        timeoutMs,
      );
      if (
        secondAnswer.kind === "answer" &&
        secondAnswer.value !== null &&
        isAcceptedAnswer(secondAnswer.value)
      ) {
        return { type: "approve_once" };
      }
      return { type: "deny", reason: "The change was not approved." };
    },
  };
}

async function askOnce(
  io: SessionIO,
  prompt: string,
  signal: AbortSignal | undefined,
  timeoutMs: number,
): Promise<PromptOutcome> {
  let timer: NodeJS.Timeout | undefined;
  let abortListener: (() => void) | undefined;
  const timeout = new Promise<PromptOutcome>((resolve) => {
    timer = setTimeout(() => {
      resolve({ kind: "timeout" });
    }, timeoutMs);
  });
  const aborted = new Promise<PromptOutcome>((resolve) => {
    abortListener = () => resolve({ kind: "aborted" });
    signal?.addEventListener("abort", abortListener, { once: true });
  });
  const outcome = await Promise.race([
    io.ask(prompt).then((value) => ({ kind: "answer" as const, value })),
    timeout,
    aborted,
  ]);
  if (timer !== undefined) {
    clearTimeout(timer);
  }
  if (abortListener !== undefined) {
    signal?.removeEventListener("abort", abortListener);
  }
  return outcome;
}

function isAcceptedAnswer(answer: string): boolean {
  return ACCEPTED_ANSWERS.includes(answer);
}
