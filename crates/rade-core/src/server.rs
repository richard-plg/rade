//! Socket transport (API.md "Transport"): Unix socket, mode 0600, same-UID
//! peers only (R2.5, R2-c), newline-delimited JSON-RPC. The app supplies
//! the dispatcher; this module owns framing and access control.

use crate::rpc::{Error, Request, Response};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;

/// Longest accepted request line; protects the app from a runaway client.
const MAX_LINE: u64 = 4 << 20;

pub type Dispatch = dyn Fn(Request) -> Response + Send + Sync;

/// A peer may talk to RADE only if it runs as our own user.
pub fn peer_allowed(peer_uid: u32, own_uid: u32) -> bool {
    peer_uid == own_uid
}

pub fn own_uid() -> u32 {
    unsafe { libc::getuid() }
}

thread_local! {
    /// The PID of the peer this connection thread serves (`SO_PEERCRED`).
    static PEER_PID: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// The PID of the process that sent the request being dispatched on this
/// thread (a socket connection thread), if known. For refusals only: a PID
/// says which Pane a caller runs in even when it dropped `RADE_PANE_ID`.
pub fn peer_pid() -> Option<u32> {
    PEER_PID.with(|p| p.get())
}

/// UID (and PID) of the process on the other end (`SO_PEERCRED`). Lookup
/// follows limux (MIT) `rust/limux-control/src/auth.rs`.
pub fn peer_uid(stream: &UnixStream) -> std::io::Result<u32> {
    peer_cred(stream).map(|c| c.0)
}

fn peer_cred(stream: &UnixStream) -> std::io::Result<(u32, u32)> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((cred.uid, cred.pid as u32))
}

/// Bind the socket (refusing if another RADE answers on it) and serve each
/// connection on its own thread.
pub fn listen(path: &Path, dispatch: Arc<Dispatch>) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    if UnixStream::connect(path).is_ok() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!("another RADE is serving {}", path.display()),
        ));
    }
    let _ = std::fs::remove_file(path); // stale socket from a crash
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    std::thread::Builder::new()
        .name("rade-socket".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let dispatch = dispatch.clone();
                let _ = std::thread::Builder::new()
                    .name("rade-conn".into())
                    .spawn(move || serve_connection(stream, &*dispatch));
            }
        })?;
    Ok(())
}

fn serve_connection(stream: UnixStream, dispatch: &Dispatch) {
    match peer_cred(&stream) {
        Ok((uid, pid)) if peer_allowed(uid, own_uid()) => PEER_PID.with(|p| p.set(Some(pid))),
        _ => return, // dropping the stream closes it
    }
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream).take(MAX_LINE);
    let mut line = String::new();
    loop {
        line.clear();
        reader.set_limit(MAX_LINE);
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Request>(&line) {
            Ok(req) => dispatch(req),
            Err(e) => Response::err(
                Value::Null,
                &Error::usage(format!("invalid JSON-RPC request: {e}")),
            ),
        };
        let mut text = serde_json::to_string(&resp).expect("serializable");
        text.push('\n');
        if out.write_all(text.as_bytes()).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_same_uid_is_allowed() {
        assert!(peer_allowed(1000, 1000));
        assert!(!peer_allowed(0, 1000));
        assert!(!peer_allowed(1001, 1000));
    }

    #[test]
    fn peer_uid_reads_real_credentials() {
        let (a, _b) = UnixStream::pair().unwrap();
        assert_eq!(peer_uid(&a).unwrap(), own_uid());
    }

    #[test]
    fn serves_newline_json_rpc_with_0600_socket() {
        let dir = std::env::temp_dir().join(format!("rade-srv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("rade/rade.sock");
        let echo: Arc<Dispatch> = Arc::new(|req: Request| {
            if req.method == "fail" {
                Response::err(req.id, &Error::not_found("nope"))
            } else {
                Response::ok(req.id, json!({"method": req.method, "params": req.params}))
            }
        });
        listen(&path, echo.clone()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dmode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dmode, 0o700);

        let mut s = UnixStream::connect(&path).unwrap();
        s.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"a\",\"params\":{\"x\":1}}\n\nnot json\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"fail\"}\n").unwrap();
        let mut r = BufReader::new(s);
        let mut lines = Vec::new();
        for _ in 0..3 {
            let mut l = String::new();
            r.read_line(&mut l).unwrap();
            lines.push(serde_json::from_str::<Response>(&l).unwrap());
        }
        assert_eq!(
            lines[0].clone().into_result().unwrap(),
            json!({"method":"a","params":{"x":1}})
        );
        assert_eq!(
            lines[1].clone().into_result().unwrap_err().kind,
            crate::ErrorKind::Usage
        );
        assert_eq!(
            lines[2].clone().into_result().unwrap_err().kind,
            crate::ErrorKind::NotFound
        );

        // A second server on the same live socket is refused.
        assert_eq!(
            listen(&path, echo).unwrap_err().kind(),
            std::io::ErrorKind::AddrInUse
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
