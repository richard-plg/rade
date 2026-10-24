//! A small synchronous LSP client (R83, ADR-0029): Content-Length framing,
//! request ids with a reader thread per server, the answers a client owes
//! the server (`workspace/configuration`, capability registration,
//! progress tokens), readiness from `$/progress` and rust-analyzer's
//! `experimental/serverStatus`, position encodings (UTF-8 / UTF-16 ↔
//! character columns) and the shutdown sequence. No async runtime: one
//! blocking call per request with a timeout.

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// One message with its `Content-Length` header.
pub fn encode(msg: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(msg).unwrap_or_default();
    let mut out = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    out.extend(body);
    out
}

/// Larger messages are a broken server (it is then treated as exited).
pub const MAX_MESSAGE: usize = 64 << 20;

/// The next message; `Ok(None)` at end of stream. Headers other than
/// `Content-Length` are skipped.
pub fn read_message(r: &mut impl BufRead) -> std::io::Result<Option<Value>> {
    let mut len = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            if len.is_some() {
                break;
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse::<usize>().ok();
            }
        }
    }
    let len = len.unwrap_or(0);
    if len > MAX_MESSAGE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("message of {len} bytes"),
        ));
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// The position encoding the server chose (`positionEncoding`; UTF-16
/// when it says nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enc {
    Utf8,
    Utf16,
}

impl Enc {
    pub fn of(caps: &Value) -> Self {
        match caps["positionEncoding"].as_str() {
            Some("utf-8") => Enc::Utf8,
            _ => Enc::Utf16,
        }
    }
}

/// A character column of `line` → the server's column.
pub fn to_lsp_col(line: &str, chars: usize, enc: Enc) -> u32 {
    line.chars()
        .take(chars)
        .map(|c| match enc {
            Enc::Utf8 => c.len_utf8(),
            Enc::Utf16 => c.len_utf16(),
        })
        .sum::<usize>() as u32
}

/// The server's column of `line` → a character column (clamped).
pub fn from_lsp_col(line: &str, col: u32, enc: Enc) -> usize {
    let mut units = 0usize;
    for (i, c) in line.chars().enumerate() {
        if units >= col as usize {
            return i;
        }
        units += match enc {
            Enc::Utf8 => c.len_utf8(),
            Enc::Utf16 => c.len_utf16(),
        };
    }
    line.chars().count()
}

/// `file:///a%20b/c.rs` for `/a b/c.rs`.
pub fn path_uri(p: &Path) -> String {
    let mut s = String::from("file://");
    for b in p.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            s.push(b as char);
        } else {
            s += &format!("%{b:02X}");
        }
    }
    s
}

pub fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    Some(PathBuf::from(crate::link::percent_decode(rest)))
}

/// `settings` at a dotted `section` (`rust-analyzer`, `a.b`); the whole
/// object for none.
pub fn section(settings: &Value, section: Option<&str>) -> Value {
    let Some(s) = section.filter(|s| !s.is_empty()) else {
        return settings.clone();
    };
    let mut v = settings;
    for k in s.split('.') {
        match v.get(k) {
            Some(x) => v = x,
            None => return Value::Null,
        }
    }
    v.clone()
}

/// What a request failed with.
#[derive(Clone, Debug, PartialEq)]
pub enum LspError {
    Timeout,
    /// The process is gone (its last stderr line, if any).
    Dead(String),
    Server(i64, String),
}

impl std::fmt::Display for LspError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LspError::Timeout => write!(f, "timed out"),
            LspError::Dead(s) if s.is_empty() => write!(f, "server exited"),
            LspError::Dead(s) => write!(f, "server exited: {s}"),
            LspError::Server(c, m) => write!(f, "{m} ({c})"),
        }
    }
}

/// How to start a server.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Launch {
    pub argv: Vec<String>,
    /// The folder it serves (`rootUri`, `workspaceFolders`, cwd).
    pub root: PathBuf,
    /// Added to the app's environment (the Workspace `[env]`).
    pub env: Vec<(String, String)>,
    pub init: Value,
    pub settings: Value,
}

#[derive(Default)]
struct Status {
    /// `$/progress` tokens begun and not ended.
    work: HashSet<String>,
    /// rust-analyzer `experimental/serverStatus.quiescent`.
    quiescent: Option<bool>,
    /// Any progress or status seen.
    reported: bool,
    dead: bool,
    stderr: String,
}

type Pending = Arc<Mutex<HashMap<i64, mpsc::Sender<Result<Value, LspError>>>>>;

/// A running server.
pub struct Client {
    pub pid: i32,
    pub argv: Vec<String>,
    pub root: PathBuf,
    pub enc: Enc,
    pub caps: Value,
    /// `serverInfo.name` from `initialize`.
    pub name: Option<String>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    child: Mutex<Child>,
    pending: Pending,
    next: AtomicI64,
    status: Arc<Mutex<Status>>,
    /// A document was opened before (the first open may start a project load).
    pub warm: std::sync::atomic::AtomicBool,
    /// One open → ask → close at a time (a document opened twice is an error).
    pub doc_lock: Mutex<()>,
}

