//! The dotenv files of Agent Accounts (PRD R125.1, ADR-0047): `KEY=VALUE`
//! lines, an optional leading `export `, `'…'` (literal) or `"…"` (`\n`,
//! `\"`, `\\`) values, `#` comment lines and blank lines. No `$VAR`
//! interpolation, no multi-line values: anything else is an error naming
//! the line, so a value is never guessed.

use std::path::Path;

/// A variable name RADE accepts: `[A-Za-z_][A-Za-z0-9_]*`.
pub fn valid_key(k: &str) -> bool {
    let mut c = k.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

/// Parse a dotenv text into its variables, in file order (a key given
/// twice: the later line wins, in the earlier one's place).
/// `Err((line, reason))` with the 1-based line number.
pub fn parse(text: &str) -> Result<Vec<(String, String)>, (usize, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line
            .strip_prefix("export ")
            .map(str::trim_start)
            .unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else {
            return Err((n, "expected KEY=VALUE".into()));
        };
        let k = k.trim_end();
        if !valid_key(k) {
            return Err((n, format!("'{k}' is not a variable name")));
        }
        let v = value(v.trim_start()).map_err(|e| (n, e))?;
        match out.iter_mut().find(|(x, _)| x == k) {
            Some(slot) => slot.1 = v,
            None => out.push((k.to_string(), v)),
        }
    }
    Ok(out)
}

fn value(v: &str) -> Result<String, String> {
    if let Some(rest) = v.strip_prefix('\'') {
        let end = rest.find('\'').ok_or("unclosed ' quote")?;
        return tail(&rest[end + 1..]).map(|_| rest[..end].to_string());
    }
    if let Some(rest) = v.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return tail(&rest[i + 1..]).map(|_| out),
                '\\' => match chars.next() {
                    Some((_, 'n')) => out.push('\n'),
                    Some((_, '"')) => out.push('"'),
                    Some((_, '\\')) => out.push('\\'),
                    Some((_, x)) => {
                        out.push('\\');
                        out.push(x);
                    }
                    None => break,
                },
                _ => out.push(c),
            }
        }
        return Err("unclosed \" quote".into());
    }
    // Unquoted: up to an ` #` comment, trailing blanks trimmed.
    let v = match v.find(" #") {
        Some(i) => &v[..i],
        None => v,
    };
    Ok(v.trim_end().to_string())
}

/// After a closing quote only blanks or a `#` comment may follow.
fn tail(rest: &str) -> Result<(), String> {
    let r = rest.trim_start();
    if r.is_empty() || r.starts_with('#') {
        Ok(())
    } else {
        Err(format!("unexpected '{r}' after the quoted value"))
    }
}

/// Read and parse `path`; errors name the file (and line).
pub fn read(path: &Path) -> Result<Vec<(String, String)>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text).map_err(|(n, e)| format!("{}:{n}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kv(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn r125_a_dotenv_quotes_export_comments_and_bad_line() {
        let text = "# token\nexport A=1\n\nB='x $y'\nC=\"a\\nb\"\nD=plain # note\nE=\n";
        assert_eq!(
            parse(text).unwrap(),
            kv(&[
                ("A", "1"),
                ("B", "x $y"),
                ("C", "a\nb"),
                ("D", "plain"),
                ("E", "")
            ])
        );
        assert_eq!(parse("oops\n").unwrap_err().0, 1);
        assert_eq!(parse("A=1\n1X=2\n").unwrap_err().0, 2);
        assert!(parse("A='open\n").is_err());
        assert!(parse("A=\"x\" y\n").is_err());
    }

    #[test]
    fn later_line_wins_in_place() {
        assert_eq!(
            parse("A=1\nB=2\nA=3\n").unwrap(),
            kv(&[("A", "3"), ("B", "2")])
        );
    }

    #[test]
    fn read_names_file_and_line() {
        let d = std::env::temp_dir().join(format!("rade-dotenv-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("x.env");
        std::fs::write(&f, "A=1\nnope\n").unwrap();
        let e = read(&f).unwrap_err();
        assert!(e.contains("x.env:2"), "{e}");
        assert!(read(&d.join("missing.env"))
            .unwrap_err()
            .contains("missing.env"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn keys() {
        assert!(valid_key("CLAUDE_CODE_OAUTH_TOKEN"));
        assert!(valid_key("_a1"));
        assert!(!valid_key("1A"));
        assert!(!valid_key("A-B"));
        assert!(!valid_key(""));
    }
}
