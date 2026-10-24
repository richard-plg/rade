//! File locations (XDG) for state, config, and the socket.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Which RADE this process is (M10): the installed app, or a build run from a
/// Cargo target dir (`target/release/rade-app`, tests, `cargo run`). The
/// Devel flavor has its own app id, socket and state dir, so development
/// never touches the daily app. Config and the paste cache are shared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    Installed,
    Devel,
}

impl Flavor {
    /// Devel iff the executable lies inside a Cargo target dir: its root has
    /// `CACHEDIR.TAG` (written only when Cargo creates it), each profile dir
    /// `.fingerprint`. `cargo install` and the .deb copy the
    /// binary out of it, so they are Installed.
    pub fn of_exe(exe: &Path) -> Flavor {
        if exe
            .ancestors()
            .skip(1)
            .any(|d| d.join("CACHEDIR.TAG").is_file() || d.join(".fingerprint").is_dir())
        {
            Flavor::Devel
        } else {
            Flavor::Installed
        }
    }

    /// This process's flavor (from `current_exe`, resolved once).
    pub fn current() -> Flavor {
        static F: OnceLock<Flavor> = OnceLock::new();
        *F.get_or_init(|| {
            std::env::current_exe()
                .map(|e| Flavor::of_exe(&e))
                .unwrap_or(Flavor::Installed)
        })
    }

    /// Name of the data dir and the socket file stem.
    pub fn name(self) -> &'static str {
        match self {
            Flavor::Installed => "rade",
            Flavor::Devel => "rade-dev",
        }
    }

    /// GApplication id; the installed one matches `dev.rade.App.desktop`.
    pub fn app_id(self) -> &'static str {
        match self {
            Flavor::Installed => "dev.rade.App",
            Flavor::Devel => "dev.rade.App.Devel",
        }
    }

    /// Window title / application name.
    pub fn title(self) -> &'static str {
        match self {
            Flavor::Installed => "RADE",
            Flavor::Devel => "RADE (dev)",
        }
    }
}

/// Lookup for environment variables (injectable for tests).
pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

pub fn process_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn home(env: Env) -> PathBuf {
    env("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn home_dir(env: Env) -> PathBuf {
    home(env)
}

/// `$XDG_DATA_HOME/rade` or `~/.local/share/rade` (`rade-dev` for Devel).
pub fn data_dir(env: Env) -> PathBuf {
    data_dir_of(env, Flavor::current())
}

fn data_dir_of(env: Env, flavor: Flavor) -> PathBuf {
    env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home(env).join(".local/share"))
        .join(flavor.name())
}

/// `$XDG_CACHE_HOME/rade/paste` or `~/.cache/rade/paste`: pasted images (R5).
pub fn paste_dir(env: Env) -> PathBuf {
    env("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home(env).join(".cache"))
        .join("rade/paste")
}

/// Browser Pane website data (cookies, local storage) and HTTP cache
/// (R18): `$XDG_DATA_HOME/<flavor>/browser` and
/// `$XDG_CACHE_HOME/<flavor>/browser`, per flavor like the state, so a dev
/// build never shares a login with the daily app (ADR-0010, ADR-0011).
pub fn browser_dirs(env: Env) -> (PathBuf, PathBuf) {
    browser_dirs_of(env, Flavor::current())
}

fn browser_dirs_of(env: Env, flavor: Flavor) -> (PathBuf, PathBuf) {
    let cache = env("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home(env).join(".cache"))
        .join(flavor.name())
        .join("browser");
    (data_dir_of(env, flavor).join("browser"), cache)
}

pub fn state_file(env: Env) -> PathBuf {
    state_file_of(env, Flavor::current())
}

fn state_file_of(env: Env, flavor: Flavor) -> PathBuf {
    data_dir_of(env, flavor).join("state.json")
}

/// `$XDG_CONFIG_HOME/rade/config.toml` or `~/.config/rade/config.toml`.
pub fn config_file(env: Env) -> PathBuf {
    env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home(env).join(".config"))
        .join("rade/config.toml")
}

/// `$RADE_SOCKET`, else `$XDG_RUNTIME_DIR/rade/rade.sock` (API.md
/// "Transport"; `rade-dev.sock` for Devel).
pub fn socket_path(env: Env) -> PathBuf {
    socket_path_of(env, Flavor::current())
}

fn socket_path_of(env: Env, flavor: Flavor) -> PathBuf {
    if let Some(s) = env("RADE_SOCKET") {
        return s.into();
    }
    let runtime = env("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // No runtime dir: fall back to a per-user dir under /tmp.
            std::env::temp_dir().join(format!("rade-{}", env("USER").unwrap_or_default()))
        });
    runtime.join(format!("rade/{}.sock", flavor.name()))
}

