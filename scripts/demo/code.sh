#!/usr/bin/env bash
# Demo scene: code navigation and the Viewer — hover, peek, call hierarchy,
# outline on a TypeScript file of shop-web (a real typescript-language-server),
# Markdown with a Mermaid diagram, Search, Files. Stills only.
# Usage: scripts/demo/code.sh (NO_BUILD=1, KEEP=1)
set -uo pipefail
NAME=code
# Before lib.sh swaps HOME: the real language server and its typescript module.
export TSLS=$(command -v typescript-language-server || true)
export TSMOD=$( (timeout 20 npm root -g </dev/null 2>/dev/null || true) | sed 's|$|/typescript|')
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"
[[ -x $TSLS && -d $TSMOD ]] || { echo "needs typescript-language-server and a global typescript"; exit 1; }

code_ui() { rpc debug.code "${1:-{\}}"; }
pop() { rpc debug.code '{"op":"popover"}' | jq -r "$1"; }
vkey() { rade debug viewer "$1" key "$2" >/dev/null; }
find_() { rpc debug.find "${1:-{\}}"; }
export -f code_ui pop find_

# Real agent names, no "(demo)".
cat >"$XDG_CONFIG_HOME/rade/config.toml" <<C
idle_seconds = 30
[agents.claude]
command = "env DEMO_AGENT=claude DEMO_TITLE='Claude Code (demo)' $AGENT"
[agents.codex]
command = "env DEMO_AGENT=codex DEMO_TITLE='Codex (demo)' $AGENT"
[code.server.typescript]
command = ["$TSLS", "--stdio"]
C

# ---- More of shop-web: documented money helpers, pricing, receipt, a flow doc ----
printf 'node_modules\n' >"$WEB/.gitignore"
mkdir -p "$WEB/node_modules" && ln -s "$TSMOD" "$WEB/node_modules/typescript"
commit "$WEB" "feat(checkout): pricing and receipt" \
    tsconfig.json='{\n  "compilerOptions": {\n    "target": "es2022", "module": "esnext", "moduleResolution": "bundler",\n    "jsx": "preserve", "strict": true, "noEmit": true, "skipLibCheck": true\n  },\n  "include": ["src"]\n}\n' \
    .gitignore='node_modules\n' \
    src/money.ts='export type Line = { qty: number; price: number };\n\n/**\n * Formats an amount in cents as US dollars, e.g. `1800` → `"$18.00"`.\n * Negative amounts keep their sign (refunds).\n */\nexport function formatPrice(cents: number): string {\n  return (cents / 100).toLocaleString("en-US", { style: "currency", currency: "USD" });\n}\n\n/** Sums `qty × price` over the lines, in cents. */\nexport function sumLines(lines: Line[]): number {\n  return lines.reduce((sum, l) => sum + l.qty * l.price, 0);\n}\n' \
    src/cart/pricing.ts='import { formatPrice, sumLines } from "../money";\n\nexport type CartLine = { sku: string; name: string; qty: number; price: number };\n\nconst TAX_RATE = 0.0825;\n\nexport function subtotal(lines: CartLine[]): number {\n  return sumLines(lines);\n}\n\nexport function tax(lines: CartLine[]): number {\n  return Math.round(subtotal(lines) * TAX_RATE);\n}\n\n/** What the customer pays: subtotal plus tax. */\nexport function orderTotal(lines: CartLine[]): number {\n  return subtotal(lines) + tax(lines);\n}\n\nexport function describeTotal(lines: CartLine[]): string {\n  const n = lines.reduce((s, l) => s + l.qty, 0);\n  return `${n} items · ${formatPrice(orderTotal(lines))}`;\n}\n' \
    src/checkout/receipt.ts='import { formatPrice } from "../money";\nimport { orderTotal, subtotal, tax, type CartLine } from "../cart/pricing";\n\nexport function receiptLines(lines: CartLine[]): string[] {\n  return lines.map((l) => {\n    const amount = formatPrice(l.qty * l.price);\n    return `${l.name} × ${l.qty}  ${amount}`;\n  });\n}\n\nexport function renderReceipt(lines: CartLine[]): string {\n  return [\n    ...receiptLines(lines),\n    `Subtotal  ${formatPrice(subtotal(lines))}`,\n    `Tax       ${formatPrice(tax(lines))}`,\n    `Total     ${formatPrice(orderTotal(lines))}`,\n  ].join("\\n");\n}\n' \
    src/checkout/confirmation.ts='import { formatPrice } from "../money";\nimport { orderTotal } from "../cart/pricing";\nimport type { CartLine } from "../cart/pricing";\n\nexport function confirmationSubject(orderId: string, lines: CartLine[]): string {\n  return `Order ${orderId} confirmed — ${formatPrice(orderTotal(lines))}`;\n}\n' \
    docs/checkout-flow.md='# Guest checkout flow\n\nStatus: accepted · Owner: Sam Rivera\n\nA shopper can pay without an account. The cart is priced in the browser\n(`src/cart/pricing.ts`); the API re-prices it before charging.\n\n```mermaid\nflowchart LR\n  Cart[Cart] --> Order[Create<br/>order]\n  Order --> Price[Re-price<br/>on server]\n  Price --> Charge[Charge<br/>with key]\n  Charge --> Receipt[Receipt]\n```\n\n## Decisions\n\n- Prices are integers in cents everywhere; `formatPrice` is the only formatter.\n- Charges carry an idempotency key, so a retry never double-charges.\n- Tax is computed from the server total; the browser total is a preview.\n'
