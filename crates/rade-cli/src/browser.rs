//! `rade browser …` (docs/API.md, PRD R18): open a Browser Pane and drive
//! it — snapshot with refs, click / fill / press by ref, eval, screenshot,
//! console, wait, history — and list dev-server ports.

use clap::{Args, Subcommand};
use rade_core::rpc::Error;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Args)]
pub struct BrowserArgs {
    /// `webkit`: a Browser Pane in RADE (default); `chromium`: a real Chromium over CDP,
    /// headless unless --headed (ADR-0026). Same commands and refs.
    #[arg(long, value_enum, default_value_t = Engine::Webkit, global = true)]
    pub engine: Engine,
    /// Chromium: a real window (with the annotation toolbar) instead of headless.
    #[arg(long, global = true)]
    pub headed: bool,
    #[command(subcommand)]
    cmd: BrowserCmd,
}

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Engine {
    Webkit,
    Chromium,
}

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum OnOff {
    On,
    Off,
}

/// Which Browser Pane (default: the one in your Tab, else your Worktree's).
#[derive(Args)]
struct At {
    #[arg(long)]
    pane: Option<String>,
}

#[derive(Subcommand)]
enum BrowserCmd {
    /// Show a URL (`3000`, `localhost:5173/x`, `https://…`) beside you; waits for the load.
    Open {
        url: String,
        #[command(flatten)]
        at: At,
    },
    /// The page as text: roles, names, values, and [ref=eN] on what you can act on.
    Snapshot {
        /// Only elements with a ref.
        #[arg(short, long)]
        interactive: bool,
        #[command(flatten)]
        at: At,
    },
    /// Click an element by ref (from the last snapshot).
    Click {
        r#ref: String,
        #[command(flatten)]
        at: At,
    },
    /// Set a text field / select / editable element's value by ref.
    Fill {
        r#ref: String,
        text: String,
        #[command(flatten)]
        at: At,
    },
    /// Press a key in the focused element: Enter, Tab, Shift+Tab, Control+a, ArrowDown, x …
    Press {
        key: String,
        #[command(flatten)]
        at: At,
    },
    /// Run JavaScript in the page (an expression, or statements with `return`); prints JSON.
    Eval {
        js: String,
        #[command(flatten)]
        at: At,
    },
    /// Save the visible page (or --full page) as PNG.
    Screenshot {
        file: PathBuf,
        #[arg(long)]
        full: bool,
        #[command(flatten)]
        at: At,
    },
    /// Console messages and JS errors since the last call.
    Console {
        #[command(flatten)]
        at: At,
    },
    /// Block until the page loaded, shows a text, or a ref is visible.
    Wait {
        #[arg(long, conflicts_with_all = ["text", "ref"])]
        load: bool,
        #[arg(long, conflicts_with = "ref")]
        text: Option<String>,
        #[arg(long = "ref")]
        r#ref: Option<String>,
        /// Seconds; exit 6 when exceeded.
        #[arg(long, default_value_t = 30)]
        timeout: u64,
        #[command(flatten)]
        at: At,
    },
    Back {
        #[command(flatten)]
        at: At,
    },
    Forward {
        #[command(flatten)]
        at: At,
    },
    Reload {
        #[command(flatten)]
        at: At,
    },
    /// TCP ports that processes in RADE Panes listen on (dev servers).
    Ports,
    /// The owner's Browser Annotations not sent yet (and the prompt they make).
    Annotations {
        /// Send them to an Agent (then they are cleared): --to, else the Agent beside the Browser.
        #[arg(long)]
        send: bool,
        /// The Agent Pane to send to.
        #[arg(long, requires = "send")]
        to: Option<String>,
        /// No Agent there: spawn this Preset with them as its prompt.
        #[arg(long, requires = "send")]
        agent: Option<String>,
        #[command(flatten)]
        at: At,
    },
    /// Turn annotate mode on or off (the owner clicks or drags, then comments).
    Annotate {
        #[arg(value_enum)]
        state: OnOff,
        #[command(flatten)]
        at: At,
    },
    /// Chromium: quit it (its temp profile is deleted).
    Close,
}

