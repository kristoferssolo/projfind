use clap::{Command, ValueEnum};
use clap_complete::{Shell, generate as generate_for};
use std::io::Write;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    Zsh,
}

/// Writes a completion script for `shell`.
pub fn generate(shell: CompletionShell, command: &mut Command, output: &mut dyn Write) {
    generate_for(Shell::from(shell), command, "projfind", output);
}

impl From<CompletionShell> for Shell {
    fn from(shell: CompletionShell) -> Self {
        match shell {
            CompletionShell::Bash => Self::Bash,
            CompletionShell::Elvish => Self::Elvish,
            CompletionShell::Fish => Self::Fish,
            CompletionShell::Zsh => Self::Zsh,
        }
    }
}
