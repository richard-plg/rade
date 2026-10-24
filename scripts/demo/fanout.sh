#!/usr/bin/env bash
# Demo scene: Fan-out — one prompt to three Agents (claude, codex, claude),
# each in its own Worktree with a different rate limiter; a Judge picks one;
# you choose the Winner. Stills: fanout (the Fan-out Tab with its bar),
# fanout-verdict (Judge's pick), fanout-form (palette Fan-out… form),
# fanout-chosen; recording fanout.mp4/.gif (a second, identical run so the
# film never flips the color scheme). Usage: scripts/demo/fanout.sh (NO_BUILD=1, KEEP=1)
set -uo pipefail
NAME=fanout
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"

# Plays: the Judge's prompt mentions "tests", which 20-tests would answer, so
# its play goes first in this scene's copy of the plays folder.
PLAYS=$T/plays FIX=$T/fix
mkdir -p "$PLAYS" && cp "$DEMO"/plays/*.play "$PLAYS/" && cp "$DEMO/plays/68-judge.play" "$PLAYS/00-judge.play"
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<C
idle_seconds = 30
[agents.claude]
command = "env DEMO_AGENT=claude DEMO_TITLE='Claude Code (demo)' DEMO_PLAYS=$PLAYS DEMO_FIX=$FIX $AGENT"
[agents.codex]
command = "env DEMO_AGENT=codex DEMO_TITLE='Codex (demo)' DEMO_PLAYS=$PLAYS DEMO_FIX=$FIX $AGENT"
[fanout]
judge = "claude"
C

# What each Agent does (plays/69-ratelimit.play runs $FIX/<n>/work.play).
mkdir -p "$FIX"/{1,2,3}/files/src/middleware
ROUTES_HEAD='import { orders } from "./orders/handler";'
ROUTES_TAIL='  if (req.url?.startsWith("/orders")) return orders(req, res);
  res.statusCode = 404;
  res.end();
}'
# 1 (claude): token bucket + Retry-After + tests
cat >"$FIX/1/files/src/middleware/rateLimit.ts" <<'F'
type Bucket = { tokens: number; at: number };

const buckets = new Map<string, Bucket>();
const CAPACITY = 20;
const REFILL_PER_SEC = 5;

export function take(key: string, now = Date.now()) {
  const b = buckets.get(key) ?? { tokens: CAPACITY, at: now };
  b.tokens = Math.min(CAPACITY, b.tokens + ((now - b.at) / 1000) * REFILL_PER_SEC);
  b.at = now;
  const ok = b.tokens >= 1;
  if (ok) b.tokens -= 1;
  buckets.set(key, b);
  return { ok, retryAfter: ok ? 0 : Math.ceil((1 - b.tokens) / REFILL_PER_SEC) };
}
F
cat >"$FIX/1/files/src/middleware/rateLimit.test.ts" <<'F'
import { describe, it, expect } from "vitest";
import { take } from "./rateLimit";

describe("take", () => {
  it("allows a burst up to the capacity", () => {
    const allowed = Array.from({ length: 21 }, () => take("a", 0).ok);
    expect(allowed.filter(Boolean)).toHaveLength(20);
  });

  it("refills over time", () => {
    for (let i = 0; i < 20; i++) take("b", 0);
    expect(take("b", 1000).ok).toBe(true);
  });
});
F
cat >"$FIX/1/files/src/routes.ts" <<F
$ROUTES_HEAD
import { take } from "./middleware/rateLimit";

export function routes(req, res) {
  const { ok, retryAfter } = take(req.socket.remoteAddress ?? "local");
  if (!ok) {
    res.statusCode = 429;
    res.setHeader("Retry-After", String(retryAfter));
    return res.end();
  }
$ROUTES_TAIL
F
cat >"$FIX/1/work.play" <<F
say Adding a token bucket per client IP.
tool Read src/routes.ts
out Read 7 lines
tool Write src/middleware/rateLimit.ts
out Wrote 15 lines
tool Write src/middleware/rateLimit.test.ts
out Wrote 14 lines
tool Edit src/routes.ts
add if (!ok) { res.statusCode = 429; …
sh cp -r "\$DEMO_FIX/1/files/." .
tool Bash npm test
out ✓ rateLimit.test.ts (2 tests) 6ms
out Test Files  1 passed (1)
say Done: 20 requests, then 5 a second, with
out 429 and a Retry-After header.
F
# 2 (codex): fixed window counter, inline
cat >"$FIX/2/files/src/routes.ts" <<F
$ROUTES_HEAD

const hits = new Map<string, { n: number; start: number }>();

export function routes(req, res) {
  const ip = req.socket.remoteAddress ?? "local";
  const now = Date.now();
  const h = hits.get(ip);
  if (!h || now - h.start > 60_000) hits.set(ip, { n: 1, start: now });
  else if (++h.n > 100) {
    res.statusCode = 429;
    return res.end();
  }
$ROUTES_TAIL
F
rm -rf "$FIX/2/files/src/middleware"
cat >"$FIX/2/work.play" <<F
say Counting requests per IP, 100 a minute.
tool Read src/routes.ts
out Read 7 lines
tool Edit src/routes.ts
add const hits = new Map<string, …>();
add else if (++h.n > 100) { 429 }
sh cp -r "\$DEMO_FIX/2/files/." .
say Done. Over the limit gets a 429.
F
# 3 (claude): sliding window, no tests
cat >"$FIX/3/files/src/middleware/slidingWindow.ts" <<'F'
const log = new Map<string, number[]>();

export function allow(key: string, limit = 60, windowMs = 60_000, now = Date.now()) {
  const recent = (log.get(key) ?? []).filter((t) => now - t < windowMs);
  const ok = recent.length < limit;
  if (ok) recent.push(now);
  log.set(key, recent);
  return ok;
}
F
cat >"$FIX/3/files/src/routes.ts" <<F
$ROUTES_HEAD
import { allow } from "./middleware/slidingWindow";

export function routes(req, res) {
  if (!allow(req.socket.remoteAddress ?? "local")) {
    res.statusCode = 429;
    return res.end("Too many requests");
  }
$ROUTES_TAIL
F
cat >"$FIX/3/work.play" <<F
say Sliding window: 60 requests a minute.
tool Read src/routes.ts
out Read 7 lines
tool Write src/middleware/slidingWindow.ts
out Wrote 9 lines
tool Edit src/routes.ts
add if (!allow(ip)) { 429 "Too many…" }
sh cp -r "\$DEMO_FIX/3/files/." .
tool Bash npm run build
out compiled 9 modules in 0.7s
say Done. Exact over any 60 second span.
F

spawn() { rade spawn "$@" --json | id; }
fo() { rade fanout status limiter --json | jq -r "$1"; }
bar() { rade debug fanout "$1" info | jq -r "$2"; }
PROMPT="Add rate limiting to the orders API"

# fanout_run <name> : the three Agents, until they have all finished.
fanout_run() {
    FO=$(rade fanout --source shop-api --base develop --name "$1" --agents claude,codex,claude "$PROMPT" --json)
    P1=$(jq -r '.panes[0]' <<<"$FO")
    eventually 90 is "rade fanout status $1 --json | jq -r .phase" finished
    sleep 1.5
    rade focus "$P1" >/dev/null
    eventually 15 is "bar $1 .progress" "3/3 done"
    sleep 1
}
# fanout_judge <name>
fanout_judge() {
    rade fanout judge "$1" >/dev/null
    eventually 90 is "rade fanout status $1 --json | jq -r .phase" judged
    sleep 2
    rade focus "$P1" >/dev/null
    sleep 1
}

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
acme_register
rade workspace switch acme >/dev/null

echo "== stills"
fanout_run limiter
notoasts
still fanout
fanout_judge limiter
notoasts
still fanout-verdict
# The palette's Fan-out… form.
rade debug action command_palette >/dev/null
rade debug palette --ui type ">fan-out" >/dev/null
rade debug palette --ui key enter | jq -c '[.form.fields[] | [.key, .kind]]'
rade debug palette --ui field "prompt=$PROMPT" >/dev/null
rade debug palette --ui field "agents=claude=2,codex=1" | jq -c '[.form.fields[1].value, .form.can_submit]'
rade debug palette --ui field "base=develop" >/dev/null
sleep 0.8
still fanout-form
rade debug palette --ui key escape >/dev/null
rade debug palette --ui key escape >/dev/null
sleep 0.8
# Choose the Judge's Winner: staged in the main checkout, never committed.
rade fanout choose limiter --json | jq -c '[.winner, .target, .staged, (.conflicts | length)]'
sleep 2
notoasts
still fanout-chosen

echo "== recording (a second run of the same story)"
rade fanout discard limiter >/dev/null
git -C "$API" reset -q --hard && git -C "$API" clean -qfd
sleep 1
rec_start fanout
sleep 1
fanout_run limiter
sleep 1.5
fanout_judge limiter
sleep 2.5
rade fanout choose limiter >/dev/null
sleep 3
rec_stop
