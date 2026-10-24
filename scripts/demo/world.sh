# The demo world (sourced after lib.sh): Workspace `acme` with real git
# repositories and history — shop-web (storefront), shop-api (orders,
# payments), shared-ui (components) and billing — on bare remotes under
# $T/remotes; Projects Shop (shop-web, shop-api, shared-ui at develop) and
# Billing (billing at main, shared-ui at release/2.4). All names are fake.
A=$HOME/workspaces/acme
WEB=$A/shop-web API=$A/shop-api UI=$A/shared-ui BILL=$A/billing
REM=$T/remotes
mkdir -p "$REM"

# commit <repo> <message> <file>=<content>…  (content may contain \n)
commit() {
    local r=$1 m=$2; shift 2
    local kv
    for kv in "$@"; do mkdir -p "$(dirname "$r/${kv%%=*}")"; printf '%b' "${kv#*=}" >"$r/${kv%%=*}"; done
    git -C "$r" add -A && git -C "$r" commit -qm "$m"
}
repo() { # dir → new repo with a bare origin
    mkdir -p "$1" && git -C "$1" init -q
    git init -q --bare "$REM/${1##*/}.git"
    git -C "$1" remote add origin "$REM/${1##*/}.git"
}

repo "$WEB"
commit "$WEB" "chore: scaffold storefront" \
    package.json='{\n  "name": "shop-web",\n  "private": true,\n  "scripts": { "dev": "vite", "test": "vitest run" },\n  "dependencies": { "react": "^19.1.0", "@acme/shared-ui": "workspace:*" }\n}\n' \
    README.md='# shop-web\n\nThe Acme storefront: catalog, cart and checkout.\n\n```sh\nnpm run dev   # http://localhost:5173\n```\n' \
    src/main.tsx='import { createRoot } from "react-dom/client";\nimport { App } from "./App";\n\ncreateRoot(document.getElementById("root")!).render(<App />);\n' \
    src/App.tsx='import { Catalog } from "./catalog/Catalog";\nimport { Cart } from "./cart/Cart";\n\nexport function App() {\n  return (\n    <main>\n      <Catalog />\n      <Cart />\n    </main>\n  );\n}\n'
commit "$WEB" "feat(cart): cart with line items and totals" \
    src/cart/Cart.tsx='import { useCart } from "./useCart";\nimport { formatPrice } from "../money";\n\nexport function Cart() {\n  const { items, total } = useCart();\n  return (\n    <aside className="cart">\n      {items.map((i) => (\n        <div key={i.sku}>{i.name} × {i.qty}</div>\n      ))}\n      <strong>{formatPrice(total)}</strong>\n    </aside>\n  );\n}\n' \
    src/cart/useCart.ts='export type Item = { sku: string; name: string; qty: number; price: number };\n\nexport function useCart() {\n  const items: Item[] = [];\n  const total = items.reduce((sum, i) => sum + i.qty * i.price, 0);\n  return { items, total };\n}\n' \
    src/money.ts='export function formatPrice(cents: number): string {\n  return (cents / 100).toLocaleString("en-US", { style: "currency", currency: "USD" });\n}\n'
commit "$WEB" "feat(catalog): product grid" \
    src/catalog/Catalog.tsx='import { ProductCard } from "./ProductCard";\nimport { products } from "./products";\n\nexport function Catalog() {\n  return <section className="grid">{products.map((p) => <ProductCard key={p.sku} product={p} />)}</section>;\n}\n' \
    src/catalog/ProductCard.tsx='import { Button } from "@acme/shared-ui";\nimport { formatPrice } from "../money";\n\nexport function ProductCard({ product }: { product: { sku: string; name: string; price: number } }) {\n  return (\n    <article>\n      <h3>{product.name}</h3>\n      <p>{formatPrice(product.price)}</p>\n      <Button>Add to cart</Button>\n    </article>\n  );\n}\n' \
    src/catalog/products.ts='export const products = [\n  { sku: "mug-01", name: "Enamel mug", price: 1800 },\n  { sku: "tee-02", name: "Logo tee", price: 2600 },\n  { sku: "cap-03", name: "Field cap", price: 2200 },\n];\n'
