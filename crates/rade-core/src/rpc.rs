//! JSON-RPC 2.0 messages and error kinds of the socket contract (docs/API.md).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Error kinds and their CLI exit codes (API.md "Exit codes").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorKind {
    Internal,
    Usage,
    NotRunning,
    NotFound,
    Ambiguous,
    Limit,
    Timeout,
    /// An Agent Account of another Workspace, or a command that would run
    /// an account's Pane under another config dir (1.0.0, R108.3, R108.7).
    Forbidden,
}

impl ErrorKind {
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorKind::Internal => 1,
            ErrorKind::Usage => 2,
            ErrorKind::NotRunning => 3,
            ErrorKind::NotFound | ErrorKind::Ambiguous => 4,
            ErrorKind::Limit => 5,
            ErrorKind::Timeout => 6,
            ErrorKind::Forbidden => 7,
        }
    }
}

/// Domain error shared by the model, the server, and the CLI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
    /// The release that removed the method or command (R101.2), sent as
    /// `data.removed`.
    pub removed: Option<String>,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            removed: None,
        }
    }
    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Usage, message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::NotFound, message)
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Forbidden, message)
    }
}

/// The release that removed the Task, Batch, Initiative and Report engines
/// (ADR-0033).
pub const REMOVED_IN: &str = "1.0.0";

/// Socket method families removed in 1.0.0 (R101.2): every method under
/// these prefixes, plus the exact names of [`REMOVED_METHODS`].
const REMOVED_PREFIXES: &[&str] = &[
    "task.",
    "batch.",
    "initiative.",
    "digest.",
    "report.",
    "plan.",
    "verify.",
];

/// Single removed methods (R101.2).
const REMOVED_METHODS: &[&str] = &[
    "flow.get",
    "lessons.draft",
    "debug.batch",
    "debug.initiative",
    "debug.reports",
];

/// The answer for a method removed in 1.0.0 (R101.2), `None` for any other
/// method (kept or unknown).
pub fn removed(method: &str) -> Option<Error> {
    let gone =
        REMOVED_METHODS.contains(&method) || REMOVED_PREFIXES.iter().any(|p| method.starts_with(p));
    gone.then(|| Error {
        kind: ErrorKind::Usage,
        message: format!("'{method}' was removed in RADE {REMOVED_IN} (ADR-0033)"),
        removed: Some(REMOVED_IN.into()),
    })
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Set to `1` in the environment that starts `rade-app` to serve the hidden
/// `debug.*` methods (tests only, ADR-0021). Never passed on to Panes.
pub const DEBUG_ENV: &str = "RADE_DEBUG_SOCKET";

/// `debug.*` methods drive the UI's own buttons (Approve…, Push…): only an
/// app started in test mode serves them (ADR-0021).
pub fn gate_debug(method: &str, enabled: bool) -> Result<()> {
    if enabled || !method.starts_with("debug.") {
        return Ok(());
    }
    Err(Error::usage(format!(
        "{method}: debug methods are disabled; they are for tests: start rade-app with {DEBUG_ENV}=1 (ADR-0021)"
    )))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    pub fn new(id: u64, method: &str, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ErrorData {
    pub kind: ErrorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: ErrorData,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Value, e: &Error) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(RpcError {
                // JSON-RPC reserves -32768..-32000 for protocol errors; ours
                // are application errors, numbered by exit code.
                code: e.kind.exit_code() as i64,
                message: e.message.clone(),
                data: ErrorData {
                    kind: e.kind,
                    removed: e.removed.clone(),
                },
            }),
        }
    }

    pub fn into_result(self) -> Result<Value> {
        match (self.result, self.error) {
            (_, Some(e)) => Err(Error {
                removed: e.data.removed,
                ..Error::new(e.data.kind, e.message)
            }),
            (Some(v), None) => Ok(v),
            (None, None) => Ok(Value::Object(Default::default())),
        }
    }
}

