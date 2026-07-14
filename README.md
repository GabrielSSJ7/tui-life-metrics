# tui-life-metrics

Log daily actions in plain language; Claude turns each sentence into structured
data; a TUI dashboard aggregates flexible, ever-growing life metrics. Storage is
local SQLite (git-ignored), with import/export for moving between devices.

## Flow

1. `SUPER+ALT+L` opens the **capture** window. Type `Fiz exercício hoje por 30min`.
2. The binary shells out to the `claude` CLI, which returns JSON:
   `{"category":"exercício","occurred_on":"2026-07-14","attributes":{"duration_min":30},"note":"exercício"}`.
3. The entry is stored. If Claude is unreachable, the raw sentence is saved as
   `unprocessed` and can be reparsed later with `tui-life-metrics reprocess`.
4. `SUPER+CTRL+ALT+L` opens the **dashboard**: counts, summed numeric attributes,
   trend vs. the previous period, and day-streaks per category.

## Design

- **Categories are model-decided.** Claude picks the life area freely per
  sentence; categories are lowercased/trimmed to avoid accidental duplicates.
- **Zero-migration metrics.** Each entry carries an `attributes` JSON bag, so a
  new metric (`distance_km`, `amount_brl`, …) is just a new key — no schema change.
  Any numeric attribute is automatically summed in the dashboard.
- **Blocking capture with offline fallback.** Parsing runs on a background thread
  so the spinner animates; nothing is lost if Claude is down.

## Commands

```
tui-life-metrics                Open the dashboard (default)
tui-life-metrics add            Open the capture window
tui-life-metrics reprocess      Reparse offline-saved entries via Claude
tui-life-metrics export <path>  Copy the DB to <path>
tui-life-metrics import <path>  Replace the DB from <path> (backs up current first)
tui-life-metrics --help
```

## Dashboard keys

| Key | Action |
|-----|--------|
| `d` `w` `m` `y` | Day / week / month / year granularity |
| `←` `→` (or `h` `l`) | Previous / next period |
| `↑` `↓` (or `k` `j`) | Select category |
| `Enter` | Drill into the selected category's entries |
| `Esc` | Back / quit |
| `q` | Quit |

## Install

```bash
./install.sh
```

Builds the release binary into `~/.local/bin`, creates the data dir, and prints
the Omarchy/Hyprland keybinds. Requires the `claude` CLI on `PATH`.

## Storage & sync

DB lives at `~/.local/tui-life-metrics/metrics.db` (override with
`TUI_LIFE_METRICS_DIR` / `TUI_LIFE_METRICS_DB`). It is git-ignored. To move
devices: `export` on the old, copy the file over, `import` on the new.
