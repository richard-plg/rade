//! Image paste and file drops (R5, ADR-0004): which clipboard representation
//! wins, `text/uri-list` → shell-escaped paths, paste file names, saving and
//! the 7-day cleanup of `$XDG_CACHE_HOME/rade/paste`.

use shell_quote::Bash;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Paste files older than this are deleted on startup (R5.4).
pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// One way to turn the clipboard into pasted text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clip {
    /// `text/uri-list`: local files → their shell-escaped paths.
    Files,
    /// Plain text, pasted as is.
    Text,
    /// Image data → saved as PNG, its path pasted.
    Image,
}

/// Sources to try, best first (R5.1): copied files, then text, then an image.
/// Text beats an image, so a clipboard offering both pastes the text; a later
/// source is used only when an earlier one yields nothing (e.g. a uri-list of
/// `https://` URLs).
pub fn clip_sources<'a>(
    mime_types: impl IntoIterator<Item = &'a str>,
    has_texture: bool,
) -> Vec<Clip> {
    let (mut files, mut text, mut image) = (false, false, has_texture);
    for m in mime_types {
        let m = m.to_ascii_lowercase();
        files |= m == "text/uri-list";
        text |= m == "text/plain" || m.starts_with("text/plain;");
        image |= m.starts_with("image/");
    }
    [
        (files, Clip::Files),
        (text, Clip::Text),
        (image, Clip::Image),
    ]
    .into_iter()
    .filter_map(|(on, s)| on.then_some(s))
    .collect()
}

/// Local paths in a `text/uri-list` (RFC 2483): `#` comments skipped,
/// `file:` URIs percent-decoded, other schemes ignored.
pub fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let rest = l.get(..5).filter(|s| s.eq_ignore_ascii_case("file:"))?;
            let rest = &l[rest.len()..];
            // file:///p, file://host/p, file:/p
            let path = match rest.strip_prefix("//") {
                Some(r) => &r[r.find('/')?..],
                None => rest,
            };
            let path = path.split(['?', '#']).next().unwrap_or(path);
            Some(PathBuf::from(OsString::from_vec(
                crate::link::percent_decode_bytes(path),
            )))
        })
        .collect()
}

/// Shell-quoted paths joined by spaces, as a drop or a pasted file list
/// types them (R5.2); `None` for no paths. Plain paths stay bare; others are
/// single-quoted like GNOME Terminal's drops (`'/a b.png'`), the form Agents'
/// pasted-path detection understands; only control bytes or invalid UTF-8
/// fall back to Bash's `$'…'`.
pub fn escape_paths<P: AsRef<Path>>(paths: &[P]) -> Option<String> {
    let parts: Vec<String> = paths
        .iter()
        .map(|p| quote(p.as_ref().as_os_str().as_bytes()))
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn quote(b: &[u8]) -> String {
    let bash = Bash::quote_vec(b);
    match std::str::from_utf8(b) {
        Ok(s) if bash != b && !s.chars().any(char::is_control) => {
            format!("'{}'", s.replace('\'', r"'\''"))
        }
        _ => String::from_utf8_lossy(&bash).into_owned(),
    }
}

/// `yyyymmdd-hhmmss-mmm` for `secs` since the epoch in *local* time.
fn stamp(secs: i64, millis: u32) -> String {
    let (y, m, d, h, mi, s) = crate::date::civil(secs);
    format!("{y:04}{m:02}{d:02}-{h:02}{mi:02}{s:02}-{millis:03}")
}

/// Paste file name for `now`, in the local time zone: `<yyyymmdd-hhmmss-ms>.png`.
pub fn file_name(now: SystemTime) -> String {
    let d = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    format!(
        "{}.png",
        stamp(crate::date::local_secs(now), d.subsec_millis())
    )
}

/// Write `png` into `dir` (created 0700) as a new 0600 file named for `now`;
/// a second paste in the same millisecond gets a `-1`, `-2`… suffix.
pub fn save(dir: &Path, png: &[u8], now: SystemTime) -> io::Result<PathBuf> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    let name = file_name(now);
    let stem = name.trim_end_matches(".png");
    for n in 0..100 {
        let path = match n {
            0 => dir.join(&name),
            n => dir.join(format!("{stem}-{n}.png")),
        };
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(mut f) => {
                f.write_all(png)?;
                return Ok(path);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "too many pastes this millisecond",
    ))
}

/// Entries (path, mtime) older than `max_age` at `now`.
pub fn expired(
    entries: &[(PathBuf, SystemTime)],
    now: SystemTime,
    max_age: Duration,
) -> Vec<PathBuf> {
    entries
        .iter()
        .filter(|(_, mtime)| now.duration_since(*mtime).is_ok_and(|age| age > max_age))
        .map(|(p, _)| p.clone())
        .collect()
}

