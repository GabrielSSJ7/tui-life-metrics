use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};

use crate::parser::{ActionParser, ClaudeParser};
use crate::paths;
use crate::store::Store;
use crate::ui;

/// Entry point: route the first argument to a subcommand.
///
/// ```text
/// tui-life-metrics                -> dashboard (default)
/// tui-life-metrics add            -> capture window
/// tui-life-metrics reprocess      -> reparse offline-saved entries via Claude
/// tui-life-metrics export <path>  -> copy the DB to <path>
/// tui-life-metrics import <path>  -> replace the DB from <path> (backs up first)
/// tui-life-metrics --help
/// ```
pub fn run(args: Vec<String>) -> Result<()> {
    match args.first().map(String::as_str).unwrap_or("dashboard") {
        "dashboard" => dashboard(),
        "add" | "capture" => capture(),
        "reprocess" => reprocess(),
        "export" => export(args.get(1)),
        "import" => import(args.get(1)),
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        other => Err(anyhow!(
            "unknown command {other:?}; run `tui-life-metrics --help`"
        )),
    }
}

fn open_store() -> Result<Store> {
    std::fs::create_dir_all(paths::data_dir()).context("creating data dir")?;
    Store::open(&paths::db_path())
}

fn dashboard() -> Result<()> {
    ui::dashboard::run(open_store()?)
}

fn capture() -> Result<()> {
    let parser: Arc<dyn ActionParser> = Arc::new(ClaudeParser::default());
    ui::capture::run(open_store()?, parser)
}

/// Reparse every offline-saved entry through Claude, in place.
fn reprocess() -> Result<()> {
    let store = open_store()?;
    let parser = ClaudeParser::default();
    let pending = store.unprocessed()?;
    if pending.is_empty() {
        println!("Nothing to reprocess.");
        return Ok(());
    }
    println!("Reprocessing {} entr(ies)…", pending.len());
    let mut done = 0;
    for entry in &pending {
        match parser.parse(&entry.raw_text) {
            Ok(action) => {
                let action = action.normalized();
                store.update_processed(entry.id, &action)?;
                done += 1;
                println!("  ✓ {} -> {}", entry.raw_text, action.category);
            }
            Err(err) => println!("  ✗ {} ({err})", entry.raw_text),
        }
    }
    println!("Done: {done}/{} reprocessed.", pending.len());
    Ok(())
}

fn export(dest: Option<&String>) -> Result<()> {
    let dest = dest.ok_or_else(|| anyhow!("usage: tui-life-metrics export <path>"))?;
    let src = paths::db_path();
    if !src.exists() {
        bail!("no database at {} yet — add an entry first", src.display());
    }
    std::fs::copy(&src, dest).with_context(|| format!("copying db to {dest}"))?;
    println!("Exported {} -> {dest}", src.display());
    Ok(())
}

fn import(source: Option<&String>) -> Result<()> {
    let source = source.ok_or_else(|| anyhow!("usage: tui-life-metrics import <path>"))?;
    let src = PathBuf::from(source);
    validate_sqlite(&src)?;
    let dest = paths::db_path();
    std::fs::create_dir_all(paths::data_dir())?;
    backup_existing(&dest)?;
    std::fs::copy(&src, &dest).with_context(|| format!("importing {source}"))?;
    println!("Imported {source} -> {}", dest.display());
    Ok(())
}

/// Refuse to import a file that is not a readable SQLite DB with our schema.
fn validate_sqlite(src: &Path) -> Result<()> {
    if !src.exists() {
        bail!("source {} does not exist", src.display());
    }
    let store =
        Store::open(src).with_context(|| format!("{} is not a valid sqlite db", src.display()))?;
    // Touch the schema so a stray sqlite file without `entries` is rejected early.
    store
        .entries_between(
            chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap(),
            chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap(),
        )
        .with_context(|| format!("{} has no `entries` table", src.display()))?;
    Ok(())
}

fn backup_existing(dest: &Path) -> Result<()> {
    if !dest.exists() {
        return Ok(());
    }
    let bak = dest.with_extension("db.bak");
    std::fs::copy(dest, &bak).with_context(|| format!("backing up to {}", bak.display()))?;
    println!("Backed up current db -> {}", bak.display());
    Ok(())
}

fn print_help() {
    println!(
        "tui-life-metrics — log daily actions, aggregate flexible life metrics.\n\n\
         USAGE:\n\
         \x20 tui-life-metrics [COMMAND]\n\n\
         COMMANDS:\n\
         \x20 (none) | dashboard   Open the metrics dashboard (default)\n\
         \x20 add | capture        Open the capture window to log an action\n\
         \x20 reprocess            Reparse offline-saved entries via Claude\n\
         \x20 export <path>        Copy the SQLite DB to <path>\n\
         \x20 import <path>        Replace the DB from <path> (backs up current first)\n\
         \x20 --help               Show this help\n\n\
         ENV:\n\
         \x20 TUI_LIFE_METRICS_DIR  data root (default ~/.local/tui-life-metrics)\n\
         \x20 TUI_LIFE_METRICS_DB   explicit DB path (overrides the dir)"
    );
}
