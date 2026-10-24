# Session Restore recreates layout and resumes Agents; processes do not survive

After restart RADE recreates Projects, Tabs, Layouts, cwds and runs each Agent Preset's resume command; running processes die with the app. Keeping PTYs alive needs a separate daemon plus Ghostty's manual-I/O surface mode, estimated 2–3 extra weeks and not done by any GTK embedder we found. Terminals obtain their PTY through a single `PtySource` interface so a daemon-backed source can be added later without touching the UI.
