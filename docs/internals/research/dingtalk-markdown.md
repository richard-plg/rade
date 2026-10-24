# DingTalk robot Markdown (for R45 export)

Checked 2026-10-02 against DingTalk's English developer docs, "Robot message type":
<https://help.dingtalk.io/open/development/robot-message-type>. Not yet checked by a real paste
into a DingTalk group (R45-c, pending owner) — treat the rendering notes as **UNVERIFIED** until
`docs/internals/checks/M18.md` records that paste.

## Payload

```json
{ "msgtype": "markdown", "markdown": { "title": "…", "text": "…" }, "at": { … } }
```

`title` is what the conversation list and the notification show; `text` is the Markdown.
Custom robots need a security setting (a keyword the text must contain, a signature, or an IP
allow-list). `rade report export --format dingtalk --json` prints this payload without `at`.

## Documented syntax

| Element | Syntax |
|---|---|
| Headings | `#` … `######` |
| Quote | `> text` |
| Bold / italic | `**bold**`, `*italic*` |
| Link | `[text](url)` |
| Image | `![](url)` |
| Lists | `- item`, `1. item` |
| Line break | "add 2 spaces before and after `\n`" |

**Not documented** (so RADE converts them): tables, code (inline and fenced), strikethrough,
nested lists, task lists, horizontal rules. No length limit is documented.

## What `report_export.rs` does for DingTalk

- Keeps headings, bold, italic, links, images, quotes and flat `- ` / `1. ` lists.
- Tables → a bold header line, then one `- a · b · c` line per row.
- Code blocks → one `> ` line per code line; inline code → its text.
- Strikethrough → plain text; nested lists → flattened to one level; task items → `☐` / `☑`.
- Blocks are separated by a blank line; a line break inside a paragraph becomes `  \n  `.
