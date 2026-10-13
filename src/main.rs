use clap::Parser;
use readaloud::cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Script(_) => anyhow::bail!("no implementado"),
        Command::Say(_) => anyhow::bail!("no implementado"),
        Command::Voices(_) => anyhow::bail!("no implementado"),
        Command::Play(_) => anyhow::bail!("no implementado"),
    }
}