/// One row of `pane.list` (API.md). Agent fields stay null until M2/M6.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneInfo {
    pub pane: String,
    pub name: Option<String>,
    /// Its Source's Workspace (R23.4).
    pub workspace: String,
    pub source: String,
    pub worktree: String,
    pub tab: String,
    pub parent: Option<String>,
    pub agent: Option<String>,
    pub status: Option<String>,
    /// `hook` (an Agent hook, `rade status`, or RADE itself: spawn with a
    /// prompt, `send --enter`, process exit) or `heuristic` (an R10.2
    /// fallback); null without a status.
    pub status_source: Option<String>,
    /// When `status` last changed (Unix seconds).
    pub status_since: Option<u64>,
    pub unread: u32,
    pub cwd: String,
    /// Terminal title (OSC 0/2), sanitized; null if none set.
    pub title: Option<String>,
    /// Its Agent Account (R108.2); null without one.
    pub account: Option<String>,
    /// `account` was detected from a hand-started Agent (R125.4).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub account_detected: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn r101_b_removed_methods_answer_removed_in_0_8_0() {
        for m in [
            "task.list",
            "batch.status",
            "initiative.list",
            "digest.get",
            "report.list",
            "flow.get",
            "lessons.draft",
            "debug.batch",
        ] {
            let e = removed(m).unwrap_or_else(|| panic!("{m} must be removed"));
            let r = serde_json::to_value(Response::err(json!(1), &e)).unwrap();
            assert_eq!(
                r["error"],
                json!({
                    "code": 2,
                    "message": format!("'{m}' was removed in RADE 1.0.0 (ADR-0033)"),
                    "data": {"kind": "USAGE", "removed": "1.0.0"}
                }),
                "{m}"
            );
        }
        // Every other removed family and debug op.
        for m in [
            "task.start",
            "batch.add",
            "initiative.start",
            "report.new",
            "plan.approve",
            "debug.initiative",
            "debug.reports",
        ] {
            assert!(removed(m).is_some(), "{m}");
        }
        // Kept methods and an unknown one are not "removed".
        for m in [
            "foo.bar",
            "pane.list",
            "project.list",
            "fanout.start",
            "debug.ui",
        ] {
            assert!(removed(m).is_none(), "{m}");
        }
        let unknown = Error::usage("unknown method 'foo.bar'");
        let r = serde_json::to_value(Response::err(json!(2), &unknown)).unwrap();
        assert_eq!(r["error"]["code"], 2);
        assert_eq!(r["error"]["data"], json!({"kind": "USAGE"}));
        // The client side keeps the marker.
        let back: Response = serde_json::from_value(
            serde_json::to_value(Response::err(json!(3), &removed("task.list").unwrap())).unwrap(),
        )
        .unwrap();
        assert_eq!(
            back.into_result().unwrap_err().removed.as_deref(),
            Some("1.0.0")
        );
    }

    #[test]
    fn debug_methods_need_test_mode() {
        let e = gate_debug("debug.batch", false).unwrap_err();
        assert_eq!(e.kind, ErrorKind::Usage);
        assert!(e.message.contains("debug methods are disabled") && e.message.contains(DEBUG_ENV));
        assert!(gate_debug("debug.state", false).is_err());
        assert!(gate_debug("debug.batch", true).is_ok());
        for m in ["pane.list", "batch.status", "app.present", "debugx"] {
            assert!(gate_debug(m, false).is_ok(), "{m}");
        }
    }

    #[test]
    fn exit_codes_match_api_table() {
        use ErrorKind::*;
        let codes: Vec<i32> = [
            Internal, Usage, NotRunning, NotFound, Ambiguous, Limit, Timeout, Forbidden,
        ]
        .iter()
        .map(|k| k.exit_code())
        .collect();
        assert_eq!(codes, [1, 2, 3, 4, 4, 5, 6, 7]);
    }

    #[test]
    fn error_response_shape() {
        let r = Response::err(json!(7), &Error::not_found("no pane p_9"));
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(
            v,
            json!({"jsonrpc":"2.0","id":7,"error":{"code":4,"message":"no pane p_9","data":{"kind":"NOT_FOUND"}}})
        );
        let back: Response = serde_json::from_value(v).unwrap();
        assert_eq!(back.into_result().unwrap_err().kind, ErrorKind::NotFound);
    }

    #[test]
    fn ok_response_and_request_roundtrip() {
        let req = Request::new(1, "pane.list", json!({}));
        let line = serde_json::to_string(&req).unwrap();
        assert_eq!(
            line,
            r#"{"jsonrpc":"2.0","id":1,"method":"pane.list","params":{}}"#
        );
        let r = Response::ok(json!(1), json!({"panes": []}));
        assert_eq!(r.into_result().unwrap(), json!({"panes": []}));
    }

    #[test]
    fn request_params_default_to_null() {
        let req: Request =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":1,"method":"x"}"#).unwrap();
        assert_eq!(req.params, Value::Null);
    }
}
