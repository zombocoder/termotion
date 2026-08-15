mod commands;
mod pipeline;
mod report;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "termotion",
    about = "Declarative terminal animation generator",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate a scenario file
    Validate {
        file: PathBuf,
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Print the compiled timeline
    Inspect { file: PathBuf },
    /// Render a scenario to media
    Render {
        file: PathBuf,
        /// Output file (or, for `--format png`, directory) path. Overrides
        /// the scenario's `output.path`; defaults to `<name>.<format>` in the
        /// project's output directory (or the current directory) when unset.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Container/output format: webm, mp4, or png. Overrides the
        /// scenario's `output.format`; defaults to webm.
        #[arg(long)]
        format: Option<String>,
        /// Canvas width in pixels. Overrides the scenario's `canvas.width`;
        /// defaults to 1920.
        #[arg(long)]
        width: Option<u32>,
        /// Canvas height in pixels. Overrides the scenario's `canvas.height`;
        /// defaults to 1080.
        #[arg(long)]
        height: Option<u32>,
        /// Frame rate in frames per second. Overrides the scenario's
        /// `canvas.fps`; defaults to 30.
        #[arg(long)]
        fps: Option<u32>,
        /// Named color theme to apply. Overrides the scenario's `theme`.
        #[arg(long)]
        theme: Option<String>,
        /// Background color, as a palette name or hex value (e.g. `#080B09`).
        /// Overrides the scenario's `canvas.background`.
        #[arg(long)]
        background: Option<String>,
        /// Render with a transparent background instead of a solid one. NOT
        /// YET SUPPORTED for WebM/MP4 output: it fails cleanly at render
        /// time rather than silently producing an opaque file (alpha support
        /// for those formats lands in a later milestone). PNG sequence
        /// output does support it.
        #[arg(long)]
        transparent: bool,
        /// Loop the scenario's playback. Overrides the scenario's
        /// `playback.looping`.
        #[arg(long = "loop")]
        looping: bool,
        /// Encoder CRF quality (lower is higher quality; the valid range
        /// depends on the codec: 0-63 for VP9/webm, 0-51 for H.264/mp4).
        /// Overrides the scenario's `output.quality`; defaults to 32.
        #[arg(long)]
        quality: Option<u8>,
        /// Replace an existing output file or directory instead of failing
        /// with `OUTPUT_EXISTS`.
        #[arg(long)]
        overwrite: bool,
        /// Suppress the progress line and the final summary line.
        #[arg(short, long)]
        quiet: bool,
    },
    /// Inspect and manage themes
    Themes {
        #[command(subcommand)]
        action: ThemeAction,
    },
    /// Check external dependencies
    Doctor,
    /// Show available fonts, or what a scenario resolves to
    Fonts {
        #[arg(long)]
        scenario: Option<PathBuf>,
    },
    /// Generate a shell completion script
    Completions { shell: clap_complete::Shell },
    /// Print the version
    Version,
}

#[derive(Subcommand)]
enum ThemeAction {
    /// List available themes
    List,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let code = match cli.command {
        Command::Validate { file, json } => commands::validate::run(&file, json),
        Command::Inspect { file } => commands::inspect::run(&file),
        Command::Render {
            file,
            output,
            format,
            width,
            height,
            fps,
            theme,
            background,
            transparent,
            looping,
            quality,
            overwrite,
            quiet,
        } => commands::render::run(commands::render::RenderArgs {
            file,
            overrides: termotion_schema::resolve::Overrides {
                width,
                height,
                fps,
                theme,
                background,
                // Absent-or-true: a bare `--transparent` sets `Some(true)`, but its
                // absence must stay `None` rather than becoming `Some(false)`, so a
                // scenario that sets `canvas.transparent: true` is not silently
                // overridden by an unrelated CLI invocation (see `Overrides`' `.or`
                // chain in `resolve.rs`, which treats `None` as "no opinion").
                transparent: transparent.then_some(true),
                looping: looping.then_some(true),
                quality,
                format,
                output,
                overwrite,
            },
            quiet,
        }),
        Command::Themes { action } => match action {
            ThemeAction::List => commands::themes::list(),
        },
        Command::Doctor => commands::doctor::run(),
        Command::Fonts { scenario } => commands::fonts::run(scenario.as_deref()),
        Command::Completions { shell } => commands::completions::run(shell),
        Command::Version => commands::version::run(),
    };

    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
