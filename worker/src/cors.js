// CORS helpers. All API responses get these headers; preflight is short-circuited.

export function corsHeaders(env) {
  return {
    'Access-Control-Allow-Origin':  env.ALLOWED_ORIGIN || 'https://clauge.in',
    'Access-Control-Allow-Methods': 'GET, POST, PUT, PATCH, DELETE, OPTIONS',
    'Access-Control-Allow-Headers': 'Content-Type, Authorization, X-Provider, X-Confirm',
    'Access-Control-Max-Age':       '86400',
    'Vary':                         'Origin',
  };
}

export function preflight(env) {
  return new Response(null, { status: 204, headers: corsHeaders(env) });
}

export function json(env, body, status = 200, extra = {}) {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      'Content-Type': 'application/json',
      ...corsHeaders(env),
      ...extra,
    },
  });
}

export function err(env, status, error, extra = {}) {
  return json(env, { error }, status, extra);
}
