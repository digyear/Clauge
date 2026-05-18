import { describe, it, expect, beforeEach, vi } from "vitest";
import { env } from "cloudflare:test";
import { handleBillingWebhook } from "../src/billing.js";
import { seedUser } from "./setup.js";

async function buildSignedHeaders(rawBody, opts = {}) {
  const enc = new TextEncoder();
  const id = opts.id ?? `msg_${Math.random().toString(36).slice(2)}`;
  const timestamp = opts.timestamp ?? String(Math.floor(Date.now() / 1000));
  const secret = env.POLAR_WEBHOOK_SECRET;
  const key = await crypto.subtle.importKey(
    "raw",
    enc.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );
  const mac = await crypto.subtle.sign("HMAC", key, enc.encode(`${id}.${timestamp}.${rawBody}`));
  const sigBytes = new Uint8Array(mac);
  let bin = "";
  for (const b of sigBytes) bin += String.fromCharCode(b);
  return { id, headers: new Headers({
    "webhook-id": id,
    "webhook-timestamp": timestamp,
    "webhook-signature": `v1,${btoa(bin)}`,
    "content-type": "application/json",
  }) };
}

async function postWebhook(body, opts = {}) {
  const { headers } = await buildSignedHeaders(body, opts);
  return handleBillingWebhook(
    new Request("https://x/api/billing/webhook", { method: "POST", headers, body }),
    env
  );
}

describe("handleBillingWebhook router", () => {
  beforeEach(async () => {
    await env.CLAUGE_DB.prepare("DELETE FROM subscription_history").run();
    await env.CLAUGE_DB.prepare("DELETE FROM users").run();
  });

  it("rejects requests with missing signature headers", async () => {
    const r = await handleBillingWebhook(
      new Request("https://x", { method: "POST", body: "{}" }),
      env
    );
    expect(r.status).toBe(401);
  });

  it("rejects requests with bad signature", async () => {
    const body = "{}";
    const r = await handleBillingWebhook(
      new Request("https://x", {
        method: "POST",
        headers: {
          "webhook-id": "msg_x",
          "webhook-timestamp": String(Math.floor(Date.now() / 1000)),
          "webhook-signature": "v1,YmFkc2lndmFsdWU=",
        },
        body,
      }),
      env
    );
    expect(r.status).toBe(401);
  });

  it("rejects requests with stale timestamp header", async () => {
    const body = JSON.stringify({ type: "subscription.created", data: {} });
    const old = String(Math.floor(Date.now() / 1000) - 6 * 60);
    const r = await postWebhook(body, { timestamp: old });
    expect(r.status).toBe(401);
  });

  it("returns 200 for an unknown event type (graceful drop)", async () => {
    const body = JSON.stringify({
      type: "some.future.event",
      data: {},
    });
    const r = await postWebhook(body);
    expect(r.status).toBe(200);
  });

  it("dedupes by webhook-id (replay-safe)", async () => {
    const userId = await seedUser({ slug: "u1" });
    const body = JSON.stringify({
      type: "subscription.created",
      data: {
        id: "sub_test_1",
        status: "active",
        current_period_start: new Date().toISOString(),
        current_period_end: new Date(Date.now() + 30 * 86400_000).toISOString(),
        external_customer_id: String(userId),
        product_id: env.POLAR_PRODUCT_MONTHLY,
        cancel_at_period_end: false,
      },
    });
    // Two calls with the same webhook-id header
    expect((await postWebhook(body, { id: "msg_dup_1" })).status).toBe(200);
    expect((await postWebhook(body, { id: "msg_dup_1" })).status).toBe(200);
    const count = await env.CLAUGE_DB.prepare(
      "SELECT COUNT(*) AS n FROM subscription_history WHERE polar_event_id = ?"
    ).bind("msg_dup_1").first();
    expect(count.n).toBe(1);
  });
});

