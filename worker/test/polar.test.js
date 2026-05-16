import { describe, it, expect } from "vitest";
import { env } from "cloudflare:test";
import { verifyPolarSignature, checkReplayWindow } from "../src/polar.js";

async function makeSig(body, secret = "test_webhook_secret") {
  const enc = new TextEncoder();
  const key = await crypto.subtle.importKey(
    "raw",
    enc.encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"]
  );
  const mac = await crypto.subtle.sign("HMAC", key, enc.encode(body));
  const bytes = Array.from(new Uint8Array(mac));
  return bytes.map((b) => b.toString(16).padStart(2, "0")).join("");
}

describe("verifyPolarSignature", () => {
  it("returns true for a valid signature", async () => {
    const body = '{"type":"order.paid"}';
    const sig = await makeSig(body);
    expect(await verifyPolarSignature(body, sig, env)).toBe(true);
  });

  it("returns false when body is tampered", async () => {
    const goodBody = '{"type":"order.paid"}';
    const sig = await makeSig(goodBody);
    const tamperedBody = '{"type":"order.refunded"}';
    expect(await verifyPolarSignature(tamperedBody, sig, env)).toBe(false);
  });

  it("returns false when signature is junk", async () => {
    expect(await verifyPolarSignature("body", "deadbeef", env)).toBe(false);
  });

  it("returns false when signature has wrong length", async () => {
    expect(await verifyPolarSignature("body", "abc", env)).toBe(false);
  });
});

describe("checkReplayWindow", () => {
  it("accepts a timestamp within 5 minutes", () => {
    const recent = new Date(Date.now() - 60_000).toISOString();
    expect(checkReplayWindow(recent)).toBe(true);
  });

  it("rejects a timestamp older than 5 minutes", () => {
    const old = new Date(Date.now() - 6 * 60_000).toISOString();
    expect(checkReplayWindow(old)).toBe(false);
  });

  it("rejects unparseable timestamps", () => {
    expect(checkReplayWindow("not-a-date")).toBe(false);
  });
});
