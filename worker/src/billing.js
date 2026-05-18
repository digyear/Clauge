import { verifyPolarSignature, parsePolarEvent } from "./polar.js";

// Polar webhook payloads expose the customer at either of these paths
// depending on event/expansion shape. Returns the Polar customer UUID
// (string) or null.
function resolvePolarCustomerId(eventData) {
  const d = eventData ?? {};
  return d.customer_id ?? d.customer?.id ?? null;
}

// Polar webhook payloads expose the product id under several shapes depending
// on event type and expansion. Returns the product id (string) or null.
function resolveProductId(eventData) {
  const d = eventData ?? {};
  return d.product_id ?? d.product?.id ?? d.subscription?.product_id ?? d.order?.product_id ?? null;
}

// Plan credit allowance lookup — single source of truth is billing_pricing.
// Operator changes via D1 console; takes effect on the user's NEXT order.paid
// (renewal). Returns null if the row is missing or has 0 credits configured.
async function getPlanCredits(planId, env) {
  if (!planId) return null;
  const row = await env.CLAUGE_DB.prepare(
    "SELECT credits FROM billing_pricing WHERE plan_id = ?"
  ).bind(planId).first();
  if (!row) return null;
  const n = Number(row.credits);
  return Number.isInteger(n) && n > 0 ? n : null;
}

// Per spec §10b: webhook signature verification ALWAYS comes first.
// No DB write happens unless verification passes.
//
// Event handling is dispatched per `type`. Unknown types return 200
// (Polar's docs say to ack any 2xx to prevent retries; ignoring unknown
// types is safer than logging an error to operator alerts).

export async function handleBillingWebhook(request, env) {
  const webhookId = request.headers.get("webhook-id");
  if (!webhookId) return new Response("missing signature", { status: 401 });

  const rawBody = await request.text();
  const ok = await verifyPolarSignature(rawBody, request.headers, env);
  if (!ok) return new Response("bad signature", { status: 401 });

  const event = parsePolarEvent(rawBody);
  if (!event) return new Response("bad payload", { status: 400 });

  // Dedup on the delivery ID from the header (not payload event id).
  const existing = await env.CLAUGE_DB.prepare(
    "SELECT 1 FROM subscription_history WHERE polar_event_id = ?"
  )
    .bind(webhookId)
    .first();
  if (existing) return new Response("duplicate", { status: 200 });

  const userId = resolveUserId(event);
  if (userId === null) {
    return new Response("no user context", { status: 200 });
  }

  await dispatch(event, userId, env);
  await logEvent(event, webhookId, userId, rawBody, env);

  return new Response("ok", { status: 200 });
}

function resolveUserId(event) {
  // Polar webhooks may expose the external customer ID at any of these paths.
  // Check in order, take first non-null.
  const d = event.data ?? {};
  const candidates = [
    d.external_customer_id,
    d.customer?.external_id,
    d.customer_external_id,             // legacy field name
    d.order?.external_customer_id,
    d.order?.customer?.external_id,
    d.subscription?.external_customer_id,
    d.subscription?.customer?.external_id,
  ];
  for (const c of candidates) {
    if (c == null) continue;
    const n = Number(c);
    if (Number.isInteger(n) && n > 0) return n;
  }
  return null;
}

async function logEvent(event, webhookId, userId, rawBody, env) {
  await env.CLAUGE_DB.prepare(
    `INSERT OR IGNORE INTO subscription_history
       (user_id, event_type, polar_event_id, payload_json, occurred_at)
     VALUES (?, ?, ?, ?, ?)`
  )
    .bind(userId, event.type, webhookId, rawBody, event.created_at ?? new Date().toISOString())
    .run();
}

async function dispatch(event, userId, env) {
  // Per-event handlers live below — added in Tasks 7-8.
  // Unknown types are no-op (graceful, returns 200 from caller).
  switch (event.type) {
    case "subscription.created":
      return handleSubscriptionCreated(event, userId, env);
    case "subscription.updated":
    case "subscription.active":         // fires when sub becomes active; delegate to updated
    case "subscription.uncanceled":     // user toggled off cancel-at-period-end; delegate
    case "subscription.past_due":       // explicit past_due event; delegate
      return handleSubscriptionUpdated(event, userId, env);
    case "subscription.canceled":
      return handleSubscriptionCanceled(event, userId, env);
    case "subscription.revoked":
      return handleSubscriptionRevoked(event, userId, env);
    case "order.created":
      return; // pending — no-op until order.paid
    case "order.paid":
      return handleOrderPaid(event, userId, env);
    case "order.refunded":
      return handleOrderRefunded(event, userId, env);
    case "customer.state_changed":
      return; // we derive everything from sub/order events
    default:
      return;
  }
}

