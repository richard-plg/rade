//! Design-system checks (PRD R102): tokens → CSS, no raw hex colors, icons,
//! bundled fonts and their licences. They run in `cargo test`, which is the
//! CI check stage.
use crate::tokens_gen::{self, Tokens};
use std::path::{Path, PathBuf};

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn repo() -> PathBuf {
    manifest().join("../..")
}
fn tokens() -> Tokens {
    tokens_gen::parse(include_str!("tokens.toml")).expect("tokens.toml parses")
}
fn names(l: &[(String, String)]) -> Vec<String> {
    l.iter().map(|(k, _)| k.clone()).collect()
}

/// Token names of one table of the spec README ("### Dark (default)" or
/// "### Light"): the backticked first cell of each row.
fn readme_table(title: &str) -> Vec<String> {
    let readme =
        std::fs::read_to_string(repo().join("docs/internals/design/spec/README.md")).unwrap();
    let part = readme.split_once(title).expect("table heading").1;
    let mut out = vec![];
    for line in part.lines().skip(1) {
        if line.starts_with("### ") || line.starts_with("Token notes") {
            break;
        }
        if let Some(rest) = line.strip_prefix("| `") {
            out.push(rest.split('`').next().unwrap().to_string());
        }
    }
    out
}

#[test]
fn r102_a_generated_css_has_one_define_color_per_token_per_theme() {
    let t = tokens();
    // The CSS the app embeds is the generator's output for tokens.toml.
    assert_eq!(crate::tokens::CSS, tokens_gen::gtk_css(&t));
    let (light, dark) = crate::tokens::css_halves();
    for (half, list) in [(light, &t.light), (dark, &t.dark)] {
        let defs: Vec<&str> = half
            .lines()
            .filter_map(|l| l.strip_prefix("@define-color "))
            .collect();
        assert_eq!(defs.len(), list.len(), "one @define-color per token");
        for (k, v) in list.iter() {
            assert!(defs.contains(&format!("{k} {v};").as_str()), "{k} {v}");
        }
    }
    assert!(!t.dark.is_empty());
}

#[test]
fn r102_a_token_names_equal_the_spec_readme_tables() {
    let t = tokens();
    for (title, list) in [("### Dark (default)", &t.dark), ("### Light", &t.light)] {
        let spec = readme_table(title);
        let ours = names(list);
        let missing: Vec<_> = spec.iter().filter(|n| !ours.contains(n)).collect();
        let extra: Vec<_> = ours.iter().filter(|n| !spec.contains(n)).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "{title}: missing {missing:?}, extra {extra:?}"
        );
        assert_eq!(spec.len(), ours.len(), "{title}: same count");
    }
}

#[test]
fn r102_a_mockup_tokens_css_has_the_same_values() {
    let t = tokens();
    let path = repo().join("docs/internals/design/redesign-2026-10-23/tokens.css");
    let css = std::fs::read_to_string(&path).expect("mockup tokens.css (written by build.rs)");
    assert_eq!(css, tokens_gen::mockup_css(&t), "stale {}", path.display());
    let block = |sel: &str| {
        css.split_once(&format!("{sel} {{"))
            .unwrap()
            .1
            .split('}')
            .next()
            .unwrap()
            .to_string()
    };
    for (sel, list) in [(":root", &t.dark), ("body.light", &t.light)] {
        let b = block(sel);
        for (k, v) in list.iter() {
            let line = b
                .lines()
                .find(|l| l.trim_start().starts_with(&format!("--{k}:")))
                .unwrap_or_else(|| panic!("{sel} --{k}"));
            assert!(line.contains(v.as_str()), "{sel} --{k} has {v}: {line}");
        }
    }
    // The mockup reads it after its own inline tokens.
    let html = std::fs::read_to_string(
        repo().join("docs/internals/design/redesign-2026-10-23/mockup.html"),
    )
    .unwrap();
    let link = html
        .find("<link rel=\"stylesheet\" href=\"tokens.css\">")
        .expect("mockup links tokens.css");
    assert!(link > html.find("</style>").unwrap());
}

/// `#` + `hex`: fixtures spell colors this way so this file passes its own
/// scan (R102-b).
fn c(hex: &str) -> String {
    format!("#{hex}")
}