/// A temp root outside any git work tree or Cargo target dir, for tests
/// whose result depends on what encloses a folder (`TMPDIR` may be
/// `<repo>/target/tmp`).
#[cfg(test)]
pub(crate) fn outside_temp_dir() -> PathBuf {
    let t = std::env::temp_dir();
    let enclosed = t
        .ancestors()
        .any(|a| a.join(".git").exists() || a.join("CACHEDIR.TAG").exists());
    if enclosed {
        PathBuf::from("/tmp")
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |k| {
            pairs
                .iter()
                .find(|(a, _)| *a == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn xdg_overrides_home() {
        let e = env(&[
            ("HOME", "/h"),
            ("XDG_DATA_HOME", "/d"),
            ("XDG_CONFIG_HOME", "/c"),
        ]);
        assert_eq!(
            state_file_of(&e, Flavor::Installed),
            PathBuf::from("/d/rade/state.json")
        );
        assert_eq!(config_file(&e), PathBuf::from("/c/rade/config.toml"));
        assert_eq!(
            paste_dir(&env(&[("XDG_CACHE_HOME", "/k")])),
            PathBuf::from("/k/rade/paste")
        );
        assert_eq!(
            paste_dir(&env(&[("HOME", "/h")])),
            PathBuf::from("/h/.cache/rade/paste")
        );
    }

    #[test]
    fn home_fallbacks() {
        let e = env(&[("HOME", "/h")]);
        assert_eq!(
            state_file_of(&e, Flavor::Installed),
            PathBuf::from("/h/.local/share/rade/state.json")
        );
        assert_eq!(
            state_file_of(&e, Flavor::Devel),
            PathBuf::from("/h/.local/share/rade-dev/state.json")
        );
        assert_eq!(
            config_file(&e),
            PathBuf::from("/h/.config/rade/config.toml")
        );
    }

    #[test]
    fn r18_browser_data_is_per_flavor() {
        let e = env(&[("HOME", "/h"), ("XDG_CACHE_HOME", "/k")]);
        assert_eq!(
            browser_dirs_of(&e, Flavor::Devel),
            (
                "/h/.local/share/rade-dev/browser".into(),
                "/k/rade-dev/browser".into()
            )
        );
        assert_eq!(
            browser_dirs_of(&e, Flavor::Installed),
            (
                "/h/.local/share/rade/browser".into(),
                "/k/rade/browser".into()
            )
        );
    }

    #[test]
    fn socket_env_then_runtime_dir() {
        let both = env(&[("RADE_SOCKET", "/s"), ("XDG_RUNTIME_DIR", "/r")]);
        assert_eq!(socket_path_of(&both, Flavor::Devel), PathBuf::from("/s"));
        let r = env(&[("XDG_RUNTIME_DIR", "/r")]);
        assert_eq!(
            socket_path_of(&r, Flavor::Installed),
            PathBuf::from("/r/rade/rade.sock")
        );
        assert_eq!(
            socket_path_of(&r, Flavor::Devel),
            PathBuf::from("/r/rade/rade-dev.sock")
        );
    }

    #[test]
    fn m10_flavor_is_devel_only_inside_a_cargo_target_dir() {
        let d = super::outside_temp_dir().join(format!("rade-flavor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("repo/target/release")).unwrap();
        std::fs::create_dir_all(d.join("usr/lib/rade")).unwrap();
        std::fs::write(d.join("repo/target/CACHEDIR.TAG"), "Signature: x").unwrap();
        let dev = d.join("repo/target/release/rade-app");
        assert_eq!(Flavor::of_exe(&dev), Flavor::Devel);
        // A target dir Cargo didn't create (no tag) is still recognised.
        std::fs::create_dir_all(d.join("t2/release/.fingerprint")).unwrap();
        assert_eq!(
            Flavor::of_exe(&d.join("t2/release/rade-app")),
            Flavor::Devel
        );
        assert_eq!(
            Flavor::of_exe(&d.join("usr/lib/rade/rade-app")),
            Flavor::Installed
        );
        // Test binaries (target/*/deps) are Devel too.
        assert_eq!(Flavor::current(), Flavor::Devel);
        assert_eq!(Flavor::Devel.app_id(), "dev.rade.App.Devel");
        assert_eq!(Flavor::Installed.app_id(), "dev.rade.App");
        let _ = std::fs::remove_dir_all(&d);
    }
}
