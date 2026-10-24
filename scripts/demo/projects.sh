#!/usr/bin/env bash
# Demo scene: Projects — Shop's home (KPIs, link table, Worktrees, Activity),
# Base drift (shop-api on a feature branch with uncommitted changes), the
# New Project dialog with its checkout plan, Change base and the command
# palette with a query. Stills: project-home, drift, new-project,
# change-base, palette.
# Usage: NO_BUILD=1 scripts/demo/projects.sh   (KEEP=1 keeps the temp dir)
set -uo pipefail
NAME=projects
source "$(dirname "$0")/lib.sh"
source "$DEMO/world.sh"
T0=$(date +%s)

# ---- More history for the world ---------------------------------------------------
# shop-web's develop is 2 commits ahead on the remote (the main checkout reads ↓2).
git clone -q -b develop "$REM/shop-web.git" "$T/web-clone"
commit "$T/web-clone" "fix(catalog): sort by price" src/catalog/sort.ts='export const byPrice = (a: { price: number }, b: { price: number }) => a.price - b.price;\n'
commit "$T/web-clone" "chore: bump react to 19.1.1" package.json='{\n  "name": "shop-web",\n  "private": true,\n  "scripts": { "dev": "vite", "test": "vitest run" },\n  "dependencies": { "react": "^19.1.1", "@acme/shared-ui": "workspace:*" }\n}\n'
git -C "$T/web-clone" push -q origin develop
git -C "$WEB" fetch -q origin
# shop-api: a feature branch 3 commits ahead of develop.
git -C "$API" checkout -qb feat/checkout-v2
commit "$API" "feat(orders): order totals with shipping" src/orders/totals.ts='export const withShipping = (cents: number) => cents + (cents > 5000 ? 0 : 499);\n'
commit "$API" "feat(orders): guest checkout endpoint" src/orders/guest.ts='import { createOrder } from "./service";\n\nexport const guestCheckout = (req) => createOrder(req);\n'
commit "$API" "feat(payments): confirm charge" src/payments/confirm.ts='export const confirm = (orderId: string) => ({ orderId, ok: true });\n'
git -C "$API" checkout -q develop

start_app
rpc debug.ui '{"op":"layout","width":1440,"height":900}' >/dev/null
acme_register
CV=$(rade worktree add shop-web checkout-v2 --project shop --json | jq -r .path)
PI=$(rade worktree add shop-web perf-img --project shop --json | jq -r .path)
TK=$(rade worktree add shared-ui tokens --project billing --json | jq -r .path)
# checkout-v2: 3 commits ahead of develop and uncommitted work.
commit "$CV" "feat(checkout): guest form" src/checkout/CheckoutForm.tsx='export function CheckoutForm() {\n  return <form><input name="email" /></form>;\n}\n'
commit "$CV" "feat(checkout): order summary" src/checkout/Summary.tsx='export function Summary({ total }: { total: number }) {\n  return <strong>{total}</strong>;\n}\n'
commit "$CV" "test(checkout): empty cart" src/checkout/Summary.test.ts='import { it } from "vitest";\n\nit("empty cart", () => {});\n'
commit "$TK" "feat: focus ring tokens" src/focus.ts='export const focus = { ring: "#5eead4", width: 2 };\n'
printf 'export const shipping = 499;\n' >"$CV/src/cart/shipping.ts"
echo "// guest checkout" >>"$CV/src/App.tsx"

dev=$(rade spawn --source shop-web --name dev-server --tab new --cmd "$DEMO/devserver.sh 5173" --json | id)
planner=$(rade spawn --source shop-web --worktree checkout-v2 --agent claude --name planner --tab new "Plan the guest checkout flow" --json | id)
tests=$(rade spawn --source shop-web --worktree checkout-v2 --agent codex --name tests --tab current --split right "Write tests for the cart totals" --json | id)
perf=$(rade spawn --source shop-web --worktree perf-img --agent claude --name perf --tab new "Lazy-load the product images" --json | id)
ui_=$(rade spawn --source shared-ui --worktree tokens --agent claude --name ui --tab new "Add focus ring tokens" --json | id)
eventually 40 is "pane_status $tests" waiting
eventually 40 is "pane_status $planner" done
eventually 20 is "pane_status $perf" running
rade focus "$tests" >/dev/null

# Scene: shop-api drifts to its feature branch with uncommitted changes.
git -C "$API" switch -q feat/checkout-v2
printf '\nexport const SHIPPING_FLAT = 499;\n' >>"$API/src/orders/service.ts"
printf '\n// TODO: guest route\n' >>"$API/src/routes.ts"
printf 'export const quote = (cents: number) => cents + 499;\n' >"$API/src/orders/quote.ts"
eventually 8 is "ui sidebar | jq -r '[.sidebar[] | select(.kind==\"link\" and .title==\"shop-api\") | .drift] | first'" true
sleep 2

echo "== Project home"
rpc debug.ui '{"op":"nav_click","kind":"project","title":"Shop"}' >/dev/null
eventually 5 is "ui chrome | jq -r .chrome.breadcrumb" "Shop / Home"
sleep 2
notoasts
still project-home

echo "== Base drift: Navigator marker + Changes drift card"
ui home 'worktree checkout-v2' >/dev/null
rade debug action show_changes >/dev/null
ui right_page changes >/dev/null
rade focus "$tests" >/dev/null
eventually 8 bash -c "[[ \$(ui changes | jq -r '.changes.groups[1].drift != null') == true ]]"
sleep 2
notoasts
still drift

echo "== palette"
rade focus "$tests" >/dev/null
rpc debug.ui '{"op":"key","keys":"ctrl+shift+p"}' >/dev/null
eventually 3 bash -c "[[ \$(rade --json debug palette --ui info | jq -r .open) == true ]]"
rade debug palette --ui type shared >/dev/null
eventually 5 bash -c "[[ \$(rade --json debug palette --ui info | jq -r .busy) == false ]]"
sleep 0.5
still palette
rpc debug.ui '{"op":"key","keys":"escape"}' >/dev/null

echo "== New Project dialog + its checkout plan"
dlgop() { rpc debug.ui "$1" | jq -c .dialog; }
dlgop '{"op":"dialog_open","kind":"new_project"}' >/dev/null
dlgop '{"op":"dialog_name","text":"Checkout revamp"}' >/dev/null
dlgop '{"op":"dialog_add","source":"shop-web","base":"develop"}' >/dev/null
dlgop '{"op":"dialog_add","source":"shop-api","base":"develop"}' >/dev/null
dlgop '{"op":"dialog_add","source":"shared-ui","base":"release/2.4"}' >/dev/null
eventually 5 bash -c "[[ \$(rpc debug.ui '{\"op\":\"dialog\"}' | jq -r '[.dialog.rows[].plan] | map(select(. == \"\")) | length') == 0 ]]"
sleep 1
notoasts
still new-project
rpc debug.ui '{"op":"key","keys":"escape"}' >/dev/null
sleep 1

echo "== Change base"
dlgop '{"op":"dialog_open","kind":"change_base","project":"shop","source":"shared-ui"}' >/dev/null
eventually 5 bash -c "[[ -n \$(rpc debug.ui '{\"op\":\"dialog\"}' | jq -r '.dialog.rows[0].plan') ]]"
dlgop '{"op":"dialog_row","source":"shared-ui","base":"release/2.4"}' >/dev/null
sleep 1.5
notoasts
still change-base
rpc debug.ui '{"op":"key","keys":"escape"}' >/dev/null
echo "total $(( $(date +%s) - T0 )) s"
