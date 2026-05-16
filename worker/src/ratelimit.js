// KV-backed sliding window per user. Cheap and good enough for the
// scale we're at. KV writes are eventually consistent — for stricter
// guarantees we'd use Durable Objects, but RPM is a soft guard not a
// hard one, so KV is fine here.

const RPM_TTL_SECONDS = 90; // window is 60s; TTL with slack

// Increments a per-user-per-minute counter; returns true if under limit.
export async function checkRpm(userId, limitPerMinute, env) {
  const minute = Math.floor(Date.now() / 60_000);
  const key = `rl:rpm:${userId}:${minute}`;
  const current = Number((await env.CLAUGE_KV.get(key)) ?? 0);
  if (current >= limitPerMinute) return false;
  await env.CLAUGE_KV.put(key, String(current + 1), { expirationTtl: RPM_TTL_SECONDS });
  return true;
}

// Generic per-key per-minute rate limiter. Used for IP-based gating
// on routes that have no authenticated user (e.g. webhook pre-HMAC).
export async function checkKeyRpm(keyId, limitPerMinute, env) {
  const minute = Math.floor(Date.now() / 60_000);
  const key = `rl:key:${keyId}:${minute}`;
  const current = Number((await env.CLAUGE_KV.get(key)) ?? 0);
  if (current >= limitPerMinute) return false;
  await env.CLAUGE_KV.put(key, String(current + 1), { expirationTtl: RPM_TTL_SECONDS });
  return true;
}

// Burst budget: at most `fraction` of allowance per `windowSeconds`.
// `costCredits` is added to the running total; if over cap, refused.
export async function checkBurstBudget(userId, allowancePerCycle, fraction, windowSeconds, costCredits, env) {
  const cap = Math.floor(allowancePerCycle * fraction);
  const window = Math.floor(Date.now() / (windowSeconds * 1000));
  const key = `burst:${userId}:${window}`;
  const current = Number((await env.CLAUGE_KV.get(key)) ?? 0);
  if (current + costCredits > cap) return false;
  await env.CLAUGE_KV.put(key, String(current + costCredits), {
    expirationTtl: windowSeconds + 60,
  });
  return true;
}
