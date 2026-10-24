//! Inline sub-forms of the command palette (R14.4): an action that needs
//! input (Spawn Agent…, New Worktree…) replaces the result list with a few
//! fields; Enter submits, Esc goes back to the list. "Fan-out…" (R15.1)
//! picks Agents with counts. GTK-free: fields, values, checks.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// A one-line text entry.
    Text,
    /// One of the options (a drop-down).
    Choice(Vec<String>),
    /// A count per option (spin buttons); value `a=1,b=2`, zeros left out.
    Counts(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub value: String,
    pub placeholder: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    /// The palette action that opened it (`spawn_agent`, `new_worktree`).
    pub action: String,
    pub title: String,
    /// The submit button's label.
    pub submit: &'static str,
    pub fields: Vec<Field>,
}

/// What a form may offer: the Agent Presets, and whether the Worktree on
/// screen belongs to General (no git repository).
pub struct FormContext<'a> {
    pub presets: &'a [String],
    pub general: bool,
}

/// Where "Spawn Agent…" puts the Agent.
pub const WHERE_NEW_TAB: &str = "New Tab";
pub const WHERE_SPLIT: &str = "Split Right";
pub const WHERE_WORKTREE: &str = "New Worktree";

fn text(key: &'static str, label: &'static str, placeholder: &str) -> Field {
    Field {
        key,
        label,
        kind: FieldKind::Text,
        value: String::new(),
        placeholder: placeholder.into(),
    }
}

fn choice(key: &'static str, label: &'static str, options: Vec<String>) -> Field {
    Field {
        key,
        label,
        value: options.first().cloned().unwrap_or_default(),
        kind: FieldKind::Choice(options),
        placeholder: String::new(),
    }
}

/// The form an action opens, if it takes input.
pub fn form_for(action: &str, ctx: &FormContext) -> Option<Form> {
    Some(match action {
        "spawn_agent" => {
            // claude first (the common case), then the rest by name.
            let mut presets = ctx.presets.to_vec();
            if let Some(i) = presets.iter().position(|p| p == "claude") {
                let c = presets.remove(i);
                presets.insert(0, c);
            }
            let mut places = vec![WHERE_NEW_TAB, WHERE_SPLIT];
            if !ctx.general {
                places.push(WHERE_WORKTREE);
            }
            Form {
                action: action.into(),
                title: "Spawn Agent".into(),
                submit: "Spawn",
                fields: vec![
                    choice("agent", "Agent", presets),
                    text("prompt", "Prompt", "optional first prompt"),
                    choice(
                        "where",
                        "Where",
                        places.into_iter().map(String::from).collect(),
                    ),
                ],
            }
        }
        "fanout" => {
            // One claude and one codex (compare two models), else two of the first.
            let has = |p: &str| ctx.presets.iter().any(|x| x == p);
            let value = match ctx.presets.first() {
                _ if has("claude") && has("codex") => "claude=1,codex=1".into(),
                Some(p) => format!("{p}=2"),
                None => String::new(),
            };
            Form {
                action: action.into(),
                title: "Fan-out".into(),
                submit: "Start",
                fields: vec![
                    text("prompt", "Prompt", "the task every Agent gets"),
                    Field {
                        key: "agents",
                        label: "Agents",
                        kind: FieldKind::Counts(ctx.presets.to_vec()),
                        value,
                        placeholder: String::new(),
                    },
                    text("base", "Base", "main branch"),
                    text("name", "Name", "fanout-<n>"),
                ],
            }
        }
        "new_worktree" => Form {
            action: action.into(),
            title: "New Worktree".into(),
            submit: "Create",
            fields: vec![
                text("name", "Name", "branch name, e.g. feat-login"),
                text("base", "Base", "main branch"),
            ],
        },
        "create_workspace" => Form {
            action: action.into(),
            title: "Create Workspace".into(),
            submit: "Set up",
            fields: vec![
                text("root", "Root", "folder, e.g. ~/workspaces/acme"),
                text("name", "Name", "the folder's name"),
                text("git_name", "Git name", "Ann Example"),
                text("email", "Email", "ann@acme.com"),
                text("github", "GitHub", "GitHub login"),
                text("ssh_key", "SSH key", "id_ed25519_<name> in ~/.ssh"),
                choice(
                    "claude",
                    "Claude",
                    vec![CLAUDE_SHARED.into(), CLAUDE_SEPARATE.into()],
                ),
            ],
        },
        _ => return None,
    })
}

/// "Create Workspace…" Claude choices: the usual `~/.claude`, or a config
/// dir of its own (`--claude separate`).
pub const CLAUDE_SHARED: &str = "Shared (~/.claude)";
pub const CLAUDE_SEPARATE: &str = "Separate (~/.claude-<name>)";

