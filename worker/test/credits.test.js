import { describe, it, expect, beforeEach } from "vitest";
import { env } from "cloudflare:test";
import { precheckBalance, deductCredits, classifyOperation, computeChargeCredits, reserveCredits, refundReservation } from "../src/credits.js";
import { seedUser } from "./setup.js";

describe("classifyOperation", () => {
  it("returns 'chat' by default", () => {
    expect(classifyOperation({})).toBe("chat");
  });
  it("returns 'tool_call_round' when tools present", () => {
    expect(classifyOperation({ tools: [{}] })).toBe("tool_call_round");
  });
});

describe("computeChargeCredits", () => {
  const weights = {
    operations: {
      chat: { base: 1, long_ctx_threshold_tokens: 8000, long_ctx_multiplier: 2 },
      tool_call_round: { base: 3 },
    },
    cost_to_clauge_credit_divisor_usd: 0.01,
    min_credits_per_call: 1,
  };

  it("uses base when actual cost is small", () => {
    expect(computeChargeCredits("chat", 4000, 500, weights)).toBe(1);
  });
  it("scales up with cost", () => {
    // 0.05 USD / 0.01 divisor = 5
    expect(computeChargeCredits("chat", 4000, 50_000, weights)).toBe(5);
  });
  it("applies long-context multiplier", () => {
    // 10000 tokens > 8000 threshold → base × 2 = 2, max(2, 0) = 2
    expect(computeChargeCredits("chat", 10000, 0, weights)).toBe(2);
  });
  it("never goes below min_credits_per_call", () => {
    expect(computeChargeCredits("chat", 0, 0, weights)).toBe(1);
  });
});

describe("precheckBalance", () => {
  beforeEach(async () => {
    await env.CLAUGE_DB.prepare("DELETE FROM users").run();
  });
  it("passes when balance >= estimate", async () => {
    const userId = await seedUser({ slug: "u_pre1" });
    await env.CLAUGE_DB.prepare("UPDATE users SET credits_remaining=10 WHERE user_id=?")
      .bind(userId).run();
    expect(await precheckBalance(userId, 5, env)).toBe(true);
  });
  it("fails when balance < estimate", async () => {
    const userId = await seedUser({ slug: "u_pre2" });
    await env.CLAUGE_DB.prepare("UPDATE users SET credits_remaining=3 WHERE user_id=?")
      .bind(userId).run();
    expect(await precheckBalance(userId, 5, env)).toBe(false);
  });
});

describe("deductCredits (compare-and-swap)", () => {
  beforeEach(async () => {
    await env.CLAUGE_DB.prepare("DELETE FROM users").run();
    await env.CLAUGE_DB.prepare("DELETE FROM credit_usage_log").run();
  });

  it("deducts and logs when balance sufficient", async () => {
    const userId = await seedUser({ slug: "u_d1" });
    await env.CLAUGE_DB.prepare("UPDATE users SET credits_remaining=20 WHERE user_id=?")
      .bind(userId).run();
    const ok = await deductCredits(userId, {
      operation: "chat",
      clauge_credits: 7,
      cost_usd_micros: 7000,
      request_id: "req_test_1",
    }, env);
    expect(ok).toBe(true);
    const row = await env.CLAUGE_DB.prepare("SELECT credits_remaining FROM users WHERE user_id=?")
      .bind(userId).first();
    expect(row.credits_remaining).toBe(13);
    const log = await env.CLAUGE_DB.prepare(
      "SELECT COUNT(*) AS n FROM credit_usage_log WHERE user_id=? AND request_id=?"
    ).bind(userId, "req_test_1").first();
    expect(log.n).toBe(1);
  });

  it("returns false when balance insufficient (no deduction, no log)", async () => {
    const userId = await seedUser({ slug: "u_d2" });
    await env.CLAUGE_DB.prepare("UPDATE users SET credits_remaining=3 WHERE user_id=?")
      .bind(userId).run();
    const ok = await deductCredits(userId, {
      operation: "chat",
      clauge_credits: 10,
      cost_usd_micros: 10000,
      request_id: "req_test_2",
    }, env);
    expect(ok).toBe(false);
    const row = await env.CLAUGE_DB.prepare("SELECT credits_remaining FROM users WHERE user_id=?")
      .bind(userId).first();
    expect(row.credits_remaining).toBe(3);
  });

  it("is idempotent per (user_id, request_id)", async () => {
    const userId = await seedUser({ slug: "u_d3" });
    await env.CLAUGE_DB.prepare("UPDATE users SET credits_remaining=100 WHERE user_id=?")
      .bind(userId).run();
    const args = {
      operation: "chat",
      clauge_credits: 5,
      cost_usd_micros: 5000,
      request_id: "req_test_3",
    };
    const ok1 = await deductCredits(userId, args, env);
    const ok2 = await deductCredits(userId, args, env);
    expect(ok1).toBe(true);
    expect(ok2).toBe(true); // second is no-op success
    const row = await env.CLAUGE_DB.prepare("SELECT credits_remaining FROM users WHERE user_id=?")
      .bind(userId).first();
    expect(row.credits_remaining).toBe(95); // only deducted once
  });
});

