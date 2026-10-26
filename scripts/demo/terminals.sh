#!/usr/bin/env bash
# Demo scene: Terminals — splits (dev server, tests, an Agent, a shell), the
# close confirmation, Terminal search, Ctrl+click path → Viewer, the Changes
# page with a generated commit message, the Create Pull Request dialog and
# Settings. Stills only. Usage: scripts/demo/terminals.sh (NO_BUILD=1, KEEP=1)
set -uo pipefail
NAME=terminals
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"

# Real agent names, no "(demo)"; a Draft command and a gh that never leave the box.
export XDG_CONFIG_HOME=$HOME/.config
mkdir -p "$T/bin" "$XDG_CONFIG_HOME/rade"
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<C
idle_seconds = 30
[agents.claude]
command = "env DEMO_AGENT=claude DEMO_TITLE='Claude Code (demo)' $AGENT"
[agents.codex]
command = "env DEMO_AGENT=codex DEMO_TITLE='Codex (demo)' $AGENT"
[scm]
draft_command = "$T/bin/draft.sh"
C
cat >"$T/bin/draft.sh" <<'SH'
#!/bin/sh
cat >"$0.prompt"
if grep -q "pull request" "$0.prompt"; then
  printf 'Free shipping over $75 and guest checkout totals\n\n## Summary\n- Orders of $75 or more ship free; smaller orders pay a flat $4.99\n- Totals, tax and shipping are computed in cents on the cart\n- The receipt lists shipping as its own line\n\n## Testing\n- npm test (all green)\n'
else
  printf 'feat(checkout): ship free over $75\n\nOrders of $75 or more now skip the flat $4.99 shipping fee. The\ncart total, the receipt and the confirmation subject all go through\norderTotal(), so they pick the fee up without further changes.\n'
fi
SH
cat >"$T/bin/gh" <<'SH'
#!/bin/sh
echo "https://github.com/acme/shop-web/pull/128"
SH
cat >"$T/bin/vite.sh" <<'SH'
#!/bin/bash
printf '\n  \e[32m\e[1mVITE\e[0m \e[32mv6.3.1\e[0m  ready in \e[1m412\e[0m ms\n\n'
printf '  \e[32m➜\e[0m  \e[1mLocal\e[0m:   \e[36mhttp://localhost:\e[1m5173\e[0m\e[36m/\e[0m\n'
printf '  \e[2m➜  Network: use --host to expose\e[0m\n\n'
printf '\e[2m14:02:31\e[0m \e[36m\e[1m[vite]\e[0m \e[32mhmr update\e[0m \e[2m/src/cart/Cart.tsx\e[0m\n'
printf '\e[2m14:02:44\e[0m \e[36m\e[1m[vite]\e[0m \e[32mhmr update\e[0m \e[2m/src/cart/pricing.ts\e[0m\n'
printf '\e[2m14:03:02\e[0m \e[36m\e[1m[vite]\e[0m \e[32mhmr update\e[0m \e[2m/src/money.ts (x2)\e[0m\n'
exec sleep 86400
SH
cat >"$T/bin/vitest.sh" <<'SH'
#!/bin/bash
printf '\n \e[1m\e[46m DEV \e[49m\e[22m \e[36mv3.1.2\e[39m \e[2m~/workspaces/acme/shop-web\e[22m\n\n'
for f in "src/money.test.ts (4 tests) 3ms" "src/cart/useCart.test.ts (6 tests) 6ms" "src/cart/pricing.test.ts (5 tests) 4ms" "src/checkout/receipt.test.ts (3 tests) 5ms"; do
  printf ' \e[32m✓\e[39m %s\n' "$f"
done
printf '\n\e[2m Test Files \e[22m \e[1m\e[32m4 passed\e[39m\e[22m\e[90m (4)\e[39m\n'
printf '\e[2m      Tests \e[22m \e[1m\e[32m18 passed\e[39m\e[22m\e[90m (18)\e[39m\n'
printf '\e[2m   Start at \e[22m 14:03:07\n\e[2m   Duration \e[22m 412ms\n\n'
printf ' \e[1m\e[42m PASS \e[49m\e[22m \e[32mWaiting for file changes...\e[39m\n'
printf '       \e[2mpress \e[1mh\e[22m to show help, press \e[1mq\e[22m to quit\e[22m\n'
exec sleep 86400
SH
chmod +x "$T/bin/"*
export PATH=$T/bin:$PATH

