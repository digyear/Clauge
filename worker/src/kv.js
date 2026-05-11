// KV cache for GitHub /user lookups. Validating a GitHub token requires hitting
// api.github.com — we cache (token -> user_id, profile) in KV for 5 minutes so
// repeated calls don't burn rate limit. Token revocation propagates within TTL.

const TTL_SECONDS = 300; // 5 minutes
const PREFIX = 'gh:';    // namespace inside KV

function key(token) {
  // We use the token directly as part of the key (KV keys aren't logged in CF
  // dashboards). Hashing would force a recompute per request; not worth it.
  return PREFIX + token;
}

export async function getCachedGitHubUser(env, token) {
  const raw = await env.CLAUGE_KV.get(key(token), { type: 'json' });
  return raw || null;
}

export async function setCachedGitHubUser(env, token, payload) {
  await env.CLAUGE_KV.put(key(token), JSON.stringify(payload), {
    expirationTtl: TTL_SECONDS,
  });
}
