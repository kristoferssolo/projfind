use clap::{Command, ValueEnum, ValueHint};
use clap_complete::{Shell, generate as generate_for};
use std::io::{self, Write};

const FISH_DISABLE_ROOT_PATH_COMPLETION: &str = concat!(
    r#"complete -c mekle -n "__fish_mekle_needs_command" -f"#,
    "\n",
);
const BASH_SCOPE_PATH_COMPLETION: &str = r#"
__mekle_complete() {
    _mekle "$@"

    if [[ "${COMP_WORDS[1]}" == "add" && ${COMP_CWORD} -eq 2 ]] ||
       [[ "${COMP_WORDS[1]}" == "history" && ${COMP_CWORD} -eq 3 &&
          " show set adjust remove " == *" ${COMP_WORDS[2]} "* ]]; then
        COMPREPLY+=( $(compgen -f -- "${COMP_WORDS[COMP_CWORD]}") )
    fi
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F __mekle_complete -o nosort mekle
else
    complete -F __mekle_complete mekle
fi
"#;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    Zsh,
}

/// Writes a completion script for `shell`.
///
/// # Errors
///
/// Returns an error when the generated script cannot be written.
pub fn generate(
    shell: CompletionShell,
    command: &mut Command,
    output: &mut dyn Write,
) -> io::Result<()> {
    let mut completion_command = command
        .clone()
        .mut_arg("paths", |arg| arg.value_hint(ValueHint::Other));
    generate_for(Shell::from(shell), &mut completion_command, "mekle", output);

    match shell {
        CompletionShell::Bash => output.write_all(BASH_SCOPE_PATH_COMPLETION.as_bytes())?,
        CompletionShell::Fish => {
            output.write_all(FISH_DISABLE_ROOT_PATH_COMPLETION.as_bytes())?;
        }
        CompletionShell::Elvish | CompletionShell::Zsh => {}
    }

    Ok(())
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