# A fuller Cart.tsx, so Cart.tsx:12 is a real line.
commit "$WEB" "feat(cart): empty state and checkout button" \
    src/cart/Cart.tsx='import { useCart } from "./useCart";\nimport { formatPrice } from "../money";\n\ntype Props = { onCheckout?: () => void };\n\nexport function Cart({ onCheckout }: Props) {\n  const { items, total } = useCart();\n  if (items.length === 0) return <aside className="cart empty">Your cart is empty</aside>;\n\n  return (\n    <aside className="cart">\n      {items.map((i) => (\n        <div key={i.sku}>{i.name} × {i.qty}</div>\n      ))}\n      <strong>{formatPrice(total)}</strong>\n      <button onClick={onCheckout}>Check out</button>\n    </aside>\n  );\n}\n' \
    src/cart/pricing.ts='import { formatPrice, sumLines } from "../money";\n\nexport type CartLine = { sku: string; name: string; qty: number; price: number };\n\nconst TAX_RATE = 0.0825;\n\nexport function subtotal(lines: CartLine[]): number {\n  return sumLines(lines);\n}\n\nexport function tax(lines: CartLine[]): number {\n  return Math.round(subtotal(lines) * TAX_RATE);\n}\n\n/** What the customer pays: subtotal plus tax. */\nexport function orderTotal(lines: CartLine[]): number {\n  return subtotal(lines) + tax(lines);\n}\n\nexport function describeTotal(lines: CartLine[]): string {\n  return formatPrice(orderTotal(lines));\n}\n' \
    src/money.ts='export type Line = { qty: number; price: number };\n\n/** Formats an amount in cents as US dollars. */\nexport function formatPrice(cents: number): string {\n  return (cents / 100).toLocaleString("en-US", { style: "currency", currency: "USD" });\n}\n\n/** Sums `qty × price` over the lines, in cents. */\nexport function sumLines(lines: Line[]): number {\n  return lines.reduce((sum, l) => sum + l.qty * l.price, 0);\n}\n'
git -C "$WEB" push -q origin develop 2>/dev/null

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
acme_register
CW=$(rade worktree add shop-web guest-checkout --project shop --json | jq -r .path)
cd "$CW"
SP="--source shop-web --worktree guest-checkout"

# ---- splits: an Agent, the dev server, the test watcher, a shell ------------------
ag=$(rade spawn $SP --agent claude --name convert --tab new "Convert cart pricing to integer cents" --json | id)
sleep 4
tw=$(rade spawn $SP --name tests --cmd "$T/bin/vitest.sh" --split right --json | id)
rade focus "$tw" >/dev/null
dev=$(rade spawn $SP --name dev-server --cmd "$T/bin/vite.sh" --split down --json | id)
rade focus "$ag" >/dev/null
sh=$(rade spawn $SP --name shell --split down --json | id)
tab=$(rade tab list --json | jq -r '[.tabs[] | select(.panes | length > 1)][0].id // .tabs[-1].id')
rade tab rename "$tab" "guest-checkout" >/dev/null
sleep 1.5
rade send "$sh" "git --no-pager log --oneline -n 8" --enter >/dev/null
sleep 0.5
rade send "$sh" "git status -sb" --enter >/dev/null
sleep 1
rade focus "$ag" >/dev/null
sleep 1
notoasts
still splits

# ---- terminal-search: the search bar with hits in the shell ------------------------
rade focus "$sh" >/dev/null
rade debug search "$sh" open >/dev/null
rade debug search "$sh" type cart >/dev/null
rade debug search "$sh" next >/dev/null
sleep 0.5
rade debug search "$sh" next >/dev/null
sleep 1
notoasts
still terminal-search
rade debug search "$sh" close >/dev/null