impl Client {
    /// Spawn (own session, stdin/stdout piped, stderr kept for errors) and
    /// run `initialize` / `initialized`.
    pub fn start(l: &Launch, timeout: Duration) -> Result<Client, String> {
        use std::os::unix::process::CommandExt;
        let (prog, args) = l.argv.split_first().ok_or("empty command")?;
        let mut c = Command::new(prog);
        c.args(args)
            .current_dir(&l.root)
            .envs(l.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: setsid is async-signal-safe.
        unsafe {
            c.pre_exec(|| {
                libc::setsid();
                crate::process::no_inherit();
                Ok(())
            });
        }
        let mut child = c.spawn().map_err(|e| format!("{prog}: {e}"))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take();
        let pending: Pending = Arc::default();
        let status: Arc<Mutex<Status>> = Arc::default();
        let client = Client {
            pid: child.id() as i32,
            argv: l.argv.clone(),
            root: l.root.clone(),
            enc: Enc::Utf16,
            caps: Value::Null,
            name: None,
            stdin: Arc::new(Mutex::new(stdin)),
            child: Mutex::new(child),
            pending: pending.clone(),
            next: AtomicI64::new(1),
            status: status.clone(),
            doc_lock: Mutex::new(()),
            warm: Default::default(),
        };
        if let Some(err) = stderr {
            let st = status.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    let mut s = st.lock().unwrap();
                    s.stderr = line.chars().take(400).collect();
                }
            });
        }
        let reply = Writer(Some(client.stdin.clone()));
        let settings = l.settings.clone();
        let root = l.root.clone();
        std::thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            while let Ok(Some(msg)) = read_message(&mut r) {
                handle(&msg, &pending, &status, &settings, &root, &reply);
            }
            let mut p = pending.lock().unwrap();
            status.lock().unwrap().dead = true;
            let why = status.lock().unwrap().stderr.clone();
            for (_, tx) in p.drain() {
                let _ = tx.send(Err(LspError::Dead(why.clone())));
            }
        });
        let init = json!({
            "processId": std::process::id(),
            "clientInfo": {"name": "rade"},
            "rootUri": path_uri(&l.root),
            "rootPath": l.root,
            "workspaceFolders": [{"uri": path_uri(&l.root), "name": l.root.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()}],
            "initializationOptions": l.init,
            "capabilities": {
                "general": {"positionEncodings": ["utf-8", "utf-16"]},
                "window": {"workDoneProgress": true},
                "workspace": {"configuration": true, "workspaceFolders": true,
                              "didChangeWatchedFiles": {"dynamicRegistration": true}},
                "textDocument": {
                    "synchronization": {"didSave": false},
                    "definition": {"linkSupport": true},
                    "implementation": {"linkSupport": true},
                    "references": {},
                    "callHierarchy": {},
                    "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
                },
                "experimental": {"serverStatusNotification": true},
            },
        });
        let mut client = client;
        let caps = match client.request("initialize", init, timeout) {
            Ok(r) => {
                client.name = r["serverInfo"]["name"].as_str().map(String::from);
                r["capabilities"].clone()
            }
            Err(e) => {
                client.kill();
                return Err(format!("{prog}: initialize {e}"));
            }
        };
        client.enc = Enc::of(&caps);
        client.caps = caps;
        client.notify("initialized", json!({}));
        if !l.settings.is_null() {
            client.notify(
                "workspace/didChangeConfiguration",
                json!({"settings": l.settings}),
            );
        }
        Ok(client)
    }

    fn send(&self, msg: &Value) -> bool {
        let mut g = self.stdin.lock().unwrap();
        match g.as_mut() {
            Some(s) => s.write_all(&encode(msg)).and_then(|_| s.flush()).is_ok(),
            None => false,
        }
    }

    pub fn notify(&self, method: &str, params: Value) {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    /// One request; `params` `null` is left out (TS 7 refuses it).
    pub fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, LspError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        {
            // Under the lock the reader takes when it dies: never missed.
            let mut p = self.pending.lock().unwrap();
            if self.status.lock().unwrap().dead {
                return Err(LspError::Dead(self.status.lock().unwrap().stderr.clone()));
            }
            p.insert(id, tx);
        }
        let mut msg = json!({"jsonrpc": "2.0", "id": id, "method": method});
        if !params.is_null() {
            msg["params"] = params;
        }
        if !self.send(&msg) {
            self.pending.lock().unwrap().remove(&id);
            return Err(LspError::Dead(self.status.lock().unwrap().stderr.clone()));
        }
        match rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                self.notify("$/cancelRequest", json!({"id": id}));
                Err(LspError::Timeout)
            }
        }
    }

    pub fn alive(&self) -> bool {
        !self.status.lock().unwrap().dead
    }

    /// Still loading: work in progress (`$/progress`) or not quiescent.
    pub fn busy(&self) -> bool {
        let s = self.status.lock().unwrap();
        !s.work.is_empty() || s.quiescent == Some(false)
    }

    /// Right after start: wait up to `d` for a first progress or status
    /// report (a server that reports nothing is taken as ready after it).
    pub fn settle(&self, d: Duration) {
        let t = Instant::now();
        while !self.status.lock().unwrap().reported && t.elapsed() < d && self.alive() {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// After the first `didOpen`: wait up to `start` for the server to
    /// begin reporting work, then up to `load` for that work to end
    /// (typescript-language-server loads the project then, and answers
    /// from a half-loaded one meanwhile: acceptance M4).
    pub fn first_open(&self, start: Duration, load: Duration) {
        if self.warm.swap(true, Ordering::Relaxed) {
            return;
        }
        let t = Instant::now();
        while t.elapsed() < start && self.alive() && !self.busy() {
            std::thread::sleep(Duration::from_millis(50));
        }
        let t = Instant::now();
        while t.elapsed() < load && self.alive() && self.busy() {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The document lock, waiting at most `timeout`: requests queued
    /// behind one a hung server never answers each give up after their
    /// own timeout, not one after another (M28 acceptance, Low).
    pub fn lock_doc(&self, timeout: Duration) -> Option<std::sync::MutexGuard<'_, ()>> {
        let t = Instant::now();
        loop {
            match self.doc_lock.try_lock() {
                Ok(g) => return Some(g),
                Err(std::sync::TryLockError::Poisoned(p)) => return Some(p.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) if t.elapsed() < timeout => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(_) => return None,
            }
        }
    }

    pub fn stderr(&self) -> String {
        self.status.lock().unwrap().stderr.clone()
    }

    /// `shutdown` (no params), `exit`, then SIGTERM and SIGKILL to its
    /// session's process group, `grace` apart.
    pub fn shutdown(&self, grace: Duration) {
        if self.alive() {
            let _ = self.request("shutdown", Value::Null, grace);
            self.notify("exit", Value::Null);
        }
        self.stdin.lock().unwrap().take();
        if self.wait(grace) {
            return;
        }
        unsafe { libc::kill(-self.pid, libc::SIGTERM) };
        if !self.wait(grace) {
            self.kill();
        }
    }

    fn wait(&self, d: Duration) -> bool {
        let t = Instant::now();
        loop {
            if let Ok(Some(_)) = self.child.lock().unwrap().try_wait() {
                return true;
            }
            if t.elapsed() >= d {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// SIGKILL its process group and reap it.
    pub fn kill(&self) {
        unsafe { libc::kill(-self.pid, libc::SIGKILL) };
        let mut c = self.child.lock().unwrap();
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// The reader's handle on the server's stdin (the client's own, shared).
struct Writer(Option<Arc<Mutex<Option<ChildStdin>>>>);

impl Writer {
    fn send(&self, msg: &Value) {
        if let Some(f) = &self.0 {
            if let Some(f) = f.lock().unwrap().as_mut() {
                let _ = f.write_all(&encode(msg)).and_then(|_| f.flush());
            }
        }
    }
}

fn handle(
    msg: &Value,
    pending: &Pending,
    status: &Mutex<Status>,
    settings: &Value,
    root: &Path,
    w: &Writer,
) {
    let method = msg["method"].as_str();
    match (method, msg.get("id")) {
        // A response.
        (None, Some(id)) => {
            let Some(id) = id.as_i64() else { return };
            if let Some(tx) = pending.lock().unwrap().remove(&id) {
                let r = match msg.get("error") {
                    Some(e) => Err(LspError::Server(
                        e["code"].as_i64().unwrap_or(0),
                        e["message"].as_str().unwrap_or("error").to_string(),
                    )),
                    None => Ok(msg["result"].clone()),
                };
                let _ = tx.send(r);
            }
        }
        // A request from the server: answer what a client must.
        (Some(m), Some(id)) => {
            let result = match m {
                "workspace/configuration" => Value::Array(
                    msg["params"]["items"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|i| section(settings, i["section"].as_str()))
                        .collect(),
                ),
                "workspace/workspaceFolders" => {
                    json!([{"uri": path_uri(root), "name": root.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()}])
                }
                "window/workDoneProgress/create" => {
                    status.lock().unwrap().reported = true;
                    Value::Null
                }
                _ => Value::Null,
            };
            w.send(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
        }
        (Some("$/progress"), None) => {
            let p = &msg["params"];
            let token = match &p["token"] {
                Value::String(s) => s.clone(),
                v => v.to_string(),
            };
            let mut s = status.lock().unwrap();
            s.reported = true;
            match p["value"]["kind"].as_str() {
                Some("begin") => {
                    s.work.insert(token);
                }
                Some("end") => {
                    s.work.remove(&token);
                }
                _ => {}
            }
        }
        (Some("experimental/serverStatus"), None) => {
            let mut s = status.lock().unwrap();
            s.reported = true;
            s.quiescent = msg["params"]["quiescent"].as_bool();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r83_framing_round_trip_and_skips_other_headers() {
        let a = json!({"jsonrpc": "2.0", "id": 1, "result": {"x": "é"}});
        let b = json!({"jsonrpc": "2.0", "method": "m"});
        let mut bytes = encode(&a);
        bytes.extend(b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n");
        bytes.extend(encode(&b));
        let mut r = std::io::Cursor::new(bytes);
        assert_eq!(read_message(&mut r).unwrap(), Some(a));
        assert_eq!(read_message(&mut r).unwrap(), Some(b));
        assert_eq!(read_message(&mut r).unwrap(), None);
        // The length counts bytes, not characters.
        assert!(String::from_utf8(encode(&json!("é")))
            .unwrap()
            .starts_with("Content-Length: 4\r\n\r\n"));
    }

    #[test]
    fn r83_a_huge_length_is_an_error_not_an_allocation() {
        let mut r = std::io::Cursor::new(b"Content-Length: 99999999999999\r\n\r\n{".to_vec());
        assert!(read_message(&mut r).is_err());
    }

    #[test]
    fn r83_columns_in_utf8_and_utf16() {
        let line = "let é = \"😀\".x;";
        // 'x' is character 13.
        let x = line.chars().position(|c| c == 'x').unwrap();
        assert_eq!(x, 12);
        assert_eq!(to_lsp_col(line, x, Enc::Utf8), 16); // é 2 bytes, 😀 4
        assert_eq!(to_lsp_col(line, x, Enc::Utf16), 13); // 😀 is 2 units
        assert_eq!(from_lsp_col(line, 16, Enc::Utf8), x);
        assert_eq!(from_lsp_col(line, 13, Enc::Utf16), x);
        assert_eq!(from_lsp_col(line, 999, Enc::Utf16), line.chars().count());
        assert_eq!(Enc::of(&json!({"positionEncoding": "utf-8"})), Enc::Utf8);
        assert_eq!(Enc::of(&json!({})), Enc::Utf16);
    }

    #[test]
    fn r83_uris_and_configuration_sections() {
        let p = Path::new("/a b/ü.rs");
        assert_eq!(path_uri(p), "file:///a%20b/%C3%BC.rs");
        assert_eq!(uri_path(&path_uri(p)).unwrap(), p);
        let s = json!({"rust-analyzer": {"files": {"watcher": "server"}}});
        assert_eq!(
            section(&s, Some("rust-analyzer.files.watcher")),
            json!("server")
        );
        assert_eq!(section(&s, Some("typescript")), Value::Null);
        assert_eq!(section(&s, None), s);
    }

    #[test]
    fn r83_reader_answers_server_requests_and_tracks_readiness() {
        let (pending, status): (Pending, Mutex<Status>) = Default::default();
        let (tx, rx) = mpsc::channel();
        pending.lock().unwrap().insert(7, tx);
        let w = Writer(None);
        let root = Path::new("/r");
        let settings = json!({"rust-analyzer": {"checkOnSave": false}});
        handle(
            &json!({"id": 7, "result": [1]}),
            &pending,
            &status,
            &settings,
            root,
            &w,
        );
        assert_eq!(rx.recv().unwrap(), Ok(json!([1])));
        handle(
            &json!({"method": "$/progress", "params": {"token": "t", "value": {"kind": "begin"}}}),
            &pending,
            &status,
            &settings,
            root,
            &w,
        );
        assert!(!status.lock().unwrap().work.is_empty());
        handle(
            &json!({"method": "$/progress", "params": {"token": "t", "value": {"kind": "end"}}}),
            &pending,
            &status,
            &settings,
            root,
            &w,
        );
        assert!(status.lock().unwrap().work.is_empty());
        handle(
            &json!({"method": "experimental/serverStatus", "params": {"quiescent": false}}),
            &pending,
            &status,
            &settings,
            root,
            &w,
        );
        assert_eq!(status.lock().unwrap().quiescent, Some(false));
        let (tx, rx) = mpsc::channel();
        pending.lock().unwrap().insert(8, tx);
        handle(
            &json!({"id": 8, "error": {"code": -32801, "message": "content modified"}}),
            &pending,
            &status,
            &settings,
            root,
            &w,
        );
        assert_eq!(
            rx.recv().unwrap(),
            Err(LspError::Server(-32801, "content modified".into()))
        );
    }
}
