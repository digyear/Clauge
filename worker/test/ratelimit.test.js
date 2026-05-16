import { describe, it, expect, beforeEach } from "vitest";
import { env } from "cloudflare:test";
import { checkRpm, checkBurstBudget } from "../src/ratelimit.js";

describe("checkRpm", () => {
  beforeEach(async () => {
    const list = await env.CLAUGE_KV.list({ prefix: "rl:" });
    for (const k of list.keys) await env.CLAUGE_KV.delete(k.name);
  });

  it("allows requests up to limit, blocks beyond", async () => {
    const userId = 42;
    const limit = 3;
    expect(await checkRpm(userId, limit, env)).toBe(true);
    expect(await checkRpm(userId, limit, env)).toBe(true);
    expect(await checkRpm(userId, limit, env)).toBe(true);
    expect(await checkRpm(userId, limit, env)).toBe(false);
  });

  it("isolates users from each other", async () => {
    const limit = 1;
    expect(await checkRpm(1, limit, env)).toBe(true);
    expect(await checkRpm(2, limit, env)).toBe(true);
    expect(await checkRpm(1, limit, env)).toBe(false);
    expect(await checkRpm(2, limit, env)).toBe(false);
  });
});

describe("checkBurstBudget", () => {
  beforeEach(async () => {
    const list = await env.CLAUGE_KV.list({ prefix: "burst:" });
    for (const k of list.keys) await env.CLAUGE_KV.delete(k.name);
  });

  it("allows credits up to the burst cap within the window", async () => {
    const userId = 42;
    const allowancePerCycle = 1000;
    const fraction = 0.10;  // 100 credits per hour cap
    const windowSeconds = 3600;
    expect(await checkBurstBudget(userId, allowancePerCycle, fraction, windowSeconds, 50, env)).toBe(true);
    expect(await checkBurstBudget(userId, allowancePerCycle, fraction, windowSeconds, 50, env)).toBe(true);
    expect(await checkBurstBudget(userId, allowancePerCycle, fraction, windowSeconds, 1, env)).toBe(false);
  });
});