# ---- close-confirm: closing the Tab names the working Agent -----------------------
rade focus "$ag" >/dev/null
rade debug action close_tab >/dev/null
eventually 5 bash -c "[[ \$(rade debug dialog | jq -r .open) == true ]]" || true
sleep 0.8
still close-confirm
rade debug dialog respond cancel >/dev/null
sleep 0.5

# ---- ctrl-click: an Agent's file:line opens a Viewer beside it ---------------------
au=$(rade spawn $SP --agent claude --name cart-audit --tab new "Audit the cart before checkout" --json | id)
eventually 40 is "pane_status $au" done || true
sleep 1
rade debug click "$au" "src/cart/Cart.tsx:12" >/dev/null
sleep 2.5
notoasts
still ctrl-click
V=$(rade debug state | jq -r '[.. | objects | select(.kind?.type? == "viewer")][0].id // empty')
[[ -n $V ]] && rade close "$V" --force >/dev/null 2>&1

# ---- changes-commit / pr-dialog: Changes with a generated message, then Create PR ----
cat >>"$CW/src/cart/pricing.ts" <<'TS'

const FREE_SHIPPING_FROM = 7500;

/** Flat $4.99, free from $75. */
export function shipping(lines: CartLine[]): number {
  return subtotal(lines) >= FREE_SHIPPING_FROM ? 0 : 499;
}
TS
sed -i 's|return subtotal(lines) + tax(lines);|return subtotal(lines) + tax(lines) + shipping(lines);|' "$CW/src/cart/pricing.ts"
mkdir -p "$CW/src/checkout"
{ printf 'import { shipping } from "../cart/pricing";\nimport type { CartLine } from "../cart/pricing";\n\nexport const shippingLabel = (lines: CartLine[]) =>\n  shipping(lines) === 0 ? "Free shipping" : "Shipping $4.99";\n' >"$CW/src/checkout/shipping.ts"; }
printf 'import { describe, it, expect } from "vitest";\nimport { shipping } from "./pricing";\n\ndescribe("shipping", () => {\n  it("is free from $75", () => expect(shipping([{ sku: "a", name: "A", qty: 1, price: 7500 }])).toBe(0));\n  it("is $4.99 below", () => expect(shipping([{ sku: "a", name: "A", qty: 1, price: 1800 }])).toBe(499));\n});\n' >"$CW/src/cart/pricing.test.ts"
scm() { rpc debug.scm "${1:-{\}}"; }
gmsg() { rpc debug.scm | jq -r '.cards[] | select(.source == "shop-web") | .message'; }
rade focus "$au" >/dev/null
rade debug action show_changes >/dev/null
eventually 15 bash -c "[[ \$(rpc debug.scm | jq '[.cards[] | select(.source == \"shop-web\") | .groups[].count] | add // 0') -ge 3 ]]" || true
sleep 1
for f in src/cart/pricing.ts src/cart/pricing.test.ts src/checkout/shipping.ts; do
    scm "{\"op\":\"row_action\",\"source\":\"shop-web\",\"group\":\"changes\",\"path\":\"$f\",\"arg\":\"Stage Changes\"}" >/dev/null
    sleep 0.6
done
rpc debug.ui '{"op":"click","widget":"draft-button"}' >/dev/null
eventually 20 bash -c "[[ -n \$(rpc debug.scm | jq -r '.cards[] | select(.source == \"shop-web\") | .message') ]]" || true
sleep 1.5
notoasts
still changes-commit
scm '{"op":"commit","source":"shop-web","variant":"commit"}' >/dev/null
sleep 2.5
scm '{"op":"card_menu_activate","source":"shop-web","item":"Create Pull Request…"}' >/dev/null
sleep 1
scm '{"op":"pr_generate"}' >/dev/null
eventually 15 bash -c "[[ -n \$(rpc debug.scm | jq -r .pr.title) ]]" || true
sleep 1
notoasts
still pr-dialog
rade debug dialog respond cancel >/dev/null 2>&1
rpc debug.ui '{"op":"key","keys":"Escape"}' >/dev/null
sleep 0.5

# ---- settings -----------------------------------------------------------------------
rpc debug.ui '{"op":"key","keys":"ctrl+comma"}' >/dev/null
sleep 1.5
notoasts
still settings
