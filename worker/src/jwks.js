// Google id_token verification using Web Crypto. No external deps.
//
// Google publishes its JWKS at https://www.googleapis.com/oauth2/v3/certs.
// Keys rotate but never disappear without notice. We cache in module scope
// per Worker isolate; a fresh isolate refetches on first request.

const JWKS_URL = 'https://www.googleapis.com/oauth2/v3/certs';
const ALLOWED_ISS = new Set([
  'https://accounts.google.com',
  'accounts.google.com',
]);

let jwksCache = null;       // { keys: [{kid, jwk}], fetchedAt: ms }
const JWKS_TTL_MS = 1000 * 60 * 60; // 1h soft cache; force refetch on kid miss

function b64urlDecode(s) {
  s = s.replace(/-/g, '+').replace(/_/g, '/');
  const pad = s.length % 4;
  if (pad) s += '='.repeat(4 - pad);
  const bin = atob(s);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

function b64urlDecodeToString(s) {
  return new TextDecoder().decode(b64urlDecode(s));
}

async function fetchJwks() {
  const resp = await fetch(JWKS_URL);
  if (!resp.ok) throw new Error(`JWKS fetch failed: ${resp.status}`);
  const body = await resp.json();
  return { keys: body.keys, fetchedAt: Date.now() };
}

async function getJwk(kid, forceRefetch = false) {
  if (forceRefetch || !jwksCache || Date.now() - jwksCache.fetchedAt > JWKS_TTL_MS) {
    jwksCache = await fetchJwks();
  }
  let key = jwksCache.keys.find((k) => k.kid === kid);
  if (!key && !forceRefetch) {
    // Maybe Google rotated and our cache is stale — try once more.
    jwksCache = await fetchJwks();
    key = jwksCache.keys.find((k) => k.kid === kid);
  }
  return key || null;
}

async function importPublicKey(jwk) {
  return crypto.subtle.importKey(
    'jwk',
    {
      kty: jwk.kty,
      n:   jwk.n,
      e:   jwk.e,
      alg: jwk.alg || 'RS256',
      ext: true,
    },
    { name: 'RSASSA-PKCS1-v1_5', hash: { name: 'SHA-256' } },
    false,
    ['verify'],
  );
}

/**
 * Verify a Google id_token. Returns the decoded claims on success, throws on failure.
 *
 * Validates:
 *   - signature (RS256)
 *   - iss in {accounts.google.com, https://accounts.google.com}
 *   - aud === GOOGLE_CLIENT_ID
 *   - exp > now
 *   - nbf <= now (if present)
 */
export async function verifyGoogleIdToken(idToken, expectedAudience) {
  const parts = idToken.split('.');
  if (parts.length !== 3) throw new Error('Malformed id_token');

  const [headerB64, payloadB64, signatureB64] = parts;

  let header, payload;
  try {
    header = JSON.parse(b64urlDecodeToString(headerB64));
    payload = JSON.parse(b64urlDecodeToString(payloadB64));
  } catch {
    throw new Error('Malformed id_token header/payload');
  }

  if (header.alg !== 'RS256') {
    throw new Error(`Unexpected JWT alg: ${header.alg}`);
  }
  if (!header.kid) throw new Error('Missing kid in JWT header');

  const jwk = await getJwk(header.kid);
  if (!jwk) throw new Error(`No matching JWK for kid: ${header.kid}`);

  const publicKey = await importPublicKey(jwk);

  const signingInput = new TextEncoder().encode(`${headerB64}.${payloadB64}`);
  const signature = b64urlDecode(signatureB64);

  const ok = await crypto.subtle.verify(
    { name: 'RSASSA-PKCS1-v1_5' },
    publicKey,
    signature,
    signingInput,
  );
  if (!ok) throw new Error('Invalid id_token signature');

  // Claim validation.
  if (!ALLOWED_ISS.has(payload.iss)) {
    throw new Error(`Bad iss: ${payload.iss}`);
  }
  if (payload.aud !== expectedAudience) {
    throw new Error('id_token audience mismatch');
  }
  const nowSec = Math.floor(Date.now() / 1000);
  if (typeof payload.exp !== 'number' || payload.exp < nowSec - 5) {
    throw new Error('id_token expired');
  }
  if (typeof payload.nbf === 'number' && payload.nbf > nowSec + 5) {
    throw new Error('id_token not yet valid');
  }

  return payload;
}
