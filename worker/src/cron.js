// Scheduled sweep: revoke subscriptions that should no longer entitle users
// to Pro. Two cases:
//
// 1. Past-due longer than 3 days. Polar retries the charge for several days
//    after initial failure; we let that play out before revoking, so a user
//    whose card recovers on day 2 isn't kicked.
//
// 2. End-of-period cancellation that has passed its current_period_end.
//    The authoritative signal for this is Polar's `subscription.revoked`
//    webhook — but our Polar dashboard subscribes only `subscription.{created,
//    active,uncanceled,canceled,past_due}`. The dispatch in billing.js handles
//    `revoked` if it arrives, but if the operator hasn't enabled that event
//    (or a delivery is lost), the user would otherwise sit at plan='pro'
//    forever. This sweep is the belt-and-suspenders fallback.
//
// Both branches filter is_lifetime=0 — lifetime users have current_period_end
// NULL and are not subject to recurring revocation.

export async function sweepPastDue(env) {
  const pastDue = await env.CLAUGE_DB.prepare(
    `UPDATE users SET
       plan = 'free',
       subscription_status = 'canceled',
       cancel_at_period_end = 0,
       credits_remaining = 0,
       past_due_started_at = NULL,
       updated_at = CURRENT_TIMESTAMP
     WHERE subscription_status = 'past_due'
       AND past_due_started_at < datetime('now', '-3 days')
       AND is_lifetime = 0`
  ).run();

  const periodEnd = await env.CLAUGE_DB.prepare(
    `UPDATE users SET
       plan = 'free',
       subscription_status = 'canceled',
       cancel_at_period_end = 0,
       credits_remaining = 0,
       past_due_started_at = NULL,
       updated_at = CURRENT_TIMESTAMP
     WHERE plan = 'pro'
       AND cancel_at_period_end = 1
       AND current_period_end IS NOT NULL
       AND current_period_end < CURRENT_TIMESTAMP
       AND is_lifetime = 0`
  ).run();

  return (pastDue.meta?.changes ?? 0) + (periodEnd.meta?.changes ?? 0);
}