git -C "$WEB" push -q origin develop 2>/dev/null

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
acme_register
CW=$(rade worktree add shop-web receipt --project shop --json | jq -r .path)
mkdir -p "$CW/node_modules" && ln -s "$TSMOD" "$CW/node_modules/typescript"
cd "$CW"
ag=$(rade spawn --source shop-web --worktree receipt --agent claude --name receipt --tab new "Add a receipt to the checkout" --json | id)
eventually 40 is "pane_status $ag" done || true
sleep 1
# Warm the language server (the first answer takes a few seconds).
rade code callers src/money.ts:7:17 --wait 90 --json </dev/null >/dev/null 2>&1 || true
rade code hover src/money.ts:7:17 --wait 30 --json </dev/null >/dev/null 2>&1 || true

# ---- hover / peek on receipt.ts ------------------------------------------------
rade focus "$ag" >/dev/null
V=$(rade open src/checkout/receipt.ts:1)
sleep 2
rade debug viewer "$V" place 6:22 >/dev/null     # formatPrice in receiptLines
vkey "$V" ctrl+i
eventually 15 is "pop .open" true || true
sleep 0.8
notoasts
still hover
rpc debug.code '{"op":"popover_close"}' >/dev/null

rade debug viewer "$V" place 16:32 >/dev/null    # orderTotal in renderReceipt
vkey "$V" alt+F12
eventually 15 is "pop .open" true || true
sleep 0.8
notoasts
still peek
rpc debug.code '{"op":"popover_close"}' >/dev/null

# ---- callers: the call hierarchy tree of formatPrice -------------------------------
rade focus "$ag" >/dev/null
V2=$(rade open src/cart/pricing.ts:16)
sleep 1.5
rade debug viewer "$V2" place 16:20 >/dev/null
vkey "$V2" alt+shift+h
eventually 30 bash -c "[[ \$(code_ui | jq -r '.title') == Callers* && \$(code_ui | jq '.rows | length') -gt 2 ]]" || true
sleep 1
code_ui '{"op":"activate","name":"orderTotal","depth":1}' >/dev/null 2>&1 || true
notoasts
still callers
rpc debug.code '{"op":"popover_close"}' >/dev/null

# ---- outline of pricing.ts -----------------------------------------------------------
rade focus "$ag" >/dev/null
V3=$(rade open src/cart/pricing.ts:1)
sleep 1.5
vkey "$V3" alt+shift+o
eventually 30 bash -c "[[ \$(code_ui | jq -r '.title') == Outline* ]]" || true
sleep 1
notoasts
still outline
rpc debug.code '{"op":"popover_close"}' >/dev/null

# ---- markdown with Mermaid -----------------------------------------------------------
rade focus "$ag" >/dev/null
V4=$(rade open docs/checkout-flow.md)
sleep 4
rade focus "$V4" >/dev/null
rade debug action zoom >/dev/null
sleep 2
notoasts
still markdown
rade debug action zoom >/dev/null
sleep 0.5

# ---- search and files in the Inspector ---------------------------------------------
rade close "$V4" --force >/dev/null 2>&1
rade focus "$ag" >/dev/null
rade open src/checkout/receipt.ts:6 >/dev/null
sleep 1.5
rade debug action search_worktree >/dev/null
sleep 0.5
find_ '{"op":"set","query":"formatPrice"}' >/dev/null
eventually 10 bash -c "[[ \$(find_ | jq -r '.running') == false && \$(find_ | jq '.rows | length') -gt 2 ]]" || true
sleep 1
notoasts
still search

# Files: uncommitted work shows in the tree's colors.
printf '\n// TODO: gift wrap line\n' >>"$CW/src/checkout/receipt.ts"
printf 'export const sender = "orders@acme.dev";\n' >"$CW/src/checkout/email.ts"
echo "// guest checkout" >>"$CW/src/App.tsx"
ui right_page files >/dev/null
sleep 1
for d in src src/cart src/checkout docs; do rade debug tree expand "$CW/$d" >/dev/null 2>&1; done
sleep 1.5
notoasts
still files
