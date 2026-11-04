//! The one writer of RADE's own files (R141, ADR-0052): a unique temp file in
//! the target's folder, fsync of the file and the folder, a rename. A symlink
//! is written through (the link stays, its target changes). [`Mode::Private`]
//! files (tokens, recorded Agent argv / env) are always `0600`;
//! [`Mode::Keep`] files keep the target's mode (a new file gets `0666` under
//! the umask). [`with_lock`] adds an advisory `flock` on a sibling
//! `.<name>.lock` around a read-change-write.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Which mode the written file gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `0600`, whatever the file had before.
    Private,
    /// The target's mode; a new file: `0666` under the umask.
    Keep,
}

/// `path` with symlinks followed to the real file (a dangling link gives the
/// path it points to, so the file is created there).
pub fn resolve(path: &Path) -> PathBuf {
    let mut p = path.to_path_buf();
    for _ in 0..40 {
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => match std::fs::read_link(&p) {
                Ok(t) => {
                    p = match p.parent() {
                        Some(d) => d.join(t),
                        None => t,
                    }
                }
                Err(_) => break,
            },
            _ => break,
        }
    }
    p
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique() -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{}.{n}.{t:x}", std::process::id())
}

fn split(target: &Path) -> (&Path, String) {
    let dir = target.parent().filter(|d| !d.as_os_str().is_empty());
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    (dir.unwrap_or(Path::new(".")), name)
}

/// Write `bytes` to `path` (creating its folder): all or nothing.
pub fn write(path: &Path, bytes: &[u8], mode: Mode) -> io::Result<()> {
    let target = resolve(path);
    let (dir, name) = split(&target);
    std::fs::create_dir_all(dir)?;
    // `Some` = chmod to it; `None` = a new Keep file, the umask decides.
    let (create, chmod) = match mode {
        Mode::Private => (0o600, Some(0o600)),
        Mode::Keep => match std::fs::metadata(&target) {
            Ok(m) => (0o600, Some(m.permissions().mode() & 0o7777)),
            Err(_) => (0o666, None),
        },
    };
    let (tmp, mut f) = loop {
        let tmp = dir.join(format!(".{name}.{}.tmp", unique()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(create)
            .open(&tmp)
        {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let res = (|| {
        f.write_all(bytes)?;
        if let Some(m) = chmod {
            f.set_permissions(std::fs::Permissions::from_mode(m))?;
        }
        f.sync_all()?;
        std::fs::rename(&tmp, &target)?;
        File::open(dir)?.sync_all()
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// An advisory exclusive lock on `.<name>.lock` beside `path`'s real file,
/// released when dropped.
pub struct Lock(#[allow(dead_code)] File);

impl Lock {
    pub fn acquire(path: &Path) -> io::Result<Lock> {
        let target = resolve(path);
        let (dir, name) = split(&target);
        std::fs::create_dir_all(dir)?;
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(dir.join(format!(".{name}.lock")))?;
        // SAFETY: a plain syscall on an fd we own; released when `f` drops.
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Lock(f))
    }
}

/// The current text of `path` (`""` when missing).
pub fn read_text(path: &Path) -> io::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e),
    }
}

/// Read-change-write under the lock: `change` gets the current text (empty
/// when the file is missing) and returns the new one, or `None` to write
/// nothing; an unchanged text is not written either. Returns what `change`
/// returned.
pub fn with_lock<E: From<io::Error>>(
    path: &Path,
    mode: Mode,
    change: impl FnOnce(&str) -> Result<Option<String>, E>,
) -> Result<Option<String>, E> {
    let _lock = Lock::acquire(path)?;
    let current = read_text(&resolve(path))?;
    let new = change(&current)?;
    if let Some(t) = &new {
        if *t != current {
            write(path, t.as_bytes(), mode)?;
        }
    }
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn dir(n: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rade-fsa-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o7777
    }

    #[test]
    fn r141_a_private_is_0600_also_when_it_was_0644() {
        let d = dir("a");
        let f = d.join("accounts.toml");
        std::fs::write(&f, "old").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        write(&f, b"new", Mode::Private).unwrap();
        assert_eq!(mode(&f), 0o600);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "new");
        let g = d.join("fresh.json");
        write(&g, b"x", Mode::Private).unwrap();
        assert_eq!(mode(&g), 0o600);
        let left = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(left, 0, "no temp file is left");
    }

    #[test]
    fn r141_a_keep_keeps_the_mode() {
        let d = dir("keep");
        let f = d.join("config.toml");
        std::fs::write(&f, "old").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o640)).unwrap();
        write(&f, b"new", Mode::Keep).unwrap();
        assert_eq!(mode(&f), 0o640);
        let g = d.join("new.toml");
        write(&g, b"x", Mode::Keep).unwrap();
        assert_eq!(mode(&g) & 0o600, 0o600);
        assert_eq!(mode(&g) & !0o666, 0);
    }

    #[test]
    fn r141_b_a_symlink_stays_and_its_target_changes() {
        let d = dir("b");
        let real = d.join("dotfiles/accounts.toml");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "old").unwrap();
        let link = d.join("accounts.toml");
        symlink("dotfiles/accounts.toml", &link).unwrap();
        write(&link, b"new", Mode::Private).unwrap();
        let is_link = |p: &Path| {
            std::fs::symlink_metadata(p)
                .unwrap()
                .file_type()
                .is_symlink()
        };
        assert!(is_link(&link));
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "new");
        assert_eq!(mode(&real), 0o600);
        // A dangling link is created at its target.
        let l2 = d.join("l2");
        symlink("made.toml", &l2).unwrap();
        write(&l2, b"hi", Mode::Keep).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("made.toml")).unwrap(), "hi");
        assert!(is_link(&l2));
    }

    #[test]
    fn r141_c_two_writers_in_parallel_keep_both_edits() {
        let d = dir("c");
        let f = d.join("accounts.toml");
        let mut hs = Vec::new();
        for t in 0..8 {
            let f = f.clone();
            hs.push(std::thread::spawn(move || {
                for i in 0..10 {
                    with_lock::<io::Error>(&f, Mode::Private, |cur| {
                        Ok(Some(format!("{cur}t{t}-{i}\n")))
                    })
                    .unwrap();
                }
            }));
        }
        for h in hs {
            h.join().unwrap();
        }
        let text = std::fs::read_to_string(&f).unwrap();
        assert_eq!(text.lines().count(), 80);
        assert_eq!(mode(&f), 0o600);
    }
}