pub fn request(a: &BrowserArgs) -> Result<(&'static str, Value), Error> {
    use BrowserCmd as B;
    let pane = |at: &At| json!(at.pane);
    Ok(match &a.cmd {
        B::Open { url, at } => ("browser.open", json!({"url": url, "pane": pane(at)})),
        B::Snapshot { interactive, at } => (
            "browser.snapshot",
            json!({"interactive": interactive, "pane": pane(at)}),
        ),
        B::Click { r#ref, at } => ("browser.click", json!({"ref": r#ref, "pane": pane(at)})),
        B::Fill { r#ref, text, at } => (
            "browser.fill",
            json!({"ref": r#ref, "text": text, "pane": pane(at)}),
        ),
        B::Press { key, at } => ("browser.press", json!({"key": key, "pane": pane(at)})),
        B::Eval { js, at } => ("browser.eval", json!({"js": js, "pane": pane(at)})),
        B::Screenshot { file, full, at } => (
            "browser.screenshot",
            json!({"path": crate::abs(file.clone())?, "full": full, "pane": pane(at)}),
        ),
        B::Console { at } => ("browser.console", json!({"pane": pane(at)})),
        B::Wait {
            load,
            text,
            r#ref,
            timeout,
            at,
        } => {
            let what = match (load, text, r#ref) {
                (_, Some(t), _) => json!({"text": t}),
                (_, _, Some(r)) => json!({"ref": r}),
                (true, ..) => json!("load"),
                _ => return Err(Error::usage("wait needs --load, --text <s> or --ref <r>")),
            };
            (
                "browser.wait",
                json!({"for": what, "timeout": timeout, "pane": pane(at)}),
            )
        }
        B::Back { at } => ("browser.back", json!({"pane": pane(at)})),
        B::Forward { at } => ("browser.forward", json!({"pane": pane(at)})),
        B::Reload { at } => ("browser.reload", json!({"pane": pane(at)})),
        B::Ports => ("browser.ports", json!({})),
        B::Annotations {
            send,
            to,
            agent,
            at,
        } => (
            "browser.annotations",
            json!({"send": send, "to": to, "agent": agent, "pane": pane(at)}),
        ),
        B::Annotate { state, at } => (
            "browser.annotate",
            json!({"on": *state == OnOff::On, "pane": pane(at)}),
        ),
        B::Close if a.engine == Engine::Webkit => {
            return Err(Error::usage(
                "close is for --engine chromium; a Browser Pane closes with `rade close <pane>`",
            ))
        }
        B::Close => ("browser.close", json!({})),
    })
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("-")
}

pub fn print(a: &BrowserArgs, r: &Value) {
    use BrowserCmd as B;
    match &a.cmd {
        B::Snapshot { .. } => print!("{}", s(&r["text"])),
        B::Eval { .. } => match &r["result"] {
            Value::String(t) => println!("{t}"),
            v => println!("{v}"),
        },
        B::Console { .. } => {
            for m in r["messages"].as_array().into_iter().flatten() {
                let at = match (m["source"].as_str(), m["line"].as_u64()) {
                    (Some(src), Some(l)) if l > 0 => format!("  ({src}:{l})"),
                    _ => String::new(),
                };
                // First line with its source; a stack indented below it.
                let text = s(&m["text"]);
                let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
                println!("[{}] {first}{at}", s(&m["level"]));
                for l in rest.lines().filter(|l| !l.trim().is_empty()) {
                    println!("    {l}");
                }
            }
            if r["dropped"].as_u64().unwrap_or(0) > 0 {
                println!("… {} older messages dropped", r["dropped"]);
            }
        }
        B::Ports => {
            for p in r["ports"].as_array().into_iter().flatten() {
                println!(
                    "{}\t{}\t{}\t{}/{}",
                    p["port"],
                    s(&p["pane"]),
                    s(&p["name"]),
                    s(&p["source"]),
                    s(&p["worktree"])
                );
            }
        }
        B::Screenshot { .. } => println!("{}", s(&r["path"])),
        B::Annotations { send: true, .. } => println!(
            "sent {} annotation(s) to {}",
            r["annotations"],
            s(&r["pane"])
        ),
        B::Annotations { .. } => match r["prompt"].as_str() {
            Some(p) => println!("{p}"),
            None => println!("no pending annotations"),
        },
        B::Annotate { .. } => println!("annotate {}", if r["on"] == true { "on" } else { "off" }),
        B::Close => {}
        B::Wait { .. } => {}
        B::Click { .. } | B::Fill { .. } | B::Press { .. } => {
            if r["navigated"] == true {
                println!("→ {}", s(&r["url"]));
            }
        }
        B::Open { .. } | B::Back { .. } | B::Forward { .. } | B::Reload { .. } => {
            println!("{}\t{}\t{}", s(&r["pane"]), s(&r["url"]), s(&r["title"]));
            if let Some(e) = r["error"].as_str() {
                eprintln!("rade: load failed: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        cmd: Top,
    }
    #[derive(clap::Subcommand)]
    enum Top {
        Browser(BrowserArgs),
    }

    fn req(args: &[&str]) -> Result<(&'static str, Value), Error> {
        let cli = Cli::try_parse_from([&["rade", "browser"], args].concat())
            .map_err(|e| Error::usage(e.to_string()))?;
        let Top::Browser(a) = cli.cmd;
        request(&a)
    }

    #[test]
    fn r18_browser_commands_parse() {
        let (m, p) = req(&["open", "3000", "--pane", "web"]).unwrap();
        assert_eq!(
            (m, p["url"].as_str(), p["pane"].as_str()),
            ("browser.open", Some("3000"), Some("web"))
        );
        let (m, p) = req(&["snapshot", "-i"]).unwrap();
        assert_eq!(
            (m, p["interactive"].as_bool()),
            ("browser.snapshot", Some(true))
        );
        assert_eq!(p["pane"], Value::Null);
        let (m, p) = req(&["click", "e12"]).unwrap();
        assert_eq!((m, p["ref"].as_str()), ("browser.click", Some("e12")));
        let (m, p) = req(&["fill", "e3", "a@b.c"]).unwrap();
        assert_eq!((m, p["text"].as_str()), ("browser.fill", Some("a@b.c")));
        let (m, p) = req(&["press", "Shift+Tab"]).unwrap();
        assert_eq!((m, p["key"].as_str()), ("browser.press", Some("Shift+Tab")));
        let (m, p) = req(&["eval", "document.title"]).unwrap();
        assert_eq!(
            (m, p["js"].as_str()),
            ("browser.eval", Some("document.title"))
        );
        let (m, p) = req(&["screenshot", "/tmp/x.png", "--full"]).unwrap();
        assert_eq!((m, p["full"].as_bool()), ("browser.screenshot", Some(true)));
        let (m, p) = req(&["wait", "--text", "Saved", "--timeout", "5"]).unwrap();
        assert_eq!(
            (m, &p["for"], p["timeout"].as_u64()),
            ("browser.wait", &json!({"text": "Saved"}), Some(5))
        );
        assert_eq!(req(&["wait", "--load"]).unwrap().1["for"], json!("load"));
        assert_eq!(
            req(&["wait", "--ref", "e2"]).unwrap().1["for"],
            json!({"ref": "e2"})
        );
        assert!(req(&["wait"]).is_err(), "needs a condition");
        assert!(
            req(&["wait", "--load", "--text", "x"]).is_err(),
            "one condition"
        );
        assert_eq!(req(&["back"]).unwrap().0, "browser.back");
        assert_eq!(
            req(&["console", "--pane", "p_3"]).unwrap().1["pane"],
            json!("p_3")
        );
        assert_eq!(req(&["ports"]).unwrap().0, "browser.ports");
        assert!(req(&["fill", "e3"]).is_err(), "fill needs text");
    }

    #[test]
    fn r71_r72_annotations_engine_and_close_parse() {
        let (m, p) = req(&["annotations", "--send", "--to", "p_2"]).unwrap();
        assert_eq!(
            (m, p["send"].as_bool(), p["to"].as_str()),
            ("browser.annotations", Some(true), Some("p_2"))
        );
        assert!(
            req(&["annotations", "--to", "p_2"]).is_err(),
            "--to needs --send"
        );
        let (m, p) = req(&["annotate", "on"]).unwrap();
        assert_eq!((m, p["on"].as_bool()), ("browser.annotate", Some(true)));
        assert!(req(&["close"]).is_err(), "close is for chromium");
        let cli = Cli::try_parse_from([
            "rade", "browser", "snapshot", "--engine", "chromium", "--headed",
        ])
        .unwrap();
        let Top::Browser(a) = cli.cmd;
        assert!(a.engine == Engine::Chromium && a.headed);
        assert_eq!(request(&a).unwrap().0, "browser.snapshot");
        let cli =
            Cli::try_parse_from(["rade", "browser", "--engine", "chromium", "close"]).unwrap();
        let Top::Browser(a) = cli.cmd;
        assert_eq!(request(&a).unwrap().0, "browser.close");
        assert!(Cli::try_parse_from(["rade", "browser", "--engine", "gecko", "snapshot"]).is_err());
    }
}
