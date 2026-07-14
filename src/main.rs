use anyhow::Result;
use tui_life_metrics::cli;

fn main() -> Result<()> {
    cli::run(std::env::args().skip(1).collect())
}
