//! The shell integration that defines the `m` function.

use crate::completions::CompletionShell;

const BASH: &str = r#"m() {
    local dir
    dir="$(command mekle "$@" | fzf)" || return
    [[ -n "$dir" ]] || return
    if [[ "$dir" == "~" || "$dir" == "~/"* ]]; then
        dir="${HOME}${dir#\~}"
    fi
    cd -- "$dir" || return
    command mekle add "$PWD"
}
"#;

const ELVISH: &str = r"use re

fn m {|@args|
    var dir = (e:mekle $@args | e:fzf)
    if (eq $dir '') { return }
    if (re:match '^~(/|$)' $dir) {
        set dir = (re:replace '^~' $E:HOME $dir)
    }
    cd $dir
    e:mekle add $pwd
}
";

const FISH: &str = r#"function m
    set -l dir (command mekle $argv | fzf)
    or return
    test -n "$dir"
    or return
    if string match -qr '^~(/|$)' -- "$dir"
        set dir (string replace -r '^~' "$HOME" -- "$dir")
    end
    cd -- "$dir"
    or return
    command mekle add "$PWD"
end
"#;

const ZSH: &str = r#"function m() {
    local dir
    dir="$(command mekle "$@" | fzf)" || return
    [[ -n "$dir" ]] || return
    if [[ "$dir" == "~" || "$dir" == "~/"* ]]; then
        dir="${HOME}${dir#\~}"
    fi
    builtin cd -- "$dir" || return
    command mekle add "$PWD"
}
"#;

impl CompletionShell {
    /// Returns the shell integration script that defines `m`.
    #[must_use]
    pub const fn init(self) -> &'static str {
        match self {
            Self::Bash => BASH,
            Self::Elvish => ELVISH,
            Self::Fish => FISH,
            Self::Zsh => ZSH,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_script_defines_m_and_records_the_selected_directory() {
        for (shell, declaration) in [
            (CompletionShell::Bash, "m() {"),
            (CompletionShell::Elvish, "fn m {|@args|"),
            (CompletionShell::Fish, "function m"),
            (CompletionShell::Zsh, "function m() {"),
        ] {
            let script = shell.init();

            assert!(script.contains(declaration));
            assert!(script.contains("fzf"));
            assert!(script.contains("mekle add"));
            assert!(!script.contains("pf"));
        }
    }
}
