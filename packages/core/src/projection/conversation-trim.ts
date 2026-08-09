import { validateConversationItems, type ConversationItem } from "../domain/conversation.js";
import { estimateConversationItemTokens } from "./context-estimator.js";

/**
 * Conversation reduction preserving semantically atomic pairs (Stage 3
 * milestone 2 §10).
 *
 * Reduction applies to the disposable model history, never to
 * authoritative state. Tool calls are only ever dropped together with
 * their corresponding result; the trimmed transcript stays structurally
 * valid (the application's validator would reject an orphaned call).
 */

export interface ConversationTrimResult {
  readonly items: readonly ConversationItem[];
  readonly estimatedTokens: number;
  readonly droppedItems: number;
}

/** Estimate tokens for a whole conversation (deterministic). */
export function estimateConversationTokens(items: readonly unknown[]): number {
  let sum = 0;
  for (const item of items) {
    sum += estimateConversationItemTokens(item).tokens;
  }
  return sum;
}

/**
 * Drop oldest tool-call/result pairs until the token budget fits. User
 * messages (including the active request) are kept; assistant text is kept
 * while it fits; pairs are removed as whole units from the oldest end of
 * the history. A structurally invalid transcript (for example a trailing
 * tool call without its result) is never reduced — the function fails
 * closed and returns the original list. Returns the original list when it
 * already fits.
 */
export function trimConversationPreservingPairs(
  items: readonly ConversationItem[],
  maxTokens: number,
): ConversationTrimResult {
  const originalTokens = estimateConversationTokens(items);
  if (originalTokens <= maxTokens) {
    return { items: [...items], estimatedTokens: originalTokens, droppedItems: 0 };
  }
  const validatorError = validateConversationItems(items);
  if (validatorError !== null) {
    // Never reduce an already-invalid transcript; fail closed.
    return { items: [...items], estimatedTokens: originalTokens, droppedItems: 0 };
  }
  const kept: ConversationItem[] = [];
  const pendingPair = new Map<string, ConversationItem[]>();
  let tokens = 0;
  for (const item of items) {
    if (item.type === "assistant_tool_call") {
      pendingPair.set(item.callId, [item]);
      continue;
    }
    if (item.type === "tool_result") {
      const pair = pendingPair.get(item.callId);
      if (pair !== undefined) {
        pendingPair.delete(item.callId);
        pair.push(item);
        const pairTokens = estimateConversationTokens(pair);
        if (tokens + pairTokens <= maxTokens) {
          kept.push(...pair);
          tokens += pairTokens;
        }
        // else: the oldest whole pair is dropped.
        continue;
      }
    }
    const itemTokens = estimateConversationItemTokens(item).tokens;
    if (item.type === "user_message") {
      // The active request always survives; it is the current turn anchor.
      kept.push(item);
      tokens += itemTokens;
      continue;
    }
    if (item.type === "assistant_message") {
      if (tokens + itemTokens <= maxTokens) {
        kept.push(item);
        tokens += itemTokens;
      }
      continue;
    }
    kept.push(item);
    tokens += itemTokens;
  }
  const droppedItems = items.length - kept.length;
  return { items: kept, estimatedTokens: tokens, droppedItems };
}