git -C "$WEB" checkout -qb develop
commit "$WEB" "test(cart): totals" \
    src/cart/useCart.test.ts='import { describe, it, expect } from "vitest";\nimport { formatPrice } from "../money";\n\ndescribe("formatPrice", () => {\n  it("formats cents", () => expect(formatPrice(1800)).toBe("$18.00"));\n});\n'
git -C "$WEB" push -qu origin main develop 2>/dev/null

repo "$API"
commit "$API" "chore: scaffold orders service" \
    package.json='{\n  "name": "shop-api",\n  "private": true,\n  "scripts": { "dev": "tsx watch src/server.ts", "test": "vitest run" }\n}\n' \
    README.md='# shop-api\n\nOrders, payments and inventory for the storefront.\n' \
    src/server.ts='import { createServer } from "node:http";\nimport { routes } from "./routes";\n\ncreateServer(routes).listen(8080);\n' \
    src/routes.ts='import { orders } from "./orders/handler";\n\nexport function routes(req, res) {\n  if (req.url?.startsWith("/orders")) return orders(req, res);\n  res.statusCode = 404;\n  res.end();\n}\n'
commit "$API" "feat(orders): create and list orders" \
    src/orders/handler.ts='import { createOrder, listOrders } from "./service";\n\nexport async function orders(req, res) {\n  if (req.method === "POST") return res.end(JSON.stringify(await createOrder(req)));\n  res.end(JSON.stringify(await listOrders()));\n}\n' \
    src/orders/service.ts='export type Order = { id: string; total: number; status: "open" | "paid" };\n\nconst db: Order[] = [];\n\nexport async function createOrder(_req): Promise<Order> {\n  const order: Order = { id: crypto.randomUUID(), total: 0, status: "open" };\n  db.push(order);\n  return order;\n}\n\nexport async function listOrders(): Promise<Order[]> {\n  return db;\n}\n' \
    src/payments/charge.ts='export async function charge(orderId: string, cents: number) {\n  // TODO: retries\n  return { orderId, cents, ok: true };\n}\n'
git -C "$API" checkout -qb develop
commit "$API" "feat(payments): idempotency keys" \
    src/payments/charge.ts='export async function charge(orderId: string, cents: number, key = orderId) {\n  return { orderId, cents, key, ok: true };\n}\n'
git -C "$API" push -qu origin main develop 2>/dev/null

repo "$UI"
commit "$UI" "feat: Button and tokens" \
    package.json='{\n  "name": "@acme/shared-ui",\n  "version": "2.4.0"\n}\n' \
    README.md='# shared-ui\n\nAcme design tokens and components.\n' \
    src/tokens.ts='export const color = {\n  brand: "#0f766e",\n  ink: "#0b1215",\n  surface: "#ffffff",\n};\n\nexport const radius = { sm: 4, md: 8 };\n' \
    src/Button.tsx='import { color, radius } from "./tokens";\n\nexport function Button(props: { children: React.ReactNode; onClick?: () => void }) {\n  return <button style={{ background: color.brand, borderRadius: radius.md }} {...props} />;\n}\n' \
    src/index.ts='export * from "./Button";\nexport * from "./tokens";\n'
git -C "$UI" branch release/2.4
git -C "$UI" checkout -qb develop
commit "$UI" "feat: dark tokens" \
    src/tokens.ts='export const color = {\n  brand: "#0f766e",\n  ink: "#0b1215",\n  surface: "#ffffff",\n};\n\nexport const dark = { surface: "#0b1215", ink: "#e6edf3" };\n\nexport const radius = { sm: 4, md: 8 };\n'
git -C "$UI" push -qu origin main develop release/2.4 2>/dev/null

repo "$BILL"
commit "$BILL" "chore: scaffold invoices" \
    README.md='# billing\n\nInvoices and receipts.\n' \
    src/invoice.ts='export type Invoice = { id: string; lines: { label: string; cents: number }[] };\n\nexport const total = (i: Invoice) => i.lines.reduce((s, l) => s + l.cents, 0);\n'
git -C "$BILL" push -qu origin main 2>/dev/null

mkdir -p "$A/.rade"
printf 'name = "acme"\n[identity]\ngit_email = "sam@acme.dev"\n' >"$A/.rade/workspace.toml"

