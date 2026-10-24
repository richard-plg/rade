// Ported from limux (MIT), https://github.com/am-will/limux @ ae67aae
//
// libghostty-internal.so resolves GLAD's loader from the host executable, so
// compile Ghostty's vendored GLAD and link it into the binary.
use std::{env, path::PathBuf};

#[path = "src/tokens_gen.rs"]
mod tokens_gen;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ghostty_root = manifest_dir.join("../../ghostty");
    let glad_src = ghostty_root.join("vendor/glad/src/gl.c");
    let glad_include = ghostty_root.join("vendor/glad/include");

    cc::Build::new()
        .file(&glad_src)
        .include(&glad_include)
        .cargo_metadata(false)
        .compile("glad");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR must be set"));
    let glad_archive = out_dir.join("libglad.a");
    println!(
        "cargo:rustc-link-arg-bin=rade-app={}",
        glad_archive.display()
    );

    // Find libghostty-internal.so at run time without LD_LIBRARY_PATH: dev
    // builds use the submodule's output; scripts/package-deb.sh sets
    // RADE_LIBGHOSTTY_RPATH='$ORIGIN' (the lib sits next to the binary).
    let rpath = match env::var("RADE_LIBGHOSTTY_RPATH") {
        Ok(r) if !r.is_empty() => r,
        _ => ghostty_root
            .join("zig-out/lib")
            .canonicalize()
            .expect("ghostty/zig-out/lib")
            .display()
            .to_string(),
    };
    println!("cargo:rustc-link-arg=-Wl,-rpath,{rpath}");
    println!("cargo:rerun-if-env-changed=RADE_LIBGHOSTTY_RPATH");

    file_icons(&manifest_dir, &out_dir);
    // RADE's symbolic icons (status glyphs, R20.2), found by name through
    // the icon theme (`/rade/icons` is added to its resource path).
    gresource(
        &manifest_dir.join("assets/icons"),
        "/rade/icons/scalable/actions",
        "rade-icons",
        &out_dir,
    );

    // Inter + JetBrains Mono (R102.3): written to a file at startup, Pango
    // cannot read fonts from a GResource (fonts.rs).
    gresource(
        &manifest_dir.join("assets/fonts"),
        "/rade/fonts",
        "rade-fonts",
        &out_dir,
    );
    tokens(&manifest_dir, &out_dir);

    println!("cargo:rerun-if-changed={}", glad_src.display());
    println!("cargo:rerun-if-changed={}", glad_include.display());
}

/// The file tree's and palette's icons (R8, ADR-0037): Catppuccin's SVGs
/// (`assets/file-icons`, `currentColor` everywhere) painted per theme from
/// `tokens.toml` — files `fg2`, folders `fg3`, an open folder `acc` — as
/// `/rade/file-icons/{dark,light}/<stem>.svg`.
fn file_icons(manifest_dir: &std::path::Path, out_dir: &std::path::Path) {
    let src = manifest_dir.join("assets/file-icons/icons");
    println!("cargo:rerun-if-changed={}", src.display());
    let text = std::fs::read_to_string(manifest_dir.join("src/tokens.toml")).expect("tokens.toml");
    let t = tokens_gen::parse(&text).unwrap_or_else(|e| panic!("{e}"));
    for (theme, list) in [("dark", &t.dark), ("light", &t.light)] {
        let get = |name: &str| {
            let v = &list.iter().find(|(k, _)| k == name).expect("token").1;
            v[..7].to_string()
        };
        let dir = out_dir.join(format!("file-icons-{theme}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for e in std::fs::read_dir(&src).expect("assets/file-icons/icons") {
            let path = e.unwrap().path();
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            let color = if stem.ends_with("_open") {
                get("acc")
            } else if stem.starts_with("_folder") || stem.starts_with("folder_") {
                get("fg3")
            } else {
                get("fg2")
            };
            let svg = std::fs::read_to_string(&path)
                .unwrap()
                .replace("currentColor", &color);
            std::fs::write(dir.join(path.file_name().unwrap()), svg).unwrap();
        }
        gresource(
            &dir,
            &format!("/rade/file-icons/{theme}"),
            &format!("file-icons-{theme}"),
            out_dir,
        );
    }
}

/// `dir`'s files as `<name>.gresource` under `prefix`.
fn gresource(dir: &std::path::Path, prefix: &str, name: &str, out_dir: &std::path::Path) {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("assets folder")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let files: String = names.iter().map(|n| format!("<file>{n}</file>")).collect();
    let xml = out_dir.join(format!("{name}.gresource.xml"));
    std::fs::write(
        &xml,
        format!("<gresources><gresource prefix=\"{prefix}\">{files}</gresource></gresources>"),
    )
    .unwrap();
    let ok = std::process::Command::new("glib-compile-resources")
        .arg(format!("--sourcedir={}", dir.display()))
        .arg(format!(
            "--target={}",
            out_dir.join(format!("{name}.gresource")).display()
        ))
        .arg(&xml)
        .status()
        .expect("glib-compile-resources (libglib2.0-dev-bin)")
        .success();
    assert!(ok, "glib-compile-resources failed");
    println!("cargo:rerun-if-changed={}", dir.display());
}

/// Design tokens (R102.1, ADR-0036 §3): `src/tokens.toml` → `$OUT_DIR/tokens.css`
/// (GTK), `$OUT_DIR/tokens.rs` (`crate::tokens`) and the mockup's
/// `docs/design/redesign-2026-10-23/tokens.css` (rewritten only when it differs).
fn tokens(manifest_dir: &std::path::Path, out_dir: &std::path::Path) {
    let src = manifest_dir.join("src/tokens.toml");
    println!("cargo:rerun-if-changed={}", src.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("src/tokens_gen.rs").display()
    );
    let text = std::fs::read_to_string(&src).expect("src/tokens.toml");
    let t = tokens_gen::parse(&text).unwrap_or_else(|e| panic!("{e}"));
    std::fs::write(out_dir.join("tokens.css"), tokens_gen::gtk_css(&t)).unwrap();
    std::fs::write(out_dir.join("tokens.rs"), tokens_gen::rust_table(&t)).unwrap();
    let mockup = manifest_dir.join("../../docs/design/redesign-2026-10-23/tokens.css");
    let css = tokens_gen::mockup_css(&t);
    if mockup.parent().is_some_and(|d| d.is_dir())
        && std::fs::read_to_string(&mockup).ok().as_deref() != Some(css.as_str())
    {
        // A read-only source tree (a packaging build) keeps the committed copy;
        // the R102-a unit test catches a stale one.
        let _ = std::fs::write(&mockup, css);
    }
}