// Fallback when product_id can't be resolved to a known plan (env-var drift,
// new product type without a worker update, etc.). Reads from a KV key the
// operator can set; defaults to 1000.
async function fallbackAllowance(env) {
  const raw = await env.CLAUGE_KV.get("pro:default_allowance");
  const n = raw ? Number(raw) : NaN;
  return Number.isInteger(n) && n > 0 ? n : 1000;
}

async function handleSubscriptionCreated(event, userId, env) {
  const d = event.data;
  const polarCustomerId = resolvePolarCustomerId(d);
  const planId = productIdToPlan(resolveProductId(d), env);
  const allowance = (await getPlanCredits(planId, env)) ?? (await fallbackAllowance(env));
  await env.CLAUGE_DB.prepare(
    `UPDATE users SET
       plan = 'pro',
       subscription_status = ?,
       cancel_at_period_end = ?,
       current_period_start = ?,
       current_period_end = ?,
       polar_subscription_id = ?,
       polar_customer_id = COALESCE(?, polar_customer_id),
       credit_allowance_per_cycle = ?,
       past_due_started_at = NULL,
       updated_at = CURRENT_TIMESTAMP
     WHERE user_id = ?`
  )
    .bind(
      d.status === "trialing" ? "active" : d.status,
      d.cancel_at_period_end ? 1 : 0,
      d.current_period_start,
      d.current_period_end,
      d.id,
      polarCustomerId,
      allowance,
      userId
    )
    .run();
}

