//! Listening TCP ports of the processes in each Pane (PRD R18.4): dev
//! servers an Agent or the user started, for the sidebar tooltip and the
//! palette's "Open localhost:<port>". A process belongs to the Pane named
//! by `RADE_PANE_ID` in its environment (every process started in a
//! Terminal inherits it, see process.rs); its listening sockets are the
//! `socket:[inode]` fds that `/proc/net/tcp{,6}` lists in state LISTEN.
//! An Agent program's own listeners (Codex's local auth-gated server) are
//! not dev servers and are left out; what it starts still counts.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

/// TCP state LISTEN in `/proc/net/tcp`.
const LISTEN: &str = "0A";

/// (socket inode, local port) of each listening socket in a
/// `/proc/net/tcp` or `tcp6` table.
pub fn listening(table: &str) -> Vec<(u64, u16)> {
    table
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.get(3) != Some(&LISTEN) {
                return None;
            }
            let port = u16::from_str_radix(f.get(1)?.rsplit_once(':')?.1, 16).ok()?;
            Some((f.get(9)?.parse().ok()?, port))
        })
        .collect()
}

/// Listening ports per Pane id, from a `/proc`-like tree (`/proc` itself in
/// the app, a fake one in tests). Only processes we may read count, and only
/// those of this RADE (`RADE_SOCKET=<socket>`): every instance numbers its
/// Panes `p_1`, `p_2`, …. A process running a built-in Agent Preset's
/// program (`preset::detect` on its argv) is skipped.
pub fn pane_ports(proc: &Path, socket: &str) -> BTreeMap<String, BTreeSet<u16>> {
    let mine = format!("RADE_SOCKET={socket}");
    let mut inodes: HashMap<u64, u16> = HashMap::new();
    for t in ["net/tcp", "net/tcp6"] {
        if let Ok(s) = std::fs::read_to_string(proc.join(t)) {
            inodes.extend(listening(&s));
        }
    }
    let mut out: BTreeMap<String, BTreeSet<u16>> = BTreeMap::new();
    if inodes.is_empty() {
        return out;
    }
    let Ok(dir) = std::fs::read_dir(proc) else {
        return out;
    };
    let presets = crate::preset::builtins();
    for e in dir.flatten() {
        if !e
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        let Ok(fds) = std::fs::read_dir(e.path().join("fd")) else {
            continue;
        };
        let ports: Vec<u16> = fds
            .flatten()
            .filter_map(|fd| std::fs::read_link(fd.path()).ok())
            .filter_map(|l| {
                let l = l.to_string_lossy().into_owned();
                let inode = l
                    .strip_prefix("socket:[")?
                    .strip_suffix(']')?
                    .parse()
                    .ok()?;
                inodes.get(&inode).copied()
            })
            .collect();
        if ports.is_empty() {
            continue;
        }
        let Ok(env) = std::fs::read(e.path().join("environ")) else {
            continue;
        };
        if !env.split(|b| *b == 0).any(|v| v == mine.as_bytes()) {
            continue;
        }
        let pane = env
            .split(|b| *b == 0)
            .find_map(|v| v.strip_prefix(b"RADE_PANE_ID="))
            .map(|p| String::from_utf8_lossy(p).into_owned());
        let argv: Vec<String> = std::fs::read(e.path().join("cmdline"))
            .unwrap_or_default()
            .split(|b| *b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        if crate::preset::detect(&presets, &argv).is_some() {
            continue;
        }
        if let Some(pane) = pane {
            out.entry(pane).or_default().extend(ports);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000:1F40 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 111 1 0 100 0 0 10 0
   1: 0100007F:0BB8 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 222 1 0 100 0 0 10 0
   2: 0100007F:A1B2 0100007F:1F40 01 00000000:00000000 00:00000000 00000000  1000        0 333 1 0 100 0 0 10 0
";
    const TCP6: &str = "  sl  local_address remote_address st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000000000000:1F40 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 444 1 0 100 0 0 10 0
";

    #[test]
    fn r18_listening_reads_listen_rows_only() {
        assert_eq!(listening(TCP), [(111, 8000), (222, 3000)]);
        assert_eq!(listening(TCP6), [(444, 8000)]);
        assert!(listening("header only\n").is_empty());
    }

    #[test]
    fn r18_ports_map_to_the_pane_in_the_process_environment() {
        let root = std::env::temp_dir().join(format!("rade-ports-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("net")).unwrap();
        std::fs::write(root.join("net/tcp"), TCP).unwrap();
        std::fs::write(root.join("net/tcp6"), TCP6).unwrap();
        let proc_ = |pid: u32, env: &str, sockets: &[u64]| {
            let d = root.join(pid.to_string());
            std::fs::create_dir_all(d.join("fd")).unwrap();
            std::fs::write(d.join("environ"), env).unwrap();
            std::fs::write(d.join("cmdline"), "python3\0-m\0http.server\0").unwrap();
            std::os::unix::fs::symlink("/dev/null", d.join("fd/0")).unwrap();
            for (i, s) in sockets.iter().enumerate() {
                std::os::unix::fs::symlink(
                    format!("socket:[{s}]"),
                    d.join(format!("fd/{}", i + 3)),
                )
                .unwrap();
            }
        };
        // A dev server on 8000 (v4 + v6) in p_1; one on 3000 in p_2; a
        // client socket (333, not listening) and a process outside RADE.
        proc_(
            10,
            "HOME=/h\0RADE_PANE_ID=p_1\0RADE_SOCKET=/s\0",
            &[111, 444],
        );
        proc_(11, "RADE_SOCKET=/s\0RADE_PANE_ID=p_2\0", &[222, 333]);
        proc_(12, "RADE_PANE_ID=p_3\0RADE_SOCKET=/s\0", &[333]);
        proc_(13, "HOME=/h\0", &[]);
        // Another RADE's Pane p_1 (same id, other socket): not ours.
        proc_(14, "RADE_PANE_ID=p_1\0RADE_SOCKET=/other\0", &[222]);
        // p_4's Codex listens itself (auth-gated, not a dev server); the
        // dev server it started in the same Pane counts.
        proc_(15, "RADE_PANE_ID=p_4\0RADE_SOCKET=/s\0", &[111]);
        std::fs::write(root.join("15/cmdline"), "codex\0--no-daemon\0resume\0x\0").unwrap();
        proc_(16, "RADE_PANE_ID=p_4\0RADE_SOCKET=/s\0", &[222]);
        std::fs::create_dir_all(root.join("self")).unwrap();
        let ports = pane_ports(&root, "/s");
        assert_eq!(
            ports,
            BTreeMap::from([
                ("p_1".to_string(), BTreeSet::from([8000])),
                ("p_2".to_string(), BTreeSet::from([3000])),
                ("p_4".to_string(), BTreeSet::from([3000])),
            ])
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn r18_finds_a_real_listener_of_this_process() {
        // This test process has no RADE_PANE_ID (unless run inside RADE),
        // so its listener maps to no Pane but must not break the scan.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let tcp = std::fs::read_to_string("/proc/net/tcp").unwrap();
        assert!(listening(&tcp).iter().any(|(_, p)| *p == port));
        let socket = std::env::var("RADE_SOCKET").unwrap_or_default();
        let ports = pane_ports(Path::new("/proc"), &socket);
        if let (Ok(me), false) = (std::env::var("RADE_PANE_ID"), socket.is_empty()) {
            assert!(ports[&me].contains(&port));
        }
    }
}
