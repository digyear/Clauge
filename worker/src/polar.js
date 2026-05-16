// Polar webhook verification + parsing.
// Reference: https://docs.polar.sh/integrate/webhooks/delivery
// Signature is HMAC-SHA256 of raw body, hex-encoded, sent in the
// `webhook-signature` header. Replay defense uses the event's
// `created_at` (ISO 8601) — reject if older than 5 minutes.

const REPLAY_WINDOW_MS = 5 * 60 * 1000;

function hexToBytes(hex) {
  if (hex.length % 2 !== 0) return null;
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    const byte = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
    if (Number.isNaN(byte)) return null;
    out[i] = byte;
  }
  return out;
}

function constantTimeEqual(a, b) {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a[i] ^ b[i];
  return diff === 0;
}

export async function verifyPolarSignature(rawBody, signatureHex, env) {
  if (typeof signatureHex !== "string") return false;
  const provided = hexToBytes(signatureHex);
  if (!provided || provided.length !== 32) return false;

  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw",
    enc.encode(env.POLAR_WEBHOOK_SECRET),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );
  const mac = await crypto.subtle.sign("HMAC", key, enc.encode(rawBody));
  return constantTimeEqual(provided, new Uint8Array(mac));
}

export function checkReplayWindow(isoTimestamp) {
  const t = Date.parse(isoTimestamp);
  if (Number.isNaN(t)) return false;
  return Math.abs(Date.now() - t) <= REPLAY_WINDOW_MS;
}

export function parsePolarEvent(rawBody) {
  try {
    const obj = JSON.parse(rawBody);
    if (typeof obj !== "object" || obj === null) return null;
    if (typeof obj.type !== "string" || typeof obj.id !== "string") return null;
    return obj;
  } catch {
    return null;
  }
}
