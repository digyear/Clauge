// Sync route handlers. All require an authenticated user (caller passes ctx).

import { json, err } from './cors.js';
import {
  isValidKind, getSyncState, getSyncBlob, upsertSyncBlob, wipeSyncBlobs,
} from './db.js';

const MAX_PAYLOAD_BYTES = 900_000; // ~900 KB after gzip; D1 row limit is 1 MB.

/** GET /api/sync/state  → [{ kind, contentHash, updatedAt }] */
export async function handleSyncState(request, env, ctx) {
  const rows = await getSyncState(env, ctx.userId);
  return json(env, rows.map((r) => ({
    kind:        r.kind,
    contentHash: r.content_hash,
    updatedAt:   r.updated_at,
  })));
}

/** GET /api/sync/pull/:kind  → { contentHash, updatedAt, payload }  (payload = base64 of gzip) */
export async function handleSyncPull(request, env, ctx, kind) {
  if (!isValidKind(kind)) return err(env, 400, 'Invalid kind');

  const row = await getSyncBlob(env, ctx.userId, kind);
  if (!row) return err(env, 404, 'No blob for this kind');

  // payload is BLOB (Uint8Array) — encode as base64 for JSON transport.
  const payloadBytes = row.payload instanceof Uint8Array
    ? row.payload
    : new Uint8Array(row.payload);

  return json(env, {
    kind:        row.kind,
    contentHash: row.content_hash,
    updatedAt:   row.updated_at,
    payload:     bytesToBase64(payloadBytes),
  });
}

/** PUT /api/sync/push/:kind  body: { contentHash, payload }  → { updatedAt } */
export async function handleSyncPush(request, env, ctx, kind) {
  if (!isValidKind(kind)) return err(env, 400, 'Invalid kind');

  const body = await safeJson(request);
  if (!body || !body.contentHash || !body.payload) {
    return err(env, 400, 'Missing contentHash or payload');
  }
  if (typeof body.contentHash !== 'string' || body.contentHash.length > 128) {
    return err(env, 400, 'Bad contentHash');
  }

  let bytes;
  try {
    bytes = base64ToBytes(body.payload);
  } catch {
    return err(env, 400, 'payload must be base64');
  }
  if (bytes.byteLength > MAX_PAYLOAD_BYTES) {
    return err(env, 413, `Payload too large (${bytes.byteLength} bytes; max ${MAX_PAYLOAD_BYTES})`);
  }

  await upsertSyncBlob(env, ctx.userId, kind, body.contentHash, bytes);

  // Read back updated_at — using CURRENT_TIMESTAMP server-side.
  const row = await getSyncBlob(env, ctx.userId, kind);
  return json(env, { kind, contentHash: row.content_hash, updatedAt: row.updated_at });
}

/** DELETE /api/sync/wipe  Header: X-Confirm: yes  → 200 */
export async function handleSyncWipe(request, env, ctx) {
  const confirm = (request.headers.get('X-Confirm') || '').toLowerCase();
  if (confirm !== 'yes') return err(env, 400, "X-Confirm: yes header required");
  await wipeSyncBlobs(env, ctx.userId);
  return json(env, { ok: true });
}

// ─── helpers ───────────────────────────────────────────────────────

function bytesToBase64(bytes) {
  let binary = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

function base64ToBytes(b64) {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

async function safeJson(request) {
  try { return await request.json(); } catch { return null; }
}
