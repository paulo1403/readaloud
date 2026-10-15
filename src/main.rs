use clap::Parser;
use readaloud::cli::{Cli, Command};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Script(args) => {
            if args.timeline {
                let voices =
                    readaloud::tts::voices::discover(&readaloud::tts::voices::default_dir());
                let Some(voz) = voices.first() else {
                    anyhow::bail!(
                        "no hay voces instaladas; ejecuta `readaloud voices prepare <nombre>`"
                    );
                };
                print!(
                    "{}",
                    readaloud::cli::script::run_timeline(&args.path, &voz.onnx)?
                );
            } else {
                print!("{}", readaloud::cli::script::run_text(&args.path)?);
            }
            Ok(())
        }
        Command::Say(args) => readaloud::cli::say::run(
            &args.text,
            args.out.as_deref(),
            args.length_scale,
            args.voice.as_deref(),
        ),
        Command::Voices(args) => match args.cmd {
            readaloud::cli::VoicesCmd::List { dir } => {
                readaloud::cli::voices::list(dir.as_deref())
            }
            readaloud::cli::VoicesCmd::Prepare { name, dir } => {
                readaloud::cli::voices::prepare(dir.as_deref(), &name)
            }
        },
        Command::Play(args) => {
            readaloud::cli::play::run(&args.path, &args.voice, args.length_scale)
        }
    }
}