/// Delete the regular files in `dir` older than [`MAX_AGE`] (R5.4); returns them.
pub fn cleanup(dir: &Path, now: SystemTime) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let entries: Vec<_> = read
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(|m| m.is_file())?;
            Some((e.path(), meta.modified().ok()?))
        })
        .collect();
    let old = expired(&entries, now, MAX_AGE);
    old.into_iter()
        .filter(|p| fs::remove_file(p).is_ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use Clip::*;

    #[test]
    fn r5_1_files_then_text_then_image() {
        assert_eq!(clip_sources(["image/png"], true), [Image]);
        assert_eq!(clip_sources(["image/jpeg"], false), [Image]);
        assert_eq!(clip_sources([], true), [Image], "a texture without a mime");
        assert_eq!(
            clip_sources(["text/plain;charset=utf-8", "image/png"], true),
            [Text, Image]
        );
        assert_eq!(clip_sources(["TEXT/PLAIN", "text/html"], false), [Text]);
        assert_eq!(
            clip_sources(
                [
                    "x-special/gnome-copied-files",
                    "text/uri-list",
                    "text/plain"
                ],
                false
            ),
            [Files, Text]
        );
        // A browser's copied image comes with HTML, not plain text.
        assert_eq!(clip_sources(["text/html", "image/png"], true), [Image]);
        assert!(clip_sources(["text/html", "application/json"], false).is_empty());
    }

    #[test]
    fn r5_2_uri_list_local_paths_percent_decoded() {
        let list = "# comment\r\nfile:///home/u/My%20Shot.png\r\nhttps://example.com/a.png\r\n\
                    file://localhost/tmp/a%2Bb.jpg\nfile:/x/y\n\nFILE:///caps\nfile:///q?x#y\n";
        assert_eq!(
            parse_uri_list(list),
            ["/home/u/My Shot.png", "/tmp/a+b.jpg", "/x/y", "/caps", "/q"].map(PathBuf::from)
        );
        assert!(parse_uri_list("https://example.com/\n").is_empty());
        // Non-UTF-8 file names survive.
        assert_eq!(
            parse_uri_list("file:///t/%FF.png")[0]
                .as_os_str()
                .as_bytes(),
            b"/t/\xff.png"
        );
    }

    #[test]
    fn r5_2_paths_are_shell_escaped_and_space_joined() {
        assert_eq!(escape_paths::<&str>(&[]), None);
        assert_eq!(escape_paths(&["/a/b.png"]).unwrap(), "/a/b.png");
        assert_eq!(escape_paths(&["/a b/c.png"]).unwrap(), "'/a b/c.png'");
        assert_eq!(escape_paths(&["/it's"]).unwrap(), r"'/it'\''s'");
        let s = escape_paths(&["/a b/c.png", "/x/it's$(rm).png", "/n\nl"]).unwrap();
        // Round trip through a real shell: three words, byte for byte.
        let out = std::process::Command::new("bash")
            .args(["-c", &format!("printf '%s\\0' {s}")])
            .output()
            .unwrap();
        assert_eq!(out.stdout, b"/a b/c.png\0/x/it's$(rm).png\0/n\nl\0");
    }

    #[test]
    fn r5_1_file_name_stamp() {
        assert_eq!(stamp(0, 0), "19700101-000000-000");
        // 2026-10-01 13:04:05.678 UTC
        assert_eq!(stamp(1_790_859_845, 678), "20261001-130405-678");
        assert_eq!(stamp(951_782_400, 7), "20000229-000000-007", "leap day");
        let n = file_name(SystemTime::now());
        assert!(
            n.len() == "yyyymmdd-hhmmss-mmm.png".len() && n.ends_with(".png"),
            "{n}"
        );
    }

    #[test]
    fn r5_1_save_is_private_and_never_overwrites() {
        let t = std::env::temp_dir().join(format!("rade-paste-{}", std::process::id()));
        let dir = t.join("rade/paste");
        let now = SystemTime::now();
        let a = save(&dir, b"\x89PNG one", now).unwrap();
        let b = save(&dir, b"\x89PNG two", now).unwrap();
        assert_ne!(a, b);
        assert!(b.to_str().unwrap().ends_with("-1.png"));
        assert_eq!(fs::read(&a).unwrap(), b"\x89PNG one");
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&a), 0o600);
        assert_eq!(mode(&dir), 0o700);
        fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn r5_4_cleanup_selects_by_mtime() {
        let now = SystemTime::now();
        let day = Duration::from_secs(86_400);
        let entries = [
            (PathBuf::from("old"), now - 8 * day),
            (PathBuf::from("edge"), now - 7 * day),
            (PathBuf::from("new"), now - day),
            (PathBuf::from("future"), now + day),
        ];
        assert_eq!(expired(&entries, now, MAX_AGE), [PathBuf::from("old")]);

        let dir = std::env::temp_dir().join(format!("rade-clean-{}", std::process::id()));
        fs::create_dir_all(dir.join("sub")).unwrap();
        let old = dir.join("old.png");
        fs::write(&old, b"x").unwrap();
        fs::write(dir.join("new.png"), b"x").unwrap();
        fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(now - 8 * day)
            .unwrap();
        assert_eq!(cleanup(&dir, now), std::slice::from_ref(&old));
        assert!(!old.exists() && dir.join("new.png").exists() && dir.join("sub").exists());
        assert!(cleanup(&dir.join("missing"), now).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }
}
