import { message } from './locale.mjs';

// The Nodepool writes these sentences into a managed task that is waiting for its first
// replicas. The numbers are read out so the sentence can be translated with its counts.
const OWNER_RULE_REASON = /^Waiting for (\d+) Workers from different owners; (\d+) available now\.$/;
const SHARED_OWNER_REASON = /^Waiting for (\d+) eligible Workers; (\d+) available now\.$/;

/**
 * Returns a translatable message for a waiting reason, or null for any other task message.
 * Other messages are not changed by this policy.
 */
export function waitingForWorkersMessage(serverMessage) {
  const text = typeof serverMessage === 'string' ? serverMessage.trim() : '';
  const ownerRule = OWNER_RULE_REASON.exec(text);
  if (ownerRule) {
    return message('Waiting for {needed} Workers from different owners; {available} available now.', {
      needed: ownerRule[1],
      available: ownerRule[2],
    });
  }
  const shared = SHARED_OWNER_REASON.exec(text);
  if (shared) {
    return message('Waiting for {needed} eligible Workers; {available} available now.', {
      needed: shared[1],
      available: shared[2],
    });
  }
  return null;
}
