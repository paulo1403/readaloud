pub mod play;
pub mod say;
pub mod script;
pub mod voices;

use std::path::PathBuf;

#[derive(clap::Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Subcommand)]
pub enum Command {
    Script(ScriptArgs),
    Say(SayArgs),
    Voices(VoicesArgs),
    Play(PlayArgs),
}

#[derive(clap::Args)]
pub struct ScriptArgs {
    pub path: PathBuf,
    #[arg(long, conflicts_with = "text")]
    pub timeline: bool,
    #[arg(long)]
    pub text: bool,
}

#[derive(clap::Args)]
pub struct SayArgs {
    pub text: String,
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    #[arg(long, default_value = "1.0")]
    pub length_scale: f32,
    /// Nombre de la voz (sin `.onnx`). Por defecto, la primera descubierta.
    #[arg(long)]
    pub voice: Option<String>,
}

#[derive(clap::Args)]
pub struct VoicesArgs {
    #[command(subcommand)]
    pub cmd: VoicesCmd,
}

#[derive(clap::Subcommand)]
pub enum VoicesCmd {
    List {
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    Prepare {
        name: String,
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

#[derive(clap::Args)]
pub struct PlayArgs {
    pub path: PathBuf,
    #[arg(long)]
    pub voice: String,
    /// Escala de duración de la voz: por debajo de 1 habla más rápido y por
    /// encima de 1 más despacio (1.3 = más lento); no cambia el tono.
    #[arg(long, default_value = "1.0")]
    pub length_scale: f32,
    #[arg(long, hide = true)]
    pub max_sentences: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use clap::Parser;

    #[test]
    fn subcomandos_del_motor_existen() {
        let cli = Cli::parse_from(["readaloud", "script", "libro.epub", "--timeline"]);
        assert!(matches!(cli.command, Command::Script(a) if a.timeline));
    }

    #[test]
    fn no_hay_subcomandos_de_la_gui_todavia() {
        assert!(Cli::try_parse_from(["readaloud", "open", "x.epub"]).is_err());
    }

    #[test]
    fn la_cli_es_bien_formada() {
        Cli::command().debug_assert();
    }

    #[test]
    fn say_acepta_una_voz_y_la_usa() {
        let cli = Cli::parse_from(["readaloud", "say", "hola", "--voice", "es_ES-davefx-medium"]);
        let Command::Say(args) = cli.command else {
            panic!("esperaba el subcomando say");
        };
        assert_eq!(args.voice.as_deref(), Some("es_ES-davefx-medium"));
        // Una voz inexistente no cae en "la primera": `run` la busca por nombre.
        assert!(crate::cli::say::run("hola", None, 1.0, Some("voz-inexistente")).is_err());
    }
}