#[test]
fn tokens_gen_rejects_bad_files() {
    let (white, w3) = (c("ffffff"), c("fff"));
    let file = |dark: &str, light: &str| format!("[dark]\n{dark}\n[light]\n{light}\n");
    let err = tokens_gen::parse(&file(
        &format!("fg = \"{w3}\""),
        &format!("fg = \"{white}\""),
    ))
    .unwrap_err();
    assert!(err.contains(":2:"), "{err}");
    assert!(tokens_gen::parse(&file(
        &format!("fg = \"{white}\""),
        &format!("bg = \"{white}\"")
    ))
    .is_err());
    assert!(tokens_gen::parse(&format!("[dark]\nFg = \"{white}\"\n")).is_err());
    let ok = tokens_gen::parse(&format!(
        "[dark]\na = \"{}\"\n[contrast]\nx = [\"a on b\"]\n[light]\na = \"{}\"\n",
        c("00000080"),
        c("FFFFFF")
    ))
    .unwrap();
    assert_eq!(ok.light, vec![("a".into(), white)]);
}

/// Hex colors (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`) in `text`, as
/// `<name>:<line>: <color>`.
fn raw_hex(name: &str, text: &str) -> Vec<String> {
    let mut out = vec![];
    for (i, line) in text.lines().enumerate() {
        let b = line.as_bytes();
        for (j, _) in line.match_indices('#') {
            let run = b[j + 1..]
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .count();
            let next = b.get(j + 1 + run).copied();
            let word_end = !next.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_');
            if word_end && matches!(run, 3 | 4 | 6 | 8) {
                out.push(format!("{name}:{}: {}", i + 1, &line[j..j + 1 + run]));
            }
        }
    }
    out
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs" || x == "css") {
            out.push(p);
        }
    }
}

#[test]
fn r102_b_no_raw_hex_colors_in_rade_app_sources() {
    let src = manifest().join("src");
    let mut files = vec![];
    sources(&src, &mut files);
    assert!(files.len() > 40, "scanned {} files", files.len());
    let mut found = vec![];
    for f in &files {
        let rel = f.strip_prefix(manifest()).unwrap().display().to_string();
        found.extend(raw_hex(&rel, &std::fs::read_to_string(f).unwrap()));
    }
    assert!(
        found.is_empty(),
        "raw hex colors outside tokens.toml:\n{}",
        found.join("\n")
    );
}

#[test]
fn r102_b_a_hex_color_in_style_css_fails_naming_the_line() {
    let fixture = format!(
        ".rade-x {{\n  min-height: 2px;\n  color: {};\n}}\n",
        c("4fe3c8")
    );
    assert_eq!(
        raw_hex("style.css", &fixture),
        vec![format!("style.css:3: {}", c("4fe3c8"))]
    );
    // Every length, but not Rust attributes, long numbers or #identifiers.
    let text = [c("abc"), c("abcd"), c("aabbcc"), c("aabbcc80")].join(" ")
        + &format!(
            " #[derive(Debug)] #![allow] issue {} {} {}",
            c("1234567"),
            c("abcdefg"),
            c("fff_x")
        );
    let want: Vec<String> = ["abc", "abcd", "aabbcc", "aabbcc80"]
        .iter()
        .map(|h| format!("a.rs:1: {}", c(h)))
        .collect();
    assert_eq!(raw_hex("a.rs", &text), want);
}

