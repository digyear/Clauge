import { describe, it, expect, beforeEach } from "vitest";
import { env } from "cloudflare:test";
import { sweepPastDue } from "../src/cron.js";
import { seedUser } from "./setup.js";

describe("sweepPastDue", () => {
  beforeEach(async () => {
    await env.CLAUGE_DB.prepare("DELETE FROM users").run();
  });

  it("revokes users past_due for more than 3 days", async () => {
    const userId = await seedUser({ slug: "u_old_pd" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', subscription_status='past_due',
         past_due_started_at = datetime('now', '-4 days'),
         credits_remaining=500
       WHERE user_id = ?`
    )
      .bind(userId)
      .run();
    const swept = await sweepPastDue(env);
    expect(swept).toBe(1);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT plan, subscription_status, credits_remaining FROM users WHERE user_id=?"
    )
      .bind(userId)
      .first();
    expect(row.plan).toBe("free");
    expect(row.subscription_status).toBe("canceled");
    expect(row.credits_remaining).toBe(0);
  });

  it("leaves users past_due for less than 3 days alone", async () => {
    const userId = await seedUser({ slug: "u_new_pd" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', subscription_status='past_due',
         past_due_started_at = datetime('now', '-1 day'),
         credits_remaining=400
       WHERE user_id = ?`
    )
      .bind(userId)
      .run();
    const swept = await sweepPastDue(env);
    expect(swept).toBe(0);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT plan, subscription_status FROM users WHERE user_id=?"
    )
      .bind(userId)
      .first();
    expect(row.plan).toBe("pro");
    expect(row.subscription_status).toBe("past_due");
  });

  it("ignores active and free users entirely", async () => {
    const u1 = await seedUser({ slug: "u_active" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', subscription_status='active' WHERE user_id=?"
    ).bind(u1).run();
    const u2 = await seedUser({ slug: "u_free" });
    const swept = await sweepPastDue(env);
    expect(swept).toBe(0);
  });

  it("leaves lifetime users alone even when past_due > 3 days (shouldn't happen, but safe)", async () => {
    const userId = await seedUser({ slug: "u_lifetime_pd" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', is_lifetime=1, subscription_status='past_due',
         past_due_started_at = datetime('now', '-5 days'),
         credits_remaining=15000
       WHERE user_id = ?`
    ).bind(userId).run();
    const swept = await sweepPastDue(env);
    expect(swept).toBe(0);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT plan, is_lifetime, credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.plan).toBe("pro");
    expect(row.is_lifetime).toBe(1);
    expect(row.credits_remaining).toBe(15000);
  });
});

describe("sweepPastDue — period-end fallback (defensive revoke)", () => {
  beforeEach(async () => {
    await env.CLAUGE_DB.prepare("DELETE FROM users").run();
  });

  it("revokes a canceled user whose period has ended (Polar revoked event never arrived)", async () => {
    const userId = await seedUser({ slug: "u_period_end" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', subscription_status='active',
         cancel_at_period_end=1,
         current_period_end = datetime('now', '-1 hour'),
         credits_remaining=300
       WHERE user_id = ?`
    ).bind(userId).run();
    const swept = await sweepPastDue(env);
    expect(swept).toBe(1);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT plan, subscription_status, cancel_at_period_end, credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.plan).toBe("free");
    expect(row.subscription_status).toBe("canceled");
    expect(row.cancel_at_period_end).toBe(0);
    expect(row.credits_remaining).toBe(0);
  });

  it("leaves a canceled user whose period is still in the future", async () => {
    const userId = await seedUser({ slug: "u_period_future" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', subscription_status='active',
         cancel_at_period_end=1,
         current_period_end = datetime('now', '+5 days'),
         credits_remaining=500
       WHERE user_id = ?`
    ).bind(userId).run();
    const swept = await sweepPastDue(env);
    expect(swept).toBe(0);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT plan, credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.plan).toBe("pro");
    expect(row.credits_remaining).toBe(500);
  });

  it("never revokes a lifetime user (current_period_end is NULL)", async () => {
    const userId = await seedUser({ slug: "u_lifetime_safe" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', is_lifetime=1, subscription_status='active',
         current_period_end = NULL,
         credits_remaining=20000
       WHERE user_id = ?`
    ).bind(userId).run();
    const swept = await sweepPastDue(env);
    expect(swept).toBe(0);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT plan, is_lifetime, credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.plan).toBe("pro");
    expect(row.is_lifetime).toBe(1);
    expect(row.credits_remaining).toBe(20000);
  });

  it("does not revoke if cancel_at_period_end is 0 even with past period_end", async () => {
    // Shouldn't happen in practice (active sub renews and updates period bounds),
    // but if it did, we don't want to revoke — only canceled users.
    const userId = await seedUser({ slug: "u_active_stale_period" });
    await env.CLAUGE_DB.prepare(
      `UPDATE users SET plan='pro', subscription_status='active',
         cancel_at_period_end=0,
         current_period_end = datetime('now', '-1 day'),
         credits_remaining=500
       WHERE user_id = ?`
    ).bind(userId).run();
    const swept = await sweepPastDue(env);
    expect(swept).toBe(0);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT plan FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.plan).toBe("pro");
  });
});
