#!/usr/bin/env python3
"""A deterministic fake language server for the M28 suites (R83-R86).

Answers come from <root>/.fake-lsp.json, keyed by the name at the asked
position (read from the didOpen text):
  {"greet": {"def": ["src/lib.rs:1:8"], "refs": [...], "impl": [...],
             "callers": [{"name": "main", "at": "src/main.rs:2:5"}],
             "callees": [{"name": "helper", "at": "src/lib.rs:5:4"}]},
   "@symbols": {"src/lib.rs": [{"name": "greet", "at": "src/lib.rs:1:8"}]}}
Positions are 1-based lines and character columns; the server picks
utf-8 when offered, so columns are sent as byte offsets.

Environment:
  FAKE_LSP_LOG       append `start|request|config|shutdown|exit` lines
  FAKE_LSP_INDEX_MS  report indexing ($/progress) this long after start;
                     queries answer [] meanwhile
  FAKE_LSP_NO_CALLS  no callHierarchyProvider
  FAKE_LSP_HANG_ON   a method name (e.g. `initialize`,
                     `textDocument/documentSymbol`): never answer it, and
                     nothing after it (a hung server, M29)
A caller's "at" may be a list (each call site = one of its fromRanges,
"def" its name, else the first site); the next level is the caller's own
"callers" entry (by name); a callee's "sites" are its fromRanges (0.7.1).
"hover" per name is its hover Markdown (M30); "@workspace" is a list of
{"name", "at", "kind"?} answered to `workspace/symbol` when the name
contains the query (any case); FAKE_LSP_NO_HOVER drops hoverProvider.
"@symbols" entries may have "end" (the range's last position) and "kind"
(an LSP SymbolKind, default 12); `workspace/didChangeWatchedFiles` is
logged as `watched <type>:<path relative to the root> ...` (M29).
"@index_ms" in .fake-lsp.json sets the indexing time per repository,
"@hang_on" the method to hang on (as FAKE_LSP_HANG_ON).
A query on the name `crash_now` exits at once (code 3); on `garbage_now` it
sends a 99999999999999-byte Content-Length and hangs.
"""
import json, os, sys, threading, time

out = sys.stdout.buffer
lock = threading.Lock()
LOG = os.environ.get("FAKE_LSP_LOG")
ROOT = "."
DOCS = {}
ENC = "utf-16"
indexing = False


def log(*a):
    if LOG:
        with open(LOG, "a") as f:
            f.write(" ".join(str(x) for x in a) + "\n")


def send(msg):
    body = json.dumps(msg).encode()
    with lock:
        out.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        out.flush()


def read():
    n = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            if n is not None:
                break
            continue
        k, _, v = line.partition(b":")
        if k.lower() == b"content-length":
            n = int(v)
    return json.loads(sys.stdin.buffer.read(n))


def uri_path(u):
    from urllib.parse import unquote
    return unquote(u[len("file://"):])


def line_text(path, line0):
    try:
        with open(path, encoding="utf-8") as f:
            lines = f.read().split("\n")
        return lines[line0] if line0 < len(lines) else ""
    except OSError:
        return ""


def to_units(text, chars):
    s = text[:chars]
    return len(s.encode("utf-8")) if ENC == "utf-8" else len(s.encode("utf-16-le")) // 2


def from_units(text, units):
    n = 0
    for i, c in enumerate(text):
        if n >= units:
            return i
        n += len(c.encode("utf-8")) if ENC == "utf-8" else len(c.encode("utf-16-le")) // 2
    return len(text)


def loc(spec):
    path, line, col = spec.rsplit(":", 2)
    full = os.path.join(ROOT, path)
    l0 = int(line) - 1
    c = to_units(line_text(full, l0), int(col) - 1)
    pos = {"line": l0, "character": c}
    return "file://" + full, {"start": pos, "end": pos}


def item(name, spec):
    uri, rng = loc(spec)
    return {"name": name, "kind": 12, "uri": uri, "range": rng, "selectionRange": rng}


def answers():
    try:
        with open(os.path.join(ROOT, ".fake-lsp.json")) as f:
            return json.load(f)
    except OSError:
        return {}


def word_at(params):
    uri = params["textDocument"]["uri"]
    text = DOCS.get(uri)
    if text is None:
        text = open(uri_path(uri), encoding="utf-8").read()
    line = text.split("\n")[params["position"]["line"]]
    i = from_units(line, params["position"]["character"])
    isw = lambda c: c.isalnum() or c in "_$"
    if i >= len(line) or not isw(line[i]):
        i -= 1
    if i < 0 or not isw(line[i]):
        return ""
    s = e = i
    while s > 0 and isw(line[s - 1]):
        s -= 1
    while e < len(line) and isw(line[e]):
        e += 1
    return line[s:e]


def finish_indexing(ms):
    global indexing
    time.sleep(ms / 1000)
    indexing = False
    send({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "idx", "value": {"kind": "end"}}})


