# 0048 — Updates from GitHub Releases, installed by the app, applied on restart

Status: accepted (2026-10-26)

## Context
RADE ships as a `.deb` on GitHub Releases (ADR-0012) with no apt repository, so nobody learns about a new version and `apt upgrade` never installs one. The app runs many Agents for hours: an update must not interrupt them. On 2026-10-26 the owner upgraded 1.0.0 → 1.1.0 with `pkexec apt-get install` while the app ran: dpkg replaces files by rename, the running process keeps its mapped binary and library, and the new `rade` CLI talks to the old app (the socket contract is stable within a MAJOR, ADR-0041).

## Decision
- The app asks the public GitHub Releases API once a day (no token, no identifier beyond `User-Agent: rade/<version>`), on by default with a line in Settings saying what is sent; `RADE_UPDATE_CHECK=0` and `[update] check = false` turn it off.
- It tells quietly: a Statusline cell and an Activity row; a Toast only for a release marked security (`### Security`, from a `Security: yes` commit footer), never while an Agent waits. No modal.
- Install is one click on a `.deb` install: download, SHA-256 against `SHA256SUMS` and the API's asset digest, then `pkexec /usr/lib/rade/rade-install <deb> <sha256>` — a root-owned helper shipped in the .deb that copies the file into a root-only directory, checks the SHA-256 there (the download folder is user-writable) and runs `apt-get install -y --no-remove` on the copy — the system's own authorization; RADE never stores or asks for a password.
- Restart is the owner's: the app keeps running the old version until they click Restart, which asks first (running Agents come back interrupted through Session Restore), quits normally, and a detached helper starts the new binary after the old process is gone (one instance per `state.json`, ADR-0021).
- Other installs are told and sent to the release page; Devel builds never check.

## Not done
An apt repository / PPA, automatic install or restart, delta updates, attestation checks in the app (needs gh ≥ 2.81 or a sigstore client), Flatpak / Snap (non-goal), downgrades, non-amd64 builds.
