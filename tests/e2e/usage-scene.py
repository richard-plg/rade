#!/usr/bin/env python3
"""Scene E / F usage data (docs/internals/design/spec/e-usage.md "Content") written
straight into a temp Usage store, the way the app writes it
(`$XDG_STATE_HOME/rade-dev/usage/<agent>-<sha8 of realpath config_dir>.jsonl`,
docs/API.md "Usage store"), relative to now:

  acme-work  claude ~/.claude-work   5-hour rising to 62 % over 40 min (resets in
             1h 12m: forecast hits 100 % in ~40m), weekly 41 / Opus 58 / Sonnet 22 /
             OAuth apps 3 % (in 3 days 09:00), spend $12 of $50 /mo; 12.4M tokens in
             7 days (Shop 9.1M, Billing 2.3M, Scratch 1.0M; opus 78 %, sonnet 22 %;
             max day 3.1M, today 1.5M); updated 40 s ago
  acme-team  claude ~/.claude-team   5-hour 8 %, weekly 12 %; 1.1M; 2 h
  acme       codex  ~/.codex         weekly 33 %, Spark 5-hour 81 % (resets in 2h 30m); 0.9M; 5 m
  acme-biz   codex  ~/.codex-biz     weekly 27 %; 2.6M; 1 h
  personal   claude ~/.claude        weekly rising to 88 % (in 2 days 21:00, forecast 97 %
             at reset), 5-hour 30 %; 6.8M; 1 d
  personal   codex  ~/.codex-personal weekly 4 %; 0.2M; 3 d
  oss        codex  ~/.codex-oss     no data

Every store file is rewritten (old lines go), so a rerun re-centres the
scene on the current time. Tokens also exist 8–20 days back (30 days ≠ 7
days) and carry cache reads (never counted).

Usage: usage-scene.py <usage dir> <home>
"""
import hashlib
import json
import os
import sys
import time

USAGE, HOME = sys.argv[1], sys.argv[2]
NOW = int(time.time())
MIN, HOUR, DAY = 60, 3600, 86400
os.makedirs(USAGE, exist_ok=True)


def store(agent, config_dir):
    real = os.path.realpath(os.path.join(HOME, config_dir))
    sha8 = hashlib.sha256(real.encode()).hexdigest()[:8]
    return os.path.join(USAGE, f"{agent}-{sha8}.jsonl")


def day(t):
    return time.strftime("%Y-%m-%d", time.localtime(t))


def local_at(days, hour):
    """Local `hour`:00 on the day `days` after today: 2 or 3 days ahead is always
    between 1 and 7 days away, so the UI names it `<Day> HH:00` (spec E.24's
    `Sat 21:00`, `Mon 09:00` on the mockup's Friday)."""
    lt = time.localtime(NOW + days * DAY)
    return int(time.mktime((lt.tm_year, lt.tm_mon, lt.tm_mday, hour, 0, 0, 0, 0, -1)))


def limit(account, lid, group, name, used, reset, t, window=None, **extra):
    r = {"kind": "limit", "account": account, "limit_id": lid, "group": group, "name": name,
         "used_percent": used, "resets_at": reset, "source": extra.pop("source", "statusline"), "t": t}
    if window:
        r["window_minutes"] = window
    r.update(extra)
    return r


def tokens(t, d, project, model, n):
    return {"kind": "tokens", "t": t, "day": d, "project": project, "model": model,
            "input": n, "output": 0, "cache_creation": 0, "cache_read": n * 4}


def spread(total_by_day, projects, models, newest, end=0):
    """Token rows for days (oldest first, the last = `end` days before today)
    split by Project and model shares."""
    out = []
    n = len(total_by_day)
    for i, tot in enumerate(total_by_day):
        t = NOW - (n - 1 - i + end) * DAY
        stamp = min(t, newest)
        for p, ps in projects:
            for m, ms in models:
                v = int(round(tot * ps * ms))
                if v:
                    out.append(tokens(stamp, day(t), p, m, v))
    return out


def write(path, records):
    with open(path, "w") as f:
        for r in sorted(records, key=lambda r: r["t"]):
            f.write(json.dumps(r) + "\n")


M = 1_000_000
CL = [("claude-opus-4-5", 0.78), ("claude-sonnet-4-5", 0.22)]
CX = [("gpt-5.3-codex", 1.0)]

# acme-work
five_reset = NOW + 72 * MIN
mon = local_at(3, 9)
t_last = NOW - 40
# 0.92 % per minute up to 62 %: `hits 100% in ~40m` for the first ~3 minutes
# after writing (the 40 s since the last sample included).
recs = [limit("acme-work", "five_hour", "Session", "5-hour", round(62 - 0.92 * (40 - k), 2), five_reset,
              t_last - (40 - k) * MIN, 300) for k in range(41)]
