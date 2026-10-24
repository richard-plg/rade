# Browser and Annotations

A **Browser Pane** shows your dev server right next to the Agent working on it. Agents drive it
from the CLI to check their own changes; you **annotate** what's wrong — click an element or drag
over a region, write a note — and send it all to the Agent as one prompt.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/browser-light.png">
  <img alt="An Agent on the left, a Browser Pane with the storefront on the right" src="../images/browser-dark.png">
</picture>

## Open a page

`Ctrl+Shift+O` opens a Browser Pane beside the focused Pane on the first port a process in this
Worktree listens on (your dev server), or a blank page with the URL bar focused. The URL bar is
localhost-first: `5173` or `:5173/cart` → `http://localhost:5173/…`.

Listening ports also show in the Statusline, as palette rows *Open localhost:5173*, and in
`rade browser ports`.

In the page: `Ctrl+L` URL bar, `F5` reload, `Alt+Left` / `Alt+Right` history. The header has the
Web Inspector. Cookies and local storage persist per RADE install, so logins to your local apps
survive a restart.

## Annotate

![Annotating a product card and a region, then sending both to the Agent](../images/annotate.gif)
<sub>[Watch as MP4](../images/annotate.mp4)</sub>

1. Turn on annotate mode with the header's pencil toggle or `Ctrl+Shift+A` in the Browser.
2. **Click** an element (it's outlined with its role and name), or **drag** over a region.
3. Type the note; `Enter` adds it.
4. **Send to Agent (n)** under the page.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="../images/annotate-light.png">
  <img alt="Annotate mode: two numbered picks on the page and the Annotations panel with their crops and notes" src="../images/annotate-dark.png">
</picture>

For each annotation RADE sends the page URL, a stable CSS selector (test ids and stable ids
first), the element's ref, its text and box, a cropped screenshot and the page's recent console
errors — everything the Agent needs to find it in the code, without you describing it. It goes to
the Agent in the Browser's Tab (else the Worktree's only Agent; several → you choose). Pending
annotations survive reloads and restarts. Picking works from the keyboard too.

## For Agents

```sh
rade browser open 5173                 # beside the calling Agent; waits for the load
rade browser snapshot                  # the page as text; actionable elements tagged [ref=e3]
rade browser click e3
rade browser fill e2 "sam@acme.dev"
rade browser press Enter
rade browser wait --text "Saved"
rade browser console                   # new console messages and JS errors
rade browser eval "document.title"     # JSON result
rade browser screenshot /tmp/cart.png
rade browser annotations               # what the owner annotated (and the prompt)
```

The automation runs in an isolated script world: the page can't see it, fake a snapshot or
hijack a click. Only processes of your own user can reach it (the RADE socket).

## Chromium engine

Some sites don't run in WebKit, and some tests need trusted input. Add `--engine chromium` to any
`rade browser` command to run the same commands, refs and annotations on a real Chromium that
RADE starts outside its window (headless, or `--headed` with an annotation toolbar):

```sh
rade browser --engine chromium open https://localhost:5173
rade browser --engine chromium --headed open 5173
```

RADE uses `[browser] chromium` from `config.toml`, else Playwright's Chromium
(`npx playwright install chromium`), else `chromium` / `google-chrome` on `PATH`. It talks to it
over a pipe (no debugging port), with a temporary profile deleted on exit.

## Related

- [Showcase: fix a UI bug with annotations](../showcases/fix-ui-with-annotations.md)