def query(method, params):
    if method in ("callHierarchy/incomingCalls", "callHierarchy/outgoingCalls"):
        word = params["item"]["name"]
    elif "position" in params:
        word = word_at(params)
    else:
        word = ""
    log("request", method, word)
    if word == "garbage_now":
        # A broken server: a huge length, then it stays alive (acceptance H1, M3).
        with lock:
            out.write(b"Content-Length: 99999999999999\r\n\r\n{")
            out.flush()
        time.sleep(3600)
    if word == "crash_now":
        sys.stderr.write("fake-lsp: crashed on purpose\n")
        sys.stderr.flush()
        os._exit(3)
    if indexing:
        return []
    a = answers()
    w = a.get(word, {})
    if method == "textDocument/definition":
        return [dict(zip(("uri", "range"), loc(s))) for s in w.get("def", [])]
    if method == "textDocument/implementation":
        return [dict(zip(("uri", "range"), loc(s))) for s in w.get("impl", [])]
    if method == "textDocument/references":
        return [dict(zip(("uri", "range"), loc(s))) for s in w.get("refs", [])]
    if method == "textDocument/prepareCallHierarchy":
        return [item(word, w["def"][0])] if w.get("def") else None
    if method == "callHierarchy/incomingCalls":
        sites = lambda c: c["at"] if isinstance(c["at"], list) else [c["at"]]
        return [{"from": item(c["name"], c.get("def", sites(c)[0])), "fromRanges": [loc(s)[1] for s in sites(c)]}
                for c in w.get("callers", [])]
    if method == "callHierarchy/outgoingCalls":
        return [{"to": item(c["name"], c["at"]), "fromRanges": [loc(s)[1] for s in c.get("sites", [])]}
                for c in w.get("callees", [])]
    if method == "textDocument/hover":
        h = w.get("hover")
        return {"contents": {"kind": "markdown", "value": h}} if h else None
    if method == "workspace/symbol":
        q = params.get("query", "").lower()
        res = []
        for s in a.get("@workspace", []):
            if q in s["name"].lower():
                uri, rng = loc(s["at"])
                res.append({"name": s["name"], "kind": s.get("kind", 12), "location": {"uri": uri, "range": rng}})
        return res
    if method == "textDocument/documentSymbol":
        rel = os.path.relpath(uri_path(params["textDocument"]["uri"]), ROOT)
        def sym(s):
            _, sel = loc(s["at"])
            rng = {"start": sel["start"], "end": loc(s["end"])[1]["end"]} if s.get("end") else sel
            return {"name": s["name"], "kind": s.get("kind", 12), "range": rng, "selectionRange": sel,
                    "children": [sym(c) for c in s.get("children", [])]}
        return [sym(s) for s in a.get("@symbols", {}).get(rel, [])]
    return None


def main():
    global ROOT, ENC, indexing
    while True:
        msg = read()
        if msg is None:
            log("eof", os.getpid())
            return
        m, mid, p = msg.get("method"), msg.get("id"), msg.get("params")
        if m and m in (os.environ.get("FAKE_LSP_HANG_ON"), answers().get("@hang_on")):
            log("hang", m)
            time.sleep(3600)
        if m is None:
            if mid == 900:
                log("config", json.dumps(msg.get("result")))
            continue
        if m == "initialize":
            ROOT = uri_path(p["rootUri"])
            if "utf-8" in p["capabilities"].get("general", {}).get("positionEncodings", []):
                ENC = "utf-8"
            log("start", os.getpid(), ROOT, ENC, json.dumps(p.get("initializationOptions")))
            caps = {"positionEncoding": ENC, "definitionProvider": True, "referencesProvider": True,
                    "implementationProvider": True, "documentSymbolProvider": True,
                    "callHierarchyProvider": not os.environ.get("FAKE_LSP_NO_CALLS"),
                    "hoverProvider": not os.environ.get("FAKE_LSP_NO_HOVER"),
                    "workspaceSymbolProvider": True}
            send({"jsonrpc": "2.0", "id": mid, "result": {"capabilities": caps, "serverInfo": {"name": "fake-lsp"}}})
        elif m == "initialized":
            send({"jsonrpc": "2.0", "id": 900, "method": "workspace/configuration",
                  "params": {"items": [{"section": "rust-analyzer"}]}})
            ms = int(answers().get("@index_ms") or os.environ.get("FAKE_LSP_INDEX_MS", "0") or 0)
            if ms:
                indexing = True
                send({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "idx", "value": {"kind": "begin", "title": "Indexing"}}})
                threading.Thread(target=finish_indexing, args=(ms,), daemon=True).start()
        elif m == "textDocument/didOpen":
            DOCS[p["textDocument"]["uri"]] = p["textDocument"]["text"]
        elif m == "workspace/didChangeWatchedFiles":
            log("watched", *sorted("%d:%s" % (c["type"], os.path.relpath(uri_path(c["uri"]), ROOT)) for c in p["changes"]))
        elif m == "textDocument/didClose":
            DOCS.pop(p["textDocument"]["uri"], None)
        elif m == "shutdown":
            log("shutdown", os.getpid(), "params" in msg)
            send({"jsonrpc": "2.0", "id": mid, "result": None})
        elif m == "exit":
            log("exit", os.getpid())
            os._exit(0)
        elif mid is not None:
            try:
                send({"jsonrpc": "2.0", "id": mid, "result": query(m, p)})
            except Exception as e:  # noqa: BLE001
                send({"jsonrpc": "2.0", "id": mid, "error": {"code": -32603, "message": str(e)}})


main()