/// Icon names used in code: `"ph-…"` string literals.
fn icon_names_in_code() -> Vec<String> {
    let mut files = vec![];
    sources(&manifest().join("src"), &mut files);
    let mut out = vec![];
    for f in files
        .iter()
        .filter(|f| f.extension().is_some_and(|x| x == "rs"))
    {
        let text = std::fs::read_to_string(f).unwrap();
        for (i, _) in text.match_indices("\"ph-") {
            let name: String = text[i + 1..]
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                .collect();
            if name.len() > 3 && text[i + 1 + name.len()..].starts_with('"') {
                out.push(name);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Icons of the spec README "## Icons" table as bundled names
/// (`ph-<name>[-fill|-bold]-symbolic`).
fn icon_names_in_spec() -> Vec<String> {
    let readme =
        std::fs::read_to_string(repo().join("docs/internals/design/spec/README.md")).unwrap();
    let table = readme.split_once("## Icons").unwrap().1;
    let t = tokens();
    let colors = names(&t.dark);
    let mut out = vec![];
    for line in table
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Concept"))
    {
        let cells: Vec<&str> = line.split('|').collect();
        for cell in [cells.get(2), cells.get(4)].into_iter().flatten() {
            let weight = if cell.contains("(Fill") {
                "-fill"
            } else if cell.contains("(Bold") {
                "-bold"
            } else {
                ""
            };
            for (k, part) in cell.split('`').enumerate() {
                if k % 2 == 1 && !colors.iter().any(|c| c == part) && part != "proj-n" {
                    out.push(format!("ph-{part}{weight}-symbolic"));
                }
            }
        }
    }
    out
}

fn bundled(name: &str) -> bool {
    let base = name.strip_suffix("-symbolic").unwrap_or(name);
    manifest()
        .join(format!("assets/icons/{base}-symbolic.svg"))
        .is_file()
}

#[test]
fn r102_e_every_ph_icon_name_used_in_code_is_bundled() {
    let missing: Vec<_> = icon_names_in_code()
        .into_iter()
        .filter(|n| !bundled(n))
        .collect();
    assert!(missing.is_empty(), "not in assets/icons: {missing:?}");
    let spec = icon_names_in_spec();
    assert!(spec.len() >= 40, "spec icons: {spec:?}");
    let missing: Vec<_> = spec.iter().filter(|n| !bundled(n)).collect();
    assert!(missing.is_empty(), "spec icons not bundled: {missing:?}");
}

#[test]
fn r102_e_bundled_phosphor_svgs_are_filled_with_current_color() {
    let mut n = 0;
    for e in std::fs::read_dir(manifest().join("assets/icons")).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if !name.starts_with("ph-") {
            continue;
        }
        assert!(name.ends_with("-symbolic.svg"), "{name}");
        let svg = std::fs::read_to_string(&p).unwrap();
        assert!(
            svg.contains("fill=\"currentColor\""),
            "{name}: fill=\"currentColor\""
        );
        assert!(!svg.contains("stroke"), "{name}: no stroke");
        n += 1;
    }
    assert!(n >= 40, "{n} Phosphor icons");
}

#[test]
fn r102_h_licences_listed_and_fonts_in_the_gresource_manifest() {
    let tp = std::fs::read_to_string(repo().join("THIRD_PARTY.md")).unwrap();
    for (name, licence) in [
        ("Inter", "OFL-1.1"),
        ("JetBrains Mono", "OFL-1.1"),
        ("Maple Mono", "OFL-1.1"),
        ("Phosphor Icons", "MIT"),
    ] {
        assert!(
            tp.lines().any(|l| l == format!("## {name} — {licence}")),
            "THIRD_PARTY.md: '## {name} — {licence}'"
        );
    }
    let manifest_xml =
        std::fs::read_to_string(concat!(env!("OUT_DIR"), "/rade-fonts.gresource.xml")).unwrap();
    for f in crate::fonts::FILES {
        assert!(
            manifest_xml.contains(&format!("<file>{f}</file>")),
            "{f} in the fonts GResource"
        );
    }
    assert!(crate::fonts::FILES.iter().any(|f| f.starts_with("Inter-")));
    assert!(crate::fonts::FILES
        .iter()
        .any(|f| f.starts_with("JetBrainsMono")));
    assert!(crate::fonts::FILES.contains(&"MapleMono-Medium.ttf"));
}

#[test]
fn r115_every_agent_glyph_is_bundled_and_recolourable() {
    use rade_core::design::AgentMark::*;
    for m in [Claude, Codex, Antigravity, OpenCode, Other] {
        let p = manifest().join(format!("assets/icons/{}.svg", m.icon()));
        let svg = std::fs::read_to_string(&p).unwrap_or_else(|_| panic!("{p:?}"));
        assert!(
            svg.contains("fill=\"currentColor\"") && !svg.contains("stroke"),
            "{p:?}"
        );
    }
}