# acme_register: the Workspace, its Sources and the two Projects in the app.
acme_register() {
    rade workspace add "$A" >/dev/null
    local r
    for r in "$WEB" "$API" "$UI" "$BILL"; do rade source add "$r" --workspace acme >/dev/null; done
    rade workspace switch acme >/dev/null
    rade project add Shop --source shop-web:develop --source shop-api:develop --source shared-ui:develop >/dev/null
    rade project add Billing --source billing:main --source shared-ui:release/2.4 >/dev/null
}

# more_world / more_register (scenes with three Workspaces): `globex`, a client
# (Projects Portal: portal-web + portal-api, and Mobile), and `personal`
# (Projects Blog and Dotfiles), each with its own git email.
G=$HOME/workspaces/globex P=$HOME/workspaces/personal
more_world() {
    repo "$G/portal-web"
    commit "$G/portal-web" "chore: scaffold the customer portal" \
        README.md='# portal-web\n\nGlobex customer portal.\n' \
        src/index.ts='export { App } from "./App";\n' \
        src/App.tsx='export function App() {\n  return <main>Portal</main>;\n}\n' \
        src/auth/session.ts='export const session = { user: null as string | null };\n'
    repo "$G/portal-api"
    commit "$G/portal-api" "chore: scaffold the portal API" \
        README.md='# portal-api\n\nThe API behind the Globex portal: invoices, exports, tenants.\n\n```mermaid\nflowchart TD\n  web[portal-web] --> api[portal-api]\n  api --> tenant{requireTenant}\n  tenant --> invoices[(invoices)]\n  tenant --> exports[(exports)]\n```\n\n## Run\n\n```sh\nnpm run dev   # http://localhost:8080\n```\n' \
        package.json='{\n  "name": "portal-api",\n  "private": true\n}\n' \
        src/index.ts='import { router } from "./router";\n\nrouter.listen(8080);\n' \
        src/routes/invoices.ts='import { router, requireTenant } from "../router";\n\nrouter.get("/invoices", requireTenant, listInvoices);\nrouter.get("/invoices/:id/pdf", invoicePdf);\n' \
        src/routes/exports.ts='import { router } from "../router";\n\nrouter.get("/exports/:id", getExport);\n' \
        src/router.ts='export const router = createRouter();\n\nexport function requireTenant(req, _res, next) {\n  if (!req.tenant) throw new Error("no tenant");\n  next();\n}\n'
    repo "$G/mobile"
    commit "$G/mobile" "chore: scaffold the app" README.md='# mobile\n\nGlobex field app.\n' src/index.ts='export const app = "mobile";\n'
    repo "$P/blog"
    commit "$P/blog" "chore: scaffold the blog" README.md='# blog\n\nNotes on Rust, GTK and tooling.\n' \
        src/index.ts='export const feed = { title: "Sam Rivera", posts: [] as string[] };\n' \
        posts/2026-10-gtk-tips.md='# GTK tips\n\nSmall things that save time.\n'
    repo "$P/dotfiles"
    commit "$P/dotfiles" "chore: shell and editor config" README.md='# dotfiles\n' src/index.ts='// nvim config helpers\n'
    local r
    for r in "$G"/*/ "$P"/*/; do git -C "$r" push -qu origin main 2>/dev/null; done
    for r in "$G"/*/; do git -C "$r" config user.email sam@globex.example; done
    for r in "$P"/*/; do git -C "$r" config user.email sam.rivera@example.org; done
    mkdir -p "$G/.rade" "$P/.rade"
    printf 'name = "globex"\ncolor = "orange"\n[identity]\ngit_email = "sam@globex.example"\n' >"$G/.rade/workspace.toml"
    printf 'name = "personal"\ncolor = "purple"\n[identity]\ngit_email = "sam.rivera@example.org"\n' >"$P/.rade/workspace.toml"
}
more_register() {
    local r
    rade workspace add "$G" >/dev/null
    for r in "$G"/*/; do rade source add "${r%/}" --workspace globex >/dev/null; done
    rade workspace add "$P" >/dev/null
    for r in "$P"/*/; do rade source add "${r%/}" --workspace personal >/dev/null; done
    rade workspace switch globex >/dev/null
    rade project add Portal --source portal-web:main --source portal-api:main >/dev/null
    rade project add Mobile --source mobile:main >/dev/null
    rade workspace switch personal >/dev/null
    rade project add Blog --source blog:main >/dev/null
    rade project add Dotfiles --source dotfiles:main >/dev/null
}