async function handleSubscriptionUpdated(event, userId, env) {
  const d = event.data;
  if (d.status === "past_due") {
    const polarCustomerId = resolvePolarCustomerId(d);
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET
         subscription_status = 'past_due',
         past_due_started_at = COALESCE(past_due_started_at, CURRENT_TIMESTAMP),
         cancel_at_period_end = ?,
         current_period_end = COALESCE(?, current_period_end),
         polar_customer_id = COALESCE(?, polar_customer_id),
         updated_at = CURRENT_TIMESTAMP
       WHERE user_id = ?`
    )
      .bind(d.cancel_at_period_end ? 1 : 0, d.current_period_end ?? null, polarCustomerId, userId)
      .run();
    return;
  }

  if (d.status === "unpaid") {
    return revokeUser(userId, "unpaid", env);
  }

  const polarCustomerId = resolvePolarCustomerId(d);
  await env.CLAUGE_DB.prepare(
    `UPDATE users SET
       subscription_status = ?,
       cancel_at_period_end = ?,
       current_period_start = COALESCE(?, current_period_start),
       current_period_end = COALESCE(?, current_period_end),
       polar_customer_id = COALESCE(?, polar_customer_id),
       past_due_started_at = CASE WHEN ? = 'active' THEN NULL ELSE past_due_started_at END,
       updated_at = CURRENT_TIMESTAMP
     WHERE user_id = ?`
  )
    .bind(
      d.status,
      d.cancel_at_period_end ? 1 : 0,
      d.current_period_start ?? null,
      d.current_period_end ?? null,
      polarCustomerId,
      d.status,
      userId
    )
    .run();
}

async function handleSubscriptionCanceled(event, userId, env) {
  const d = event.data;
  await env.CLAUGE_DB.prepare(
    `UPDATE users SET
       cancel_at_period_end = 1,
       current_period_end = COALESCE(?, current_period_end),
       updated_at = CURRENT_TIMESTAMP
     WHERE user_id = ?`
  )
    .bind(d.current_period_end ?? null, userId)
    .run();
}

async function handleSubscriptionRevoked(event, userId, env) {
  await revokeUser(userId, "canceled", env);
}

async function revokeUser(userId, terminalStatus, env) {
  // Also clears is_lifetime — fired for both subscription cancel/revoke
  // (no-op on is_lifetime since it was already 0) and lifetime refunds
  // (where order.refunded reaches this via handleOrderRefunded).
  await env.CLAUGE_DB.prepare(
    `UPDATE users SET
       plan = 'free',
       is_lifetime = 0,
       subscription_status = ?,
       cancel_at_period_end = 0,
       credits_remaining = 0,
       past_due_started_at = NULL,
       updated_at = CURRENT_TIMESTAMP
     WHERE user_id = ?`
  )
    .bind(terminalStatus, userId)
    .run();
}

async function handleOrderPaid(event, userId, env) {
  const d = event.data;
  // Branch on the product id to detect lifetime purchases.
  const purchasedPlan = productIdToPlan(resolveProductId(d), env);
  if (purchasedPlan === "lifetime") {
    return handleLifetimeOrderPaid(d, userId, env);
  }

  const current = await env.CLAUGE_DB.prepare(
    "SELECT is_lifetime, credit_allowance_per_cycle, last_granted_order_id FROM users WHERE user_id = ?"
  )
    .bind(userId)
    .first();
  if (!current) return;

  // Never let the recurring path touch a lifetime user. productIdToPlan
  // returns null on env-var drift (POLAR_PRODUCT_LIFETIME rotated/typo'd);
  // without this guard a real lifetime order.paid would fall through here
  // and overwrite the 20k bucket with a tiny recurring grant.
  if (current.is_lifetime) return;

  // Order-ID idempotency. Polar retries up to 10× with exponential backoff;
  // each retry has a fresh webhook-id (which our outer dedup catches) but
  // legitimate re-deliveries with a NEW webhook-id can still arrive
  // (operator-triggered resend, recovery after our 5xx, etc.). Match on
  // the order's own id so we grant exactly once per business event.
  if (d.id && current.last_granted_order_id === d.id) return;

  // Plan credit allowance comes from billing_pricing (operator-tunable in D1).
  // Falls back to the per-user cached allowance, then to a KV default — so a
  // missing/zero row never starves a paying user.
  const fromDb = await getPlanCredits(purchasedPlan, env);
  const grantTotal = fromDb ?? (current.credit_allowance_per_cycle || (await fallbackAllowance(env)));

  const polarCustomerId = resolvePolarCustomerId(d);
  await env.CLAUGE_DB.prepare(
    `UPDATE users SET
       subscription_status = 'active',
       cancel_at_period_end = 0,
       past_due_started_at = NULL,
       current_period_start = COALESCE(?, current_period_start),
       current_period_end = COALESCE(?, current_period_end),
       polar_customer_id = COALESCE(?, polar_customer_id),
       credit_allowance_per_cycle = ?,
       credits_remaining = ?,
       last_granted_order_id = ?,
       updated_at = CURRENT_TIMESTAMP
     WHERE user_id = ?`
  )
    .bind(
      d.current_period_start ?? null,
      d.current_period_end ?? null,
      polarCustomerId,
      grantTotal,
      grantTotal,
      d.id ?? null,
      userId,
    )
    .run();
}

// Lifetime is a one-time order: grant 20k credits, flag is_lifetime=1,
// stamp purchase date on current_period_start (for "purchased on" display),
// and leave current_period_end NULL — there's no renewal. When the bucket
// empties, the user is out of credits until they top-up.
async function handleLifetimeOrderPaid(orderData, userId, env) {
  // Same order-ID idempotency as the recurring path. Without this, a
  // re-delivered order.paid would re-grant lifetime credits on top of
  // whatever the user has spent — far worse than the recurring case.
  const existing = await env.CLAUGE_DB.prepare(
    "SELECT polar_lifetime_order_id FROM users WHERE user_id = ?"
  ).bind(userId).first();
  if (existing?.polar_lifetime_order_id === orderData.id) return;

  // Lifetime grant amount comes from billing_pricing (operator-tunable).
  // Falls back to the KV default if the row is misconfigured — better to
  // grant a smaller bucket than to leave a paying user at 0.
  const grant = (await getPlanCredits("lifetime", env)) ?? (await fallbackAllowance(env));

  const polarCustomerId = resolvePolarCustomerId(orderData);
  await env.CLAUGE_DB.prepare(
    `UPDATE users SET
       plan = 'pro',
       is_lifetime = 1,
       subscription_status = 'active',
       cancel_at_period_end = 0,
       past_due_started_at = NULL,
       current_period_start = CURRENT_TIMESTAMP,
       current_period_end = NULL,
       credit_allowance_per_cycle = ?,
       credits_remaining = ?,
       polar_customer_id = COALESCE(?, polar_customer_id),
       polar_lifetime_order_id = ?,
       updated_at = CURRENT_TIMESTAMP
     WHERE user_id = ?`
  )
    .bind(
      grant,
      grant,
      polarCustomerId,
      orderData.id,
      userId,
    )
    .run();
}

async function handleOrderRefunded(event, userId, env) {
  await revokeUser(userId, "canceled", env);
}

function planToProductId(plan, env) {
  if (plan === "monthly") return env.POLAR_PRODUCT_MONTHLY;
  if (plan === "yearly") return env.POLAR_PRODUCT_YEARLY;
  if (plan === "lifetime") return env.POLAR_PRODUCT_LIFETIME;
  return null;
}

// Inverse lookup — used by handleOrderPaid to branch on which product was
// just paid for. Returns "monthly" | "yearly" | "lifetime" | null.
function productIdToPlan(productId, env) {
  if (productId === env.POLAR_PRODUCT_MONTHLY) return "monthly";
  if (productId === env.POLAR_PRODUCT_YEARLY) return "yearly";
  if (productId === env.POLAR_PRODUCT_LIFETIME) return "lifetime";
  return null;
}

export async function handleCreatePortal(env, userId) {
  if (!userId) return new Response("unauthorized", { status: 401 });
  const row = await env.CLAUGE_DB.prepare(
    "SELECT polar_customer_id FROM users WHERE user_id = ?"
  )
    .bind(userId)
    .first();
  if (!row?.polar_customer_id) return new Response("no customer", { status: 404 });

  const apiBase = env.POLAR_API_BASE ?? "https://api.polar.sh";
  const resp = await fetch(`${apiBase}/v1/customer-sessions/`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${env.POLAR_API_KEY}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({ customer_id: row.polar_customer_id }),
  });
  if (!resp.ok) {
    const text = await resp.text();
    return new Response("polar portal create failed: " + text.slice(0, 200), { status: 502 });
  }
  const data = await resp.json();
  return new Response(JSON.stringify({ url: data.customer_portal_url }), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}

// Public, anonymous, edge-cacheable. Reads two small operator-managed tables.
// Returns a minimal contract — desktop app computes per-month math,
// slashed prices, and all UI formatting from these raw values.
//
// schema_version=1 lets clients gracefully handle future shape changes
// (rule: only add fields, never remove or rename).
export async function handleGetPricing(env) {
  const [pricingResult, discountResult] = await Promise.all([
    env.CLAUGE_DB.prepare(
      "SELECT plan_id AS id, price_usd, credits FROM billing_pricing ORDER BY price_usd ASC"
    ).all(),
    env.CLAUGE_DB.prepare(
      "SELECT plan_id, percent, code FROM billing_discount"
    ).all(),
  ]);

  const discountsByPlan = new Map();
  for (const row of discountResult.results) {
    discountsByPlan.set(row.plan_id, { percent: row.percent, code: row.code });
  }

  const plans = pricingResult.results.map((p) => ({
    id: p.id,
    price_usd: p.price_usd,
    credits: p.credits,
    discount: discountsByPlan.get(p.id) ?? null,
  }));

  return new Response(
    JSON.stringify({ schema_version: 1, plans }),
    {
      status: 200,
      headers: {
        "content-type": "application/json",
        "cache-control": "public, max-age=300",
      },
    }
  );
}

export async function handleCreateCheckout(request, env, userId) {
  if (!userId) return new Response("unauthorized", { status: 401 });
  let body;
  try {
    body = await request.json();
  } catch {
    return new Response("bad json", { status: 400 });
  }
  const productId = planToProductId(body.plan, env);
  if (!productId) return new Response("invalid plan", { status: 400 });

  const userRow = await env.CLAUGE_DB.prepare(
    "SELECT primary_email, polar_customer_id FROM users WHERE user_id = ?"
  )
    .bind(userId)
    .first();

  // success_url carries `plan` so the desktop deep-link handler can show the
  // right "Welcome to Pro Monthly/Yearly/Lifetime" title immediately, even
  // if the Polar `order.paid` webhook hasn't reached D1 yet by the time the
  // user returns. Falls back to live cloudSub once /api/auth/me catches up.
  const req = {
    products: [productId],
    external_customer_id: String(userId),
    customer_email: userRow?.primary_email ?? undefined,
    success_url:
      "https://clauge.in/upgrade-success?ref=" +
      encodeURIComponent(String(userId)) +
      "&plan=" +
      encodeURIComponent(body.plan),
  };
  const apiBase = env.POLAR_API_BASE ?? "https://api.polar.sh";
  const resp = await fetch(`${apiBase}/v1/checkouts/`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${env.POLAR_API_KEY}`,
      "content-type": "application/json",
    },
    body: JSON.stringify(req),
  });
  if (!resp.ok) {
    const text = await resp.text();
    return new Response("polar checkout create failed: " + text.slice(0, 200), { status: 502 });
  }
  const data = await resp.json();
  return new Response(JSON.stringify({ url: data.url }), {
    status: 200,
    headers: { "content-type": "application/json" },
  });
}