/// The command "Create Workspace…" runs in a Terminal (R25.6): `rade
/// workspace setup` with the fields given, so the gh login shows there.
pub fn setup_command(f: &Form) -> String {
    let q = crate::preset::shell_quote;
    let v = |k: &str| f.value(k).trim().to_string();
    let mut cmd = format!("rade workspace setup {}", q(&v("root")));
    for (key, flag) in [
        ("name", "--name"),
        ("git_name", "--git-name"),
        ("email", "--email"),
        ("github", "--github"),
        ("ssh_key", "--ssh-key"),
    ] {
        let val = v(key);
        if !val.is_empty() {
            cmd.push_str(&format!(" {flag} {}", q(&val)));
        }
    }
    if f.value("claude") == CLAUDE_SEPARATE {
        cmd.push_str(" --claude separate");
    }
    cmd
}

impl Form {
    pub fn value(&self, key: &str) -> &str {
        self.fields
            .iter()
            .find(|f| f.key == key)
            .map_or("", |f| f.value.as_str())
    }

    /// Set a field; a choice only takes one of its options.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let f = self
            .fields
            .iter_mut()
            .find(|f| f.key == key)
            .ok_or_else(|| format!("no field '{key}'"))?;
        if let FieldKind::Choice(opts) = &f.kind {
            if !opts.iter().any(|o| o == value) {
                return Err(format!("{} must be one of: {}", f.label, opts.join(", ")));
            }
        }
        if let FieldKind::Counts(opts) = &f.kind {
            for part in value.split(',').filter(|p| !p.trim().is_empty()) {
                let (k, n) = part.split_once('=').unwrap_or((part, ""));
                if !opts.iter().any(|o| o == k.trim()) || n.trim().parse::<u32>().is_err() {
                    return Err(format!(
                        "{} takes <preset>=<count>,… ({})",
                        f.label,
                        opts.join(", ")
                    ));
                }
            }
        }
        f.value = value.into();
        Ok(())
    }

    /// The Agents of a Counts field, expanded in option order
    /// (`claude=1,codex=2` → claude, codex, codex).
    pub fn agents(&self, key: &str) -> Vec<String> {
        let Some(FieldKind::Counts(opts)) =
            self.fields.iter().find(|f| f.key == key).map(|f| &f.kind)
        else {
            return Vec::new();
        };
        let counts: Vec<(&str, usize)> = self
            .value(key)
            .split(',')
            .filter_map(|p| {
                let (k, n) = p.split_once('=')?;
                Some((k.trim(), n.trim().parse().ok()?))
            })
            .collect();
        opts.iter()
            .flat_map(|o| {
                let n = counts.iter().filter(|(k, _)| k == o).map(|(_, n)| n).sum();
                std::iter::repeat_n(o.clone(), n)
            })
            .collect()
    }

    /// Nothing typed yet: a missing required value is not shown as an
    /// error until the user submits.
    pub fn pristine(&self) -> bool {
        self.fields
            .iter()
            .all(|f| f.kind != FieldKind::Text || f.value.is_empty())
    }

    /// Why the form can't be submitted yet (shown under the fields).
    pub fn validate(&self, ctx: &FormContext) -> Result<(), String> {
        match self.action.as_str() {
            "spawn_agent" if self.value("agent").is_empty() => {
                Err("No Agent Presets configured".into())
            }
            "fanout" => {
                if ctx.general {
                    return Err("General has no git repository; pick a Source first".into());
                }
                if self.value("prompt").trim().is_empty() {
                    return Err("Type the task every Agent gets".into());
                }
                let n = self.agents("agents").len();
                if n < crate::fanout::MIN_AGENTS {
                    return Err(format!(
                        "Pick at least {} Agents",
                        crate::fanout::MIN_AGENTS
                    ));
                }
                let name = self.value("name").trim();
                if !name.is_empty() {
                    crate::git::validate_name(name).map_err(|e| e.message)?;
                }
                if self.value("base").trim().starts_with('-') {
                    return Err("Base can't start with '-'".into());
                }
                Ok(())
            }
            "new_worktree" => {
                if ctx.general {
                    return Err("General has no git repository; pick a Source first".into());
                }
                let name = self.value("name").trim();
                if name.is_empty() {
                    return Err("Name the Worktree (it is also its branch)".into());
                }
                crate::git::validate_name(name).map_err(|e| e.message)?;
                if self.value("base").trim().starts_with('-') {
                    return Err("Base can't start with '-'".into());
                }
                Ok(())
            }
            "create_workspace" => {
                if self.value("root").trim().is_empty() {
                    return Err("Pick the Workspace's root folder".into());
                }
                let email = self.value("email").trim();
                if !email.is_empty() && !email.contains('@') {
                    return Err("Email needs an @".into());
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(general: bool) -> FormContext<'static> {
        FormContext {
            presets: Box::leak(Box::new(vec!["codex".to_string(), "claude".into()])),
            general,
        }
    }

    #[test]
    fn r14_4_spawn_agent_form_offers_presets_prompt_and_place() {
        let f = form_for("spawn_agent", &ctx(false)).unwrap();
        assert_eq!(f.value("agent"), "claude", "claude first");
        assert_eq!(f.value("where"), WHERE_NEW_TAB);
        assert!(matches!(&f.fields[2].kind, FieldKind::Choice(o) if o.len() == 3));
        assert!(f.validate(&ctx(false)).is_ok(), "prompt is optional");
        // General: no Worktree to create.
        let g = form_for("spawn_agent", &ctx(true)).unwrap();
        assert!(
            matches!(&g.fields[2].kind, FieldKind::Choice(o) if !o.contains(&WHERE_WORKTREE.to_string()))
        );
        let none = FormContext {
            presets: &[],
            general: false,
        };
        assert!(form_for("spawn_agent", &none)
            .unwrap()
            .validate(&none)
            .is_err());
    }

    #[test]
    fn r14_4_set_checks_choices_and_fields() {
        let mut f = form_for("spawn_agent", &ctx(false)).unwrap();
        f.set("agent", "codex").unwrap();
        f.set("prompt", "Tiếng Việt").unwrap();
        assert_eq!(
            (f.value("agent"), f.value("prompt")),
            ("codex", "Tiếng Việt")
        );
        assert!(f.set("agent", "nope").unwrap_err().contains("claude"));
        assert!(f.set("bogus", "x").is_err());
    }

    #[test]
    fn r15_1_fanout_form_counts_agents_and_validates() {
        let c = ctx(false);
        let mut f = form_for("fanout", &c).unwrap();
        assert_eq!(
            f.value("agents"),
            "claude=1,codex=1",
            "two models by default"
        );
        assert_eq!(f.agents("agents"), ["codex", "claude"], "option order");
        assert!(f.validate(&c).unwrap_err().contains("task"));
        f.set("prompt", "add a README line").unwrap();
        assert!(f.validate(&c).is_ok());
        f.set("agents", "codex=3,claude=0").unwrap();
        assert_eq!(f.agents("agents"), ["codex", "codex", "codex"]);
        f.set("agents", "codex=1").unwrap();
        assert!(f.validate(&c).unwrap_err().contains("at least 2"));
        assert!(f.set("agents", "gemini=1").is_err());
        assert!(f.set("agents", "codex=x").is_err());
        f.set("agents", "codex=2").unwrap();
        f.set("name", "a b").unwrap();
        assert!(f.validate(&c).is_err());
        f.set("name", "").unwrap();
        assert!(f.validate(&ctx(true)).unwrap_err().contains("General"));
        let one = FormContext {
            presets: Box::leak(Box::new(vec!["fake".to_string()])),
            general: false,
        };
        assert_eq!(form_for("fanout", &one).unwrap().value("agents"), "fake=2");
    }

    #[test]
    fn r14_4_new_worktree_form_validates_the_name() {
        let c = ctx(false);
        let mut f = form_for("new_worktree", &c).unwrap();
        assert!(f.validate(&c).unwrap_err().contains("Name"));
        f.set("name", "a b").unwrap();
        assert!(f
            .validate(&c)
            .unwrap_err()
            .contains("invalid worktree name"));
        f.set("name", "feat-x").unwrap();
        assert!(f.validate(&c).is_ok());
        f.set("base", "--lock").unwrap();
        assert!(f.validate(&c).is_err());
        f.set("base", "").unwrap();
        assert!(f.validate(&ctx(true)).unwrap_err().contains("General"));
        assert!(!f.pristine());
        assert!(form_for("new_worktree", &c).unwrap().pristine());
        assert!(form_for("split_right", &c).is_none(), "no input needed");
    }

    #[test]
    fn r25_6_create_workspace_form_builds_the_setup_command() {
        let c = ctx(false);
        let mut f = form_for("create_workspace", &c).unwrap();
        assert!(f.validate(&c).unwrap_err().contains("root"));
        f.set("root", "~/workspaces/acme").unwrap();
        assert!(f.validate(&c).is_ok(), "the rest is optional");
        assert_eq!(
            setup_command(&f),
            "rade workspace setup '~/workspaces/acme'"
        );
        f.set("git_name", "Ann O'Hara").unwrap();
        f.set("email", "ann").unwrap();
        assert!(f.validate(&c).unwrap_err().contains("@"));
        f.set("email", "ann@acme.test").unwrap();
        f.set("github", "ann-acme").unwrap();
        f.set("claude", CLAUDE_SEPARATE).unwrap();
        assert_eq!(
            setup_command(&f),
            "rade workspace setup '~/workspaces/acme' --git-name 'Ann O'\\''Hara' --email 'ann@acme.test' --github 'ann-acme' --claude separate"
        );
    }
}
