//! Helpers for running commands through the configured shell program (`editor.shell`).

/// The family of a shell program, which decides how arguments are quoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    /// `sh`, `bash`, `zsh`, `dash`, `ksh` and anything unrecognized.
    Posix,
    Fish,
    Nu,
    /// `pwsh` or Windows PowerShell.
    PowerShell,
    /// Windows `cmd.exe`.
    Cmd,
}

impl ShellKind {
    /// Detects the shell family from the shell program, for example `/bin/zsh`, `pwsh` or
    /// `C:\Windows\System32\cmd.exe`.
    pub fn detect(program: &str) -> Self {
        let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
        let name = name.to_ascii_lowercase();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        match name {
            "fish" => Self::Fish,
            "nu" => Self::Nu,
            "pwsh" | "powershell" => Self::PowerShell,
            "cmd" => Self::Cmd,
            _ => Self::Posix,
        }
    }

    /// Quotes `s` so that this shell passes it to a program as exactly one literal argument.
    pub fn quote(self, s: &str) -> String {
        match self {
            // Nothing is special within single quotes, a quote is written as `'\''`.
            Self::Posix => format!("'{}'", s.replace('\'', r"'\''")),
            // Within single quotes fish treats `\'` and `\\` as escapes.
            Self::Fish => format!("'{}'", s.replace('\\', r"\\").replace('\'', r"\'")),
            // Raw strings `r#'...'#` take everything literally until the closing `'#`.
            Self::Nu => {
                let mut hashes = "#".to_string();
                while s.contains(&format!("'{hashes}")) {
                    hashes.push('#');
                }
                format!("r{hashes}'{s}'{hashes}")
            }
            // Single quotes are escaped by doubling. PowerShell also treats the typographic
            // single quotes as quote characters.
            Self::PowerShell => {
                let mut quoted = String::with_capacity(s.len() + 2);
                quoted.push('\'');
                for ch in s.chars() {
                    if matches!(ch, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
                        quoted.push(ch);
                    }
                    quoted.push(ch);
                }
                quoted.push('\'');
                quoted
            }
            // Double quotes protect `&`, `|`, `<`, `>` and `^`, and are escaped by doubling.
            // `%VAR%` is expanded even within quotes, so each `%` is placed outside of the quotes
            // where `^` escapes it: `a%b` becomes `"a"^%"b"`, which programs read as `a%b`.
            // Programs read backslashes before a quote as escapes, so those are doubled.
            Self::Cmd => {
                let mut quoted = String::with_capacity(s.len() + 2);
                quoted.push('"');
                let mut backslashes = 0;
                for ch in s.chars() {
                    let escaped = match ch {
                        '\\' => {
                            backslashes += 1;
                            quoted.push(ch);
                            continue;
                        }
                        '"' => "\"\"",
                        '%' => "\"^%\"",
                        _ => {
                            backslashes = 0;
                            quoted.push(ch);
                            continue;
                        }
                    };
                    quoted.extend(std::iter::repeat_n('\\', backslashes));
                    quoted.push_str(escaped);
                    backslashes = 0;
                }
                quoted.extend(std::iter::repeat_n('\\', backslashes));
                quoted.push('"');
                quoted
            }
        }
    }

    /// The command line argument for running `command` with this shell, when it must be passed
    /// verbatim instead of escaped by the standard library.
    ///
    /// `cmd.exe` doesn't follow the usual Windows argument escaping rules: escaping would turn
    /// every `"` in the command into `\"`. Wrapping the command in quotes makes `cmd /C` strip
    /// just those outer quotes and run the rest as typed.
    pub fn verbatim_command(self, command: &str) -> Option<String> {
        (self == Self::Cmd).then(|| format!("\"{command}\""))
    }
}

#[cfg(test)]
mod tests {
    use super::ShellKind;

    #[test]
    fn detect() {
        assert_eq!(ShellKind::detect("sh"), ShellKind::Posix);
        assert_eq!(ShellKind::detect("/usr/bin/zsh"), ShellKind::Posix);
        assert_eq!(ShellKind::detect("/opt/homebrew/bin/fish"), ShellKind::Fish);
        assert_eq!(ShellKind::detect("nu"), ShellKind::Nu);
        assert_eq!(ShellKind::detect("pwsh"), ShellKind::PowerShell);
        assert_eq!(ShellKind::detect("powershell.exe"), ShellKind::PowerShell);
        assert_eq!(ShellKind::detect("cmd"), ShellKind::Cmd);
        assert_eq!(
            ShellKind::detect(r"C:\Windows\System32\CMD.EXE"),
            ShellKind::Cmd
        );
    }

    #[test]
    fn quote() {
        let s = r#"it's "$HOME" \ 100% & more"#;
        assert_eq!(
            ShellKind::Posix.quote(s),
            r#"'it'\''s "$HOME" \ 100% & more'"#
        );
        assert_eq!(
            ShellKind::Fish.quote(s),
            r#"'it\'s "$HOME" \\ 100% & more'"#
        );
        assert_eq!(
            ShellKind::Nu.quote(s),
            r#"r#'it's "$HOME" \ 100% & more'#"#
        );
        assert_eq!(ShellKind::Nu.quote("a'#b"), "r##'a'#b'##");
        assert_eq!(
            ShellKind::PowerShell.quote(s),
            r#"'it''s "$HOME" \ 100% & more'"#
        );
        assert_eq!(ShellKind::PowerShell.quote("it’s"), "'it’’s'");
        assert_eq!(
            ShellKind::Cmd.quote(s),
            r#""it's ""$HOME"" \ 100"^%" & more""#
        );
        assert_eq!(ShellKind::Posix.quote(""), "''");
        assert_eq!(ShellKind::Cmd.quote(""), r#""""#);
        assert_eq!(ShellKind::Cmd.quote(r"C:\dir\"), r#""C:\dir\\""#);
        assert_eq!(ShellKind::Cmd.quote(r#"a\"b"#), r#""a\\""b""#);
    }
}