describe("subscription.created handler", () => {
  it("flips plan to pro, sets period bounds, sets monthly allowance from billing_pricing", async () => {
    const userId = await seedUser({ slug: "u_created" });
    const body = JSON.stringify({
      type: "subscription.created",
      data: {
        id: "sub_abc",
        status: "active",
        current_period_start: "2026-05-16T00:00:00Z",
        current_period_end: "2026-06-16T00:00:00Z",
        cancel_at_period_end: false,
        external_customer_id: String(userId),
        product_id: env.POLAR_PRODUCT_MONTHLY,
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT plan, subscription_status, polar_subscription_id, credits_remaining, credit_allowance_per_cycle, current_period_end FROM users WHERE user_id = ?"
    )
      .bind(userId)
      .first();
    expect(row.plan).toBe("pro");
    expect(row.subscription_status).toBe("active");
    expect(row.polar_subscription_id).toBe("sub_abc");
    expect(row.credit_allowance_per_cycle).toBe(1000); // monthly seed
    expect(row.credits_remaining).toBe(0);
    expect(row.current_period_end).toBe("2026-06-16T00:00:00Z");
  });

  it("sets yearly allowance (12000) for a yearly subscription", async () => {
    const userId = await seedUser({ slug: "u_created_yearly" });
    const body = JSON.stringify({
      type: "subscription.created",
      data: {
        id: "sub_yearly",
        status: "active",
        current_period_start: "2026-05-16T00:00:00Z",
        current_period_end: "2027-05-16T00:00:00Z",
        cancel_at_period_end: false,
        external_customer_id: String(userId),
        product_id: env.POLAR_PRODUCT_YEARLY,
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT credit_allowance_per_cycle FROM users WHERE user_id = ?"
    ).bind(userId).first();
    expect(row.credit_allowance_per_cycle).toBe(12000);
  });
});

describe("subscription.canceled handler", () => {
  it("sets cancel_at_period_end=1 but keeps user active", async () => {
    const userId = await seedUser({ slug: "u_cancel" });
    // Pre-seed an active sub
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', subscription_status='active', polar_subscription_id='sub_c', credit_allowance_per_cycle=1000, credits_remaining=400 WHERE user_id=?"
    )
      .bind(userId)
      .run();
    const body = JSON.stringify({
      type: "subscription.canceled",
      data: {
        id: "sub_c",
        status: "active",
        cancel_at_period_end: true,
        current_period_end: "2026-06-16T00:00:00Z",
        external_customer_id: String(userId),
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT plan, subscription_status, cancel_at_period_end, credits_remaining FROM users WHERE user_id=?"
    )
      .bind(userId)
      .first();
    expect(row.plan).toBe("pro");
    expect(row.cancel_at_period_end).toBe(1);
    expect(row.credits_remaining).toBe(400); // unchanged
  });
});

describe("subscription.revoked handler", () => {
  it("flips plan to free, wipes credits", async () => {
    const userId = await seedUser({ slug: "u_revoke" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', subscription_status='active', credits_remaining=200 WHERE user_id=?"
    )
      .bind(userId)
      .run();
    const body = JSON.stringify({
      type: "subscription.revoked",
      data: { id: "sub_r", status: "canceled", external_customer_id: String(userId) },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT plan, subscription_status, credits_remaining FROM users WHERE user_id=?"
    )
      .bind(userId)
      .first();
    expect(row.plan).toBe("free");
    expect(row.subscription_status).toBe("canceled");
    expect(row.credits_remaining).toBe(0);
  });
});

describe("subscription.updated past_due handler", () => {
  it("sets past_due_started_at on first transition, leaves credits", async () => {
    const userId = await seedUser({ slug: "u_pastdue" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', subscription_status='active', credits_remaining=500 WHERE user_id=?"
    )
      .bind(userId)
      .run();
    const body = JSON.stringify({
      type: "subscription.updated",
      data: {
        id: "sub_p",
        status: "past_due",
        cancel_at_period_end: false,
        current_period_end: "2026-06-16T00:00:00Z",
        external_customer_id: String(userId),
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT subscription_status, past_due_started_at, plan, credits_remaining FROM users WHERE user_id=?"
    )
      .bind(userId)
      .first();
    expect(row.subscription_status).toBe("past_due");
    expect(row.past_due_started_at).not.toBeNull();
    expect(row.plan).toBe("pro"); // grace period, features still on
    expect(row.credits_remaining).toBe(500);
  });
});

describe("subscription.updated unpaid handler", () => {
  it("treats unpaid as immediate revocation", async () => {
    const userId = await seedUser({ slug: "u_unpaid" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', subscription_status='past_due', credits_remaining=300 WHERE user_id=?"
    )
      .bind(userId)
      .run();
    const body = JSON.stringify({
      type: "subscription.updated",
      data: {
        id: "sub_u",
        status: "unpaid",
        external_customer_id: String(userId),
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT plan, subscription_status, credits_remaining FROM users WHERE user_id=?"
    )
      .bind(userId)
      .first();
    expect(row.plan).toBe("free");
    expect(row.subscription_status).toBe("unpaid");
    expect(row.credits_remaining).toBe(0);
  });
});

describe("order.paid handler — monthly", () => {
  it("grants billing_pricing.monthly.credits on a new billing period", async () => {
    const userId = await seedUser({ slug: "u_paid" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', subscription_status='active',
         polar_subscription_id='sub_p1',
         current_period_start='2026-05-16T00:00:00Z',
         current_period_end='2026-06-16T00:00:00Z',
         credit_allowance_per_cycle=1000, credits_remaining=42
       WHERE user_id=?`
    )
      .bind(userId)
      .run();
    const body = JSON.stringify({
      type: "order.paid",
      data: {
        id: "ord_p1",
        status: "paid",
        subscription_id: "sub_p1",
        product_id: env.POLAR_PRODUCT_MONTHLY,
        billing_reason: "subscription_cycle",
        current_period_start: "2026-06-16T00:00:00Z",
        current_period_end: "2026-07-16T00:00:00Z",
        external_customer_id: String(userId),
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT credits_remaining, credit_allowance_per_cycle, current_period_start FROM users WHERE user_id=?"
    )
      .bind(userId)
      .first();
    expect(row.credits_remaining).toBe(1000);
    expect(row.credit_allowance_per_cycle).toBe(1000);
    expect(row.current_period_start).toBe("2026-06-16T00:00:00Z");
  });
});

describe("order.paid handler — yearly", () => {
  it("grants billing_pricing.yearly.credits (12000) when product_id is yearly", async () => {
    const userId = await seedUser({ slug: "u_yearly" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', subscription_status='active',
         polar_subscription_id='sub_y1',
         current_period_start='2026-05-16T00:00:00Z',
         current_period_end='2027-05-16T00:00:00Z',
         credit_allowance_per_cycle=12000, credits_remaining=42
       WHERE user_id=?`
    )
      .bind(userId)
      .run();
    const body = JSON.stringify({
      type: "order.paid",
      data: {
        id: "ord_y1",
        subscription_id: "sub_y1",
        product_id: env.POLAR_PRODUCT_YEARLY,
        current_period_start: "2027-05-16T00:00:00Z",
        current_period_end: "2028-05-16T00:00:00Z",
        external_customer_id: String(userId),
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT credits_remaining, credit_allowance_per_cycle FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.credits_remaining).toBe(12000);
    expect(row.credit_allowance_per_cycle).toBe(12000);
  });
});

describe("order.paid handler — operator-tunable credits", () => {
  it("reflects an UPDATE billing_pricing change on the next order.paid", async () => {
    const userId = await seedUser({ slug: "u_tunable" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', subscription_status='active',
         polar_subscription_id='sub_t1', credit_allowance_per_cycle=1000, credits_remaining=0
       WHERE user_id=?`
    ).bind(userId).run();

    // Operator bumps monthly credits in D1.
    await env.CLAUGE_DB.prepare(
      "UPDATE billing_pricing SET credits = 2500 WHERE plan_id = 'monthly'"
    ).run();

    const body = JSON.stringify({
      type: "order.paid",
      data: {
        id: "ord_t1",
        subscription_id: "sub_t1",
        product_id: env.POLAR_PRODUCT_MONTHLY,
        external_customer_id: String(userId),
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT credits_remaining, credit_allowance_per_cycle FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.credits_remaining).toBe(2500);
    expect(row.credit_allowance_per_cycle).toBe(2500);

    // Reset for other tests.
    await env.CLAUGE_DB.prepare(
      "UPDATE billing_pricing SET credits = 1000 WHERE plan_id = 'monthly'"
    ).run();
  });
});

describe("order.paid handler — product_id absent", () => {
  it("falls back to cached credit_allowance_per_cycle when product_id is missing", async () => {
    // This shouldn't happen in production (Polar always sends product_id),
    // but defensive: a misshapen payload must not zero a user out.
    const userId = await seedUser({ slug: "u_no_product" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', subscription_status='active',
         polar_subscription_id='sub_nopr',
         credit_allowance_per_cycle=12000, credits_remaining=0
       WHERE user_id=?`
    ).bind(userId).run();
    const body = JSON.stringify({
      type: "order.paid",
      data: {
        id: "ord_nopr",
        subscription_id: "sub_nopr",
        external_customer_id: String(userId),
        // product_id missing
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.credits_remaining).toBe(12000); // from cached allowance
  });
});

describe("initial purchase flow (sub.created + order.paid)", () => {
  it("grants credits exactly once across both events", async () => {
    const userId = await seedUser({ slug: "u_initial" });

    // 1. subscription.created fires (no credits granted, but allowance is set)
    const subBody = JSON.stringify({
      type: "subscription.created",
      data: {
        id: "sub_init",
        status: "active",
        cancel_at_period_end: false,
        current_period_start: "2026-05-16T00:00:00Z",
        current_period_end: "2026-06-16T00:00:00Z",
        external_customer_id: String(userId),
        product_id: env.POLAR_PRODUCT_MONTHLY,
      },
    });
    await postWebhook(subBody);
    let row = await env.CLAUGE_DB.prepare(
      "SELECT plan, credits_remaining, credit_allowance_per_cycle FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.plan).toBe("pro");
    expect(row.credit_allowance_per_cycle).toBe(1000);
    expect(row.credits_remaining).toBe(0); // not yet granted

    // 2. order.paid fires (grants monthly credits)
    const ordBody = JSON.stringify({
      type: "order.paid",
      data: {
        id: "ord_init",
        subscription_id: "sub_init",
        product_id: env.POLAR_PRODUCT_MONTHLY,
        current_period_start: "2026-05-16T00:00:00Z",
        current_period_end: "2026-06-16T00:00:00Z",
        external_customer_id: String(userId),
      },
    });
    await postWebhook(ordBody);
    row = await env.CLAUGE_DB.prepare(
      "SELECT credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.credits_remaining).toBe(1000);
  });
});

describe("order.refunded handler", () => {
  it("treats refund as immediate revocation", async () => {
    const userId = await seedUser({ slug: "u_refund" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', subscription_status='active', credits_remaining=700 WHERE user_id=?"
    )
      .bind(userId)
      .run();
    const body = JSON.stringify({
      type: "order.refunded",
      data: { id: "ord_r", external_customer_id: String(userId) },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT plan, credits_remaining FROM users WHERE user_id=?"
    )
      .bind(userId)
      .first();
    expect(row.plan).toBe("free");
    expect(row.credits_remaining).toBe(0);
  });
});

describe("POST /api/billing/portal", () => {
  it("returns 401 without auth", async () => {
    const { handleCreatePortal } = await import("../src/billing.js");
    const r = await handleCreatePortal(env, null);
    expect(r.status).toBe(401);
  });

  it("returns 404 if user has no polar_customer_id", async () => {
    const userId = await seedUser({ slug: "u_portal_none" });
    const { handleCreatePortal } = await import("../src/billing.js");
    const r = await handleCreatePortal(env, userId);
    expect(r.status).toBe(404);
  });

  it("returns a portal url (Polar API mocked)", async () => {
    const userId = await seedUser({ slug: "u_portal" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET polar_customer_id='cus_x1' WHERE user_id=?"
    )
      .bind(userId)
      .run();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async (url) => {
      expect(String(url)).toContain("polar.sh");
      return new Response(JSON.stringify({ customer_portal_url: "https://sandbox.polar.sh/portal/x" }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    });
    try {
      const { handleCreatePortal } = await import("../src/billing.js");
      const r = await handleCreatePortal(env, userId);
      expect(r.status).toBe(200);
      const p = await r.json();
      expect(p.url).toContain("polar.sh/portal");
    } finally {
      fetchMock.mockRestore();
    }
  });
});

describe("rate limits", () => {
  beforeEach(async () => {
    const list = await env.CLAUGE_KV.list({ prefix: "rl:key:" });
    for (const k of list.keys) await env.CLAUGE_KV.delete(k.name);
  });

  it("blocks the 6th checkout request in the same minute from one user", async () => {
    const userId = await seedUser({ slug: "u_rl_chk" });
    const { checkKeyRpm } = await import("../src/ratelimit.js");
    // burn the budget
    for (let i = 0; i < 5; i++) {
      expect(await checkKeyRpm(`checkout:${userId}`, 5, env)).toBe(true);
    }
    expect(await checkKeyRpm(`checkout:${userId}`, 5, env)).toBe(false);
    // The route is what enforces this — verified via the checkKeyRpm contract.
  });
});

describe("GET /api/billing/pricing", () => {
  beforeEach(async () => {
    await env.CLAUGE_DB.prepare("DELETE FROM billing_discount").run();
  });

  it("returns schema_version=1 and seeded plans with credits + discount=null", async () => {
    const { handleGetPricing } = await import("../src/billing.js");
    const r = await handleGetPricing(env);
    expect(r.status).toBe(200);
    expect(r.headers.get("cache-control")).toContain("max-age=300");
    const body = await r.json();
    expect(body.schema_version).toBe(1);
    expect(body.plans.map((p) => p.id).sort()).toEqual(["lifetime", "monthly", "yearly"]);
    const monthly  = body.plans.find((p) => p.id === "monthly");
    const yearly   = body.plans.find((p) => p.id === "yearly");
    const lifetime = body.plans.find((p) => p.id === "lifetime");
    expect(monthly).toEqual({ id: "monthly", price_usd: 12, credits: 1000, discount: null });
    expect(yearly).toEqual({ id: "yearly", price_usd: 100, credits: 12000, discount: null });
    expect(lifetime).toEqual({ id: "lifetime", price_usd: 299, credits: 20000, discount: null });
  });

  it("attaches per-plan discount when row exists", async () => {
    await env.CLAUGE_DB.prepare(
      "INSERT OR REPLACE INTO billing_discount (plan_id, percent, code) VALUES ('yearly', 53, 'INTRO53')"
    ).run();
    const { handleGetPricing } = await import("../src/billing.js");
    const r = await handleGetPricing(env);
    const body = await r.json();
    const yearly = body.plans.find((p) => p.id === "yearly");
    expect(yearly.discount).toEqual({ percent: 53, code: "INTRO53" });
    // Monthly stays null
    expect(body.plans.find((p) => p.id === "monthly").discount).toBeNull();
  });

  it("supports different discounts per plan", async () => {
    await env.CLAUGE_DB.prepare(
      "INSERT OR REPLACE INTO billing_discount (plan_id, percent, code) VALUES ('monthly', 53, 'INTRO53M')"
    ).run();
    await env.CLAUGE_DB.prepare(
      "INSERT OR REPLACE INTO billing_discount (plan_id, percent, code) VALUES ('yearly', 30, 'YEAR30')"
    ).run();
    const { handleGetPricing } = await import("../src/billing.js");
    const r = await handleGetPricing(env);
    const body = await r.json();
    expect(body.plans.find((p) => p.id === "monthly").discount).toEqual({ percent: 53, code: "INTRO53M" });
    expect(body.plans.find((p) => p.id === "yearly").discount).toEqual({ percent: 30, code: "YEAR30" });
  });

  it("supports auto-apply discount (code is null)", async () => {
    await env.CLAUGE_DB.prepare(
      "INSERT OR REPLACE INTO billing_discount (plan_id, percent, code) VALUES ('yearly', 20, NULL)"
    ).run();
    const { handleGetPricing } = await import("../src/billing.js");
    const r = await handleGetPricing(env);
    const body = await r.json();
    const yearly = body.plans.find((p) => p.id === "yearly");
    expect(yearly.discount).toEqual({ percent: 20, code: null });
  });
});

describe("POST /api/billing/checkout", () => {
  it("returns 401 without auth", async () => {
    const { handleCreateCheckout } = await import("../src/billing.js");
    const r = await handleCreateCheckout(
      new Request("https://x", { method: "POST", body: '{"plan":"monthly"}' }),
      env,
      null
    );
    expect(r.status).toBe(401);
  });

  it("returns 400 on invalid plan", async () => {
    const userId = await seedUser({ slug: "u_chk" });
    const { handleCreateCheckout } = await import("../src/billing.js");
    const r = await handleCreateCheckout(
      new Request("https://x", { method: "POST", body: '{"plan":"weekly"}' }),
      env,
      userId
    );
    expect(r.status).toBe(400);
  });

  it("returns a checkout url for monthly plan (Polar API mocked)", async () => {
    const userId = await seedUser({ slug: "u_chk2", email: "user@test.invalid" });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async (url, init) => {
      expect(String(url)).toContain("polar.sh");
      const body = JSON.parse(init.body);
      expect(body.products).toEqual([env.POLAR_PRODUCT_MONTHLY]);
      expect(body.external_customer_id).toBe(String(userId));
      return new Response(JSON.stringify({ url: "https://sandbox.polar.sh/checkout/abc" }), {
        status: 201,
        headers: { "content-type": "application/json" },
      });
    });
    try {
      const { handleCreateCheckout } = await import("../src/billing.js");
      const r = await handleCreateCheckout(
        new Request("https://x", {
          method: "POST",
          body: '{"plan":"monthly"}',
          headers: { "content-type": "application/json" },
        }),
        env,
        userId
      );
      expect(r.status).toBe(200);
      const payload = await r.json();
      expect(payload.url).toContain("polar.sh/checkout");
    } finally {
      fetchMock.mockRestore();
    }
  });
});

describe("polar_customer_id capture", () => {
  it("subscription.created sets polar_customer_id from data.customer_id", async () => {
    const userId = await seedUser({ slug: "u_cust_id" });
    const body = JSON.stringify({
      type: "subscription.created",
      data: {
        id: "sub_cust_1",
        status: "active",
        cancel_at_period_end: false,
        current_period_start: "2026-05-16T00:00:00Z",
        current_period_end: "2026-06-16T00:00:00Z",
        external_customer_id: String(userId),
        customer_id: "cust_polar_xyz",
        product_id: env.POLAR_PRODUCT_MONTHLY,
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT polar_customer_id FROM users WHERE user_id = ?"
    ).bind(userId).first();
    expect(row.polar_customer_id).toBe("cust_polar_xyz");
  });

  it("subscription.created accepts customer_id at data.customer.id (nested)", async () => {
    const userId = await seedUser({ slug: "u_cust_id_nested" });
    const body = JSON.stringify({
      type: "subscription.created",
      data: {
        id: "sub_cust_2",
        status: "active",
        cancel_at_period_end: false,
        current_period_start: "2026-05-16T00:00:00Z",
        current_period_end: "2026-06-16T00:00:00Z",
        external_customer_id: String(userId),
        customer: { id: "cust_polar_abc", external_id: String(userId) },
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT polar_customer_id FROM users WHERE user_id = ?"
    ).bind(userId).first();
    expect(row.polar_customer_id).toBe("cust_polar_abc");
  });
});

describe("subscription.uncanceled dispatch", () => {
  it("clears cancel_at_period_end when user un-cancels via portal", async () => {
    const userId = await seedUser({ slug: "u_uncancel" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', subscription_status='active', cancel_at_period_end=1, polar_subscription_id='sub_unc' WHERE user_id=?"
    ).bind(userId).run();
    const body = JSON.stringify({
      type: "subscription.uncanceled",
      data: {
        id: "sub_unc",
        status: "active",
        cancel_at_period_end: false,
        external_customer_id: String(userId),
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT cancel_at_period_end, subscription_status FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.cancel_at_period_end).toBe(0);
    expect(row.subscription_status).toBe("active");
  });
});

describe("subscription.active dispatch", () => {
  it("reconciles status to active after past_due recovery", async () => {
    const userId = await seedUser({ slug: "u_active" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', subscription_status='past_due', past_due_started_at=datetime('now','-1 day'), polar_subscription_id='sub_act' WHERE user_id=?"
    ).bind(userId).run();
    const body = JSON.stringify({
      type: "subscription.active",
      data: {
        id: "sub_act",
        status: "active",
        cancel_at_period_end: false,
        external_customer_id: String(userId),
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT subscription_status, past_due_started_at FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.subscription_status).toBe("active");
    expect(row.past_due_started_at).toBeNull();
  });
});

describe("order.paid handler — order-id idempotency", () => {
  it("does not regrant credits when the same order.id is delivered twice with different webhook-ids", async () => {
    const userId = await seedUser({ slug: "u_order_dup" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', subscription_status='active',
         polar_subscription_id='sub_d1',
         current_period_start='2026-05-16T00:00:00Z',
         current_period_end='2026-06-16T00:00:00Z',
         credit_allowance_per_cycle=1000, credits_remaining=0
       WHERE user_id=?`
    ).bind(userId).run();
    const body = JSON.stringify({
      type: "order.paid",
      data: {
        id: "ord_dup_same",
        subscription_id: "sub_d1",
        product_id: env.POLAR_PRODUCT_MONTHLY,
        external_customer_id: String(userId),
      },
    });
    // First delivery — grants 1000.
    await postWebhook(body, { id: "msg_first" });
    // Burn some credits to detect a wrongful regrant.
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET credits_remaining = 100 WHERE user_id=?"
    ).bind(userId).run();
    // Second delivery, same order.id, fresh webhook-id — must be a no-op.
    await postWebhook(body, { id: "msg_second" });
    const row = await env.CLAUGE_DB.prepare(
      "SELECT credits_remaining, last_granted_order_id FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.credits_remaining).toBe(100); // not reset to 1000
    expect(row.last_granted_order_id).toBe("ord_dup_same");
  });

  it("DOES grant for a new order.id (next billing cycle's order)", async () => {
    const userId = await seedUser({ slug: "u_next_cycle" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', subscription_status='active',
         polar_subscription_id='sub_nc',
         current_period_start='2026-05-16T00:00:00Z',
         current_period_end='2026-06-16T00:00:00Z',
         credit_allowance_per_cycle=1000, credits_remaining=0,
         last_granted_order_id='ord_prev'
       WHERE user_id=?`
    ).bind(userId).run();
    const body = JSON.stringify({
      type: "order.paid",
      data: {
        id: "ord_new_cycle",
        subscription_id: "sub_nc",
        product_id: env.POLAR_PRODUCT_MONTHLY,
        external_customer_id: String(userId),
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT credits_remaining, last_granted_order_id FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.credits_remaining).toBe(1000);
    expect(row.last_granted_order_id).toBe("ord_new_cycle");
  });
});

describe("order.paid handler — lifetime user safety", () => {
  it("recurring fallback no-ops against a lifetime user (env-var drift defense)", async () => {
    const userId = await seedUser({ slug: "u_lifetime_recurring" });
    // Seed a lifetime user with their 20k bucket and some spend.
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', is_lifetime=1, subscription_status='active',
         polar_lifetime_order_id='ord_lifetime_orig',
         credit_allowance_per_cycle=20000, credits_remaining=18500,
         current_period_start=CURRENT_TIMESTAMP, current_period_end=NULL
       WHERE user_id=?`
    ).bind(userId).run();
    // An order.paid arrives where product_id doesn't match any configured product
    // (productIdToPlan → null). Without the is_lifetime guard the recurring path
    // would overwrite credits to allowance × months.
    const body = JSON.stringify({
      type: "order.paid",
      data: {
        id: "ord_drift",
        product_id: "prod_unknown_drift",
        external_customer_id: String(userId),
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT is_lifetime, credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.is_lifetime).toBe(1);
    expect(row.credits_remaining).toBe(18500); // untouched
  });
});

describe("lifetime purchase — handleLifetimeOrderPaid", () => {
  it("grants 20,000 credits exactly once, sets is_lifetime=1 and current_period_end=NULL", async () => {
    const userId = await seedUser({ slug: "u_lifetime_buy" });
    const body = JSON.stringify({
      type: "order.paid",
      data: {
        id: "ord_lifetime_1",
        product_id: env.POLAR_PRODUCT_LIFETIME,
        external_customer_id: String(userId),
        customer_id: "cust_lifetime_1",
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      `SELECT plan, is_lifetime, subscription_status, credits_remaining,
              credit_allowance_per_cycle, current_period_end,
              polar_lifetime_order_id, polar_customer_id
         FROM users WHERE user_id = ?`
    ).bind(userId).first();
    expect(row.plan).toBe("pro");
    expect(row.is_lifetime).toBe(1);
    expect(row.subscription_status).toBe("active");
    expect(row.credits_remaining).toBe(20000);
    expect(row.credit_allowance_per_cycle).toBe(20000);
    expect(row.current_period_end).toBeNull();
    expect(row.polar_lifetime_order_id).toBe("ord_lifetime_1");
    expect(row.polar_customer_id).toBe("cust_lifetime_1");
  });

  it("does not regrant on duplicate order.paid with same order id", async () => {
    const userId = await seedUser({ slug: "u_lifetime_dup" });
    const body = JSON.stringify({
      type: "order.paid",
      data: {
        id: "ord_lifetime_dup",
        product_id: env.POLAR_PRODUCT_LIFETIME,
        external_customer_id: String(userId),
      },
    });
    await postWebhook(body, { id: "msg_lt_1" });
    // Burn credits so a regrant would be visible.
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET credits_remaining=100 WHERE user_id=?"
    ).bind(userId).run();
    await postWebhook(body, { id: "msg_lt_2" });
    const row = await env.CLAUGE_DB.prepare(
      "SELECT credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.credits_remaining).toBe(100); // not refilled to 20000
  });

  it("order.refunded on a lifetime user clears is_lifetime and revokes Pro", async () => {
    const userId = await seedUser({ slug: "u_lifetime_refund" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', is_lifetime=1, subscription_status='active',
         polar_lifetime_order_id='ord_lt_refund',
         credits_remaining=15000
       WHERE user_id = ?`
    ).bind(userId).run();
    const body = JSON.stringify({
      type: "order.refunded",
      data: { id: "ord_lt_refund", external_customer_id: String(userId) },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT plan, is_lifetime, credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.plan).toBe("free");
    expect(row.is_lifetime).toBe(0);
    expect(row.credits_remaining).toBe(0);
  });
});

describe("subscription.past_due dispatch", () => {
  it("stamps past_due_started_at when explicit past_due event fires", async () => {
    const userId = await seedUser({ slug: "u_past_due_explicit" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', subscription_status='active', polar_subscription_id='sub_pde' WHERE user_id=?"
    ).bind(userId).run();
    const body = JSON.stringify({
      type: "subscription.past_due",
      data: {
        id: "sub_pde",
        status: "past_due",
        cancel_at_period_end: false,
        external_customer_id: String(userId),
      },
    });
    await postWebhook(body);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT subscription_status, past_due_started_at FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.subscription_status).toBe("past_due");
    expect(row.past_due_started_at).not.toBeNull();
  });
});