recs += [
    limit("acme-work", "seven_day", "Weekly", "All models", 41, mon, t_last - 30, 10080),
    limit("acme-work", "seven_day_opus", "Weekly", "Opus", 58, mon, t_last - 30, 10080, source="detailed"),
    limit("acme-work", "seven_day_sonnet", "Weekly", "Sonnet", 22, mon, t_last - 30, 10080, source="detailed"),
    limit("acme-work", "seven_day_oauth_apps", "Weekly", "OAuth apps", 3, mon, t_last - 30, 10080, source="detailed"),
    limit("acme-work", "spend_limit", "Extra usage", "Spend limit", 24, NOW + 9 * DAY, t_last - 30,
          used_usd=12, limit_usd=50, period="month"),
]
week = [1.6, 0.5, 0.4, 2.6, 3.1, 2.7, 1.5]
older = [0.8] * 13 + [0.0] * 3  # days −22 … −7 (the 30-day period only)
recs += spread([x * M for x in older], [("Shop", 1.0)], CL, t_last - 60, end=7)
recs += spread([x * M for x in week], [("Shop", 9.1 / 12.4), ("Billing", 2.3 / 12.4), ("Scratch", 1.0 / 12.4)],
               CL, t_last - 60)
write(store("claude", ".claude-work"), recs)

# acme-team
t = NOW - 2 * HOUR
write(store("claude", ".claude-team"), [
    limit("acme-team", "five_hour", "Session", "5-hour", 8, NOW + 3 * HOUR, t, 300),
    limit("acme-team", "seven_day", "Weekly", "All models", 12, NOW + 4 * DAY, t, 10080),
] + spread([0, 0, 0.3 * M, 0.2 * M, 0.2 * M, 0.2 * M, 0.2 * M], [("Shop", 1.0)], CL, t))

# acme (codex)
t = NOW - 5 * MIN
write(store("codex", ".codex"), [
    limit("acme", "codex", "Codex", "weekly", 33, NOW + 4 * DAY + 3 * HOUR, t, 10080, source="rollout"),
    limit("acme", "codex_bengalfox", "Spark", "5-hour", 81, NOW + 150 * MIN, t, 300, source="rollout",
          model="GPT-5.3-Codex-Spark"),
] + spread([0.1 * M, 0.1 * M, 0.1 * M, 0.2 * M, 0.2 * M, 0.1 * M, 0.1 * M], [("Shop", 1.0)], CX, t))

# acme-biz (codex)
t = NOW - HOUR
write(store("codex", ".codex-biz"), [
    limit("acme-biz", "codex", "Codex", "weekly", 27, NOW + 5 * DAY, t, 10080, source="rollout"),
] + spread([0.4 * M, 0.3 * M, 0.3 * M, 0.4 * M, 0.4 * M, 0.4 * M, 0.4 * M], [("Billing", 1.0)], CX, t))

# personal (claude): weekly rising to 88 % one day ago, 97 % forecast at its reset.
t1 = NOW - DAY - 60
# 2 days ahead at 21:00, or 1 day ahead before 03:00: the span from the last
# sample stays ≤ 90 h, so the trend (9 % over it) stays above the forecast's
# 0.1 %/h floor whatever the hour of the run.
sat = local_at(2, 21)
if sat - t1 > 90 * HOUR:
    sat = local_at(1, 21)
slope = 9.0 / (sat - t1)  # % per second
recs = [limit("personal", "seven_day", "Weekly", "All models", round(88 - slope * (t1 - (t1 - k * HOUR)), 3),
              sat, t1 - k * HOUR, 10080) for k in range(12, -1, -1)]
recs.append(limit("personal", "five_hour", "Session", "5-hour", 30, NOW + 2 * HOUR, t1, 300))
recs += spread([1.0 * M, 1.0 * M, 1.0 * M, 1.4 * M, 1.4 * M, 1.0 * M, 0], [("Scratch", 1.0)], CL, t1)
write(store("claude", ".claude"), recs)

# personal (codex)
t = NOW - 3 * DAY
write(store("codex", ".codex-personal"), [
    limit("personal", "codex", "Codex", "weekly", 4, NOW + 6 * DAY, t, 10080, source="rollout"),
] + spread([0.05 * M, 0.05 * M, 0.05 * M, 0.05 * M, 0, 0, 0], [("Scratch", 1.0)], CX, t))

# oss: nothing.
p = store("codex", ".codex-oss")
if os.path.exists(p):
    os.remove(p)
print(json.dumps({"now": NOW, "five_reset": five_reset, "mon": mon, "sat": sat}))
