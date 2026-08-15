use clap::CommandFactory;

use crate::Cli;

/// Generate a shell completion script for `shell` on stdout.
pub fn run(shell: clap_complete::Shell) -> i32 {
    let mut command = Cli::command();
    let name = command.get_name().to_string();
    clap_complete::generate(shell, &mut command, name, &mut std::io::stdout());
    0
}
