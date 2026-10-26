# Updates

RADE tells you when a newer release is out, installs it from inside the app and restarts when you
say so. It never interrupts the Agents that are running: no modal, no automatic install, no
automatic restart.

RADE ships as a `.deb` on GitHub Releases and there is no apt repository, so `apt upgrade` never
brings a new RADE. This is what does.

## What you see

A cell appears in the Statusline only while there is something to say. It sits after the Workspace,
Worktree and count cells and before the system monitor; on a narrow window it is dropped after the
hints and the monitor, and before the ports.

| Cell | Meaning | Click |
|---|---|---|
| `1.2.0` | RADE 1.2.0 is available | Settings › Updates |
| `1.2.0 security` | it fixes a security issue (amber) | Settings › Updates |
| `2.0.0 major` | it changes the `rade` CLI, the socket or the config files: read the notes first (amber) | Settings › Updates |
| `1.2.0 42%` | downloading | Settings › Updates |
| `1.2.0 installing` | checking the download, then installing | Settings › Updates |
| `restart for 1.2.0` | installed; the running window is still the old version | the Restart dialog |
| `update failed` | the last Install failed (red); the tooltip says why | Settings › Updates |

A skipped version shows no cell. Every new version also gets one row in the Activity tab.

**Settings › Updates** (`Ctrl+,`, the palette's **About RADE**, or a click on the cell) starts with
the About block: `RADE <version>`, a status (`Up to date`, `RADE 1.2.0 is available`, `Couldn't check`,
…), when it last checked, the buttons that fit the moment (**Check now**, **Install 1.2.0**, **Skip
This Version**, **Cancel**, **Try again**, **Restart…**), and **Release notes ›**, which opens what
changed since your version, newest release first, in the Markdown Viewer. Under it are the two rows
**Check for updates** and **Update channel**.

A **security** release raises one Toast, `RADE 1.2.0 fixes a security issue`, with **Details**. It
waits while any Agent is waiting for you, shows once per version, and other releases stay quiet.
The first start on a new version shows `Updated to RADE 1.2.0` once, with **What's new**.

The palette has **About RADE**, **Check for Updates** and **Restart to Finish Update…**.

## Install

**Install 1.2.0** (only a click starts it; only for a RADE installed from the `.deb`):

1. The `.deb` is downloaded into `~/.cache/rade/update/`, with progress and **Cancel**.
2. Its SHA-256 must equal the line in the release's `SHA256SUMS` and the digest GitHub gives for
   the asset. If not, it is deleted: `Download didn't match SHA256SUMS. Nothing was installed.`
3. `pkexec /usr/bin/apt-get install -y --no-remove <the .deb>` runs. **The system shows its own
   password dialog**; RADE never sees the password. Closing that dialog goes back to Install with
   no error. A failure says why (`Another package manager is running. Try again in a minute.`, or
   apt's last line).
4. The `.deb` is deleted and the cell turns `restart for 1.2.0`.

The app keeps running on the files it already loaded, and the new `rade` CLI talks to it, so nothing
stops. RADE does not verify GitHub attestations (needs `gh` 2.81 or newer); it checks the checksums
above and only downloads from `github.com/richard-plg/rade/releases/download/<tag>/`.

## Restart

When the cell reads `restart for 1.2.0` (it also does when you ran `sudo apt install` by hand), a
click asks first: it counts the Agents that are running (they stop and come back **interrupted**
through Session Restore, so send "continue" to go on) and the ones waiting for you. **Restart**
saves, quits and starts the new version with your Tabs and Panes as they were; **Not now** keeps
everything running and the cell stays.

## Channels, skipping, turning it off

- `update.channel = "stable"` (default) only offers final releases; `"rc"` offers release
  candidates too. Switching recomputes the offer from the last answer, with no request.
- **Skip This Version** hides that version everywhere. A newer one shows again.
- `update.check = false` (Settings › Updates › Check for updates) stops the automatic checks. An
  offer found earlier still shows, and **Check now** still asks.
- `RADE_UPDATE_CHECK=0` in RADE's environment stops every check, **Check now** included.

```toml
[update]
check = true         # ask GitHub once a day
channel = "stable"   # stable | rc
```

## What is sent

One `GET https://api.github.com/repos/richard-plg/rade/releases?per_page=20`, 60 seconds after
RADE starts and then at most once a day (an hour after a failed check). No token and no cookie; the
only thing identifying you is `User-Agent: rade/<version>`. The answer and the time are cached in
`~/.cache/rade/update/update.json`, which `rade version` shares. Unauthenticated, GitHub allows 60
requests an hour per IP address.

## Other installs

- A build run from source (`target/release/rade-app`, the Devel flavor) never checks; the About
  block says `Development build`.
- A RADE installed some other way (`cargo install`, an extracted `.deb`) is told about updates but
  gets **Download**, which opens the release page, instead of Install.

## `rade version`

```sh
rade version            # three lines: the CLI, the running app, the last check
rade version --check    # ask GitHub once now
rade version --json
```

```
rade 1.2.0
app 1.1.0 (pid 4242) · restart RADE to finish updating
update 1.3.0 available (minor) · checked 5m ago
```

It works without the app (`app not running`). When the app is older than the CLI, the line ends
`restart RADE to finish updating`, and any `rade` call the old app does not know fails with that
hint on stderr. Fields: [API](../API.md#rade-version---check---json).

## Environment variables

Test and power-user switches; see [API: Update variables](../API.md#update-variables). Only
`RADE_UPDATE_CHECK=0` works in the installed app; the others redirect where a root install comes
from, so they are honoured only in a Devel build.
