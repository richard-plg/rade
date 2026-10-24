#!/usr/bin/env bash
# Demo scene: an Agent drives another through the `rade` CLI — Claude Code
# asks Codex to review its change (spawn → wait → read), then fixes what
# Codex found. Still + recording. Usage: scripts/demo/delegate.sh (NO_BUILD=1)
set -uo pipefail
NAME=delegate
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
acme_register
WT=$(rade worktree add shop-web coupons --project shop --json | jq -r .path)
# The change under review: coupon codes, committed on the Worktree's branch.
printf '\nconst COUPONS: Record<string, number> = { WELCOME10: 10 };\n\nexport function applyCoupon(total: number, code?: string) {\n  const pct = COUPONS[code ?? ""] ?? 0;\n  return Math.round((total * (100 - pct)) / 100);\n}\n' >>"$WT/src/cart/useCart.ts"
printf 'import { useState } from "react";\n\nexport function CouponField({ onApply }: { onApply: (code: string) => void }) {\n  const [code, setCode] = useState("");\n  return (\n    <form onSubmit={(e) => { e.preventDefault(); onApply(code); }}>\n      <input value={code} onChange={(e) => setCode(e.target.value)} />\n      <button>Apply</button>\n    </form>\n  );\n}\n' >"$WT/src/cart/CouponField.tsx"
git -C "$WT" add -A && git -C "$WT" commit -qm "feat(cart): coupon codes"
rec_start delegate
lead=$(rade spawn --source shop-web --worktree coupons --agent claude --name coupons --tab new "Ask Codex for a second opinion on the coupon change" --json | id)
rade focus "$lead" >/dev/null
eventually 60 is "pane_status $lead" done
sleep 2.5
rec_stop
rade focus "$lead" >/dev/null
notoasts
still delegate
