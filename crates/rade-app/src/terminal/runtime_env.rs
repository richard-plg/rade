// Ported from limux (MIT), https://github.com/am-will/limux @ ae67aae
// (rust/limux-host-linux/src/main.rs: prepare_ghostty_runtime and helpers)

use std::path::{Path, PathBuf};

/// Append a value to an environment variable (comma-separated), or set it.
fn append_env(key: &str, value: &str) {
    match std::env::var(key) {
        Ok(existing) if !existing.is_empty() => {
            std::env::set_var(key, format!("{existing},{value}"));
        }
        _ => {
            std::env::set_var(key, value);
        }
    }
}

fn has_ghostty_terminfo(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };

    ["terminfo/g/ghostty", "terminfo/x/xterm-ghostty"]
        .iter()
        .any(|entry| parent.join(entry).is_file())
}

fn is_ghostty_resources_dir(path: &Path) -> bool {
    path.is_dir()
        && ["themes", "shell-integration"]
            .iter()
            .all(|entry| path.join(entry).is_dir())
        && has_ghostty_terminfo(path)
}

fn ghostty_resources_candidates(exe_dir: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    for ancestor in exe_dir.ancestors() {
        candidates.push(ancestor.join("share/rade/ghostty"));
        candidates.push(ancestor.join("share/ghostty"));
        candidates.push(ancestor.join("ghostty/zig-out/share/ghostty"));
    }

    // RADE: a `cargo install`ed binary lives outside the tree; prefer the
    // submodule resources it was built from (same Ghostty version).
    candidates
        .push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ghostty/zig-out/share/ghostty"));
    candidates.push(PathBuf::from("/usr/local/share/ghostty"));
    candidates.push(PathBuf::from("/usr/share/ghostty"));

    candidates
}

fn resolve_ghostty_resources_dir(exe_path: &Path) -> Option<PathBuf> {
    let exe_dir = exe_path.parent()?;
    ghostty_resources_candidates(exe_dir)
        .into_iter()
        .find(|path| is_ghostty_resources_dir(path))
}

fn ghostty_terminfo_dir(resources_dir: &Path) -> Option<PathBuf> {
    resources_dir.parent().map(|parent| parent.join("terminfo"))
}

fn set_env_path_if_missing_or_invalid(
    key: &str,
    path: Option<PathBuf>,
    validator: impl Fn(&Path) -> bool,
) {
    let has_valid_existing = std::env::var_os(key)
        .map(PathBuf::from)
        .is_some_and(|existing| validator(&existing));

    if has_valid_existing {
        return;
    }

    if let Some(path) = path.filter(|candidate| validator(candidate)) {
        std::env::set_var(key, path);
    }
}

fn set_ghostty_runtime_env() {
    let Some(exe_path) = std::env::current_exe().ok() else {
        return;
    };
    let Some(resources_dir) = resolve_ghostty_resources_dir(&exe_path) else {
        return;
    };

    set_env_path_if_missing_or_invalid(
        "GHOSTTY_RESOURCES_DIR",
        Some(resources_dir.clone()),
        is_ghostty_resources_dir,
    );
    set_env_path_if_missing_or_invalid(
        "TERMINFO",
        ghostty_terminfo_dir(&resources_dir),
        has_ghostty_terminfo,
    );
    set_env_path_if_missing_or_invalid(
        "GHOSTTY_SHELL_INTEGRATION_XDG_DIR",
        Some(resources_dir.join("shell-integration")),
        |candidate| candidate.is_dir(),
    );
}

fn gtk_runtime_at_least(major: u32, minor: u32, micro: u32) -> bool {
    let version = unsafe {
        (
            gtk4::ffi::gtk_get_major_version(),
            gtk4::ffi::gtk_get_minor_version(),
            gtk4::ffi::gtk_get_micro_version(),
        )
    };
    version >= (major, minor, micro)
}

/// Configure the embedded renderer before GTK initializes.
pub fn prepare_ghostty_runtime() {
    rade_ghostty_sys::ensure_glad_symbols_linked();

    // Ghostty requires desktop OpenGL, not GLES, and the exact GTK knobs
    // differ by GTK runtime version (matches Ghostty's GTK apprt).
    if gtk_runtime_at_least(4, 16, 0) {
        append_env("GDK_DISABLE", "gles-api,vulkan");
    } else if gtk_runtime_at_least(4, 14, 0) {
        append_env("GDK_DEBUG", "gl-disable-gles,vulkan-disable");
    } else {
        append_env("GDK_DEBUG", "vulkan-disable");
    }

    // Embedded Ghostty needs a resources directory for themes, terminfo and
    // shell integration.
    set_ghostty_runtime_env();

    // NO_COLOR belongs to the launching process, not to shells inside RADE.
    std::env::remove_var("NO_COLOR");
}