describe("reserveCredits (atomic CAS)", () => {
  beforeEach(async () => {
    await env.CLAUGE_DB.prepare("DELETE FROM users").run();
  });

  it("debits balance on success", async () => {
    const userId = await seedUser({ slug: "u_res1" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', credits_remaining=10 WHERE user_id=?"
    ).bind(userId).run();
    expect(await reserveCredits(userId, 3, env)).toBe(true);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.credits_remaining).toBe(7);
  });

  it("returns false (no debit) when balance insufficient", async () => {
    const userId = await seedUser({ slug: "u_res2" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', credits_remaining=2 WHERE user_id=?"
    ).bind(userId).run();
    expect(await reserveCredits(userId, 5, env)).toBe(false);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.credits_remaining).toBe(2);
  });

  it("refuses non-pro users (free tier can't reserve)", async () => {
    const userId = await seedUser({ slug: "u_res3" });
    // plan defaults to 'free'
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET credits_remaining=100 WHERE user_id=?"
    ).bind(userId).run();
    expect(await reserveCredits(userId, 1, env)).toBe(false);
  });

  it("two sequential reservations of 1 at balance=1: first wins, second fails", async () => {
    // Closes the free-response-on-near-zero-balance race. Two concurrent
    // requests both passing a read-only precheck would each get a free
    // response; with CAS, only one wins.
    const userId = await seedUser({ slug: "u_res_race" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', credits_remaining=1 WHERE user_id=?"
    ).bind(userId).run();
    expect(await reserveCredits(userId, 1, env)).toBe(true);
    expect(await reserveCredits(userId, 1, env)).toBe(false);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.credits_remaining).toBe(0);
  });
});

describe("refundReservation", () => {
  beforeEach(async () => {
    await env.CLAUGE_DB.prepare("DELETE FROM users").run();
  });

  it("restores the reserved amount", async () => {
    const userId = await seedUser({ slug: "u_refund1" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', credits_remaining=5 WHERE user_id=?"
    ).bind(userId).run();
    await refundReservation(userId, 3, env);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.credits_remaining).toBe(8);
  });

  it("is a no-op for non-positive amounts", async () => {
    const userId = await seedUser({ slug: "u_refund2" });
    await env.CLAUGE_DB.prepare(
      "UPDATE users SET plan='pro', credits_remaining=5 WHERE user_id=?"
    ).bind(userId).run();
    await refundReservation(userId, 0, env);
    await refundReservation(userId, -2, env);
    const row = await env.CLAUGE_DB.prepare(
      "SELECT credits_remaining FROM users WHERE user_id=?"
    ).bind(userId).first();
    expect(row.credits_remaining).toBe(5);
  });
});
