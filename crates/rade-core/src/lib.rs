//! RADE core: GTK-free domain model, persistence, config, and the RPC contract.

pub mod account;
pub mod activity;
pub mod agents;
pub mod annotate;
pub mod basic;
pub mod browser;
pub mod cdp;
pub mod checkout;
pub mod chrome;
pub mod code;
pub mod config;
pub mod date;
pub mod design;
pub mod diff;
pub mod drift;
pub mod fanout;
pub mod filetree;
pub mod git;
pub mod home;
pub mod layers;
pub mod layout;
pub mod link;
pub mod linkview;
pub mod markdown;
pub mod merge;
pub mod model;
pub mod navigator;
pub mod new_menu;
pub mod notify;
pub mod overview;
pub mod palette;
pub mod palette_form;
pub mod paste;
pub mod paths;
pub mod ports;
pub mod preset;
pub mod process;
pub mod project;
pub mod review;
pub mod review_loop;
pub mod rpc;
pub mod scm;
pub mod scm_view;
pub mod search;
pub mod server;
pub mod settings;
pub mod setup;
pub mod shell_env;
pub mod status;
pub mod store;
pub mod sysmon;
pub mod title;
pub mod usage;
pub mod viewer;
pub mod workspace;
pub mod workspace_setup;
pub mod worktree;

pub use agents::{AgentStatus, SpawnSpec, TabChoice, WaitUntil};
pub use layout::{Direction, FocusAction, Layout, Pane, PaneKind, SplitDir};
pub use model::{
    general_id, Closed, PaneLoc, Source, State, Tab, Workspace, Worktree, DEFAULT_WORKSPACE,
    GENERAL_ID,
};
pub use rpc::{Error, ErrorKind};
