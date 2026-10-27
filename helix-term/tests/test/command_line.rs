use super::*;

use helix_core::diagnostic::Severity;

#[tokio::test(flavor = "multi_thread")]
async fn history_completion() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":asdf<ret>:theme d<C-n><tab>"),
        Some(&|app| {
            assert!(!app.editor.is_err());
        }),
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn prompt_reset_anchor() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":string wider than the terminal window causing the anchor location to be non zero which would panic when the line is deleted<C-u>"),
        Some(&|app| {
            assert!(!app.editor.is_err());
        }),
        false,
    )
    .await?;

    Ok(())
}

async fn test_statusline(
    line: &str,
    expected_status: &str,
    expected_severity: Severity,
) -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(&format!("{line}<ret>")),
        Some(&|app| {
            let (status, &severity) = app.editor.get_status().unwrap();
            assert_eq!(
                severity, expected_severity,
                "'{line}' printed {severity:?}: {status}"
            );
            assert_eq!(status.as_ref(), expected_status);
        }),
        false,
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn variable_expansion() -> anyhow::Result<()> {
    test_statusline(r#":echo %{cursor_line}"#, "1", Severity::Info).await?;
    // Double quotes can be used with expansions:
    test_statusline(
        r#":echo "line%{cursor_line}line""#,
        "line1line",
        Severity::Info,
    )
    .await?;
    // Within double quotes you can escape the percent token for an expansion by doubling it.
    test_statusline(
        r#":echo "%%{cursor_line}""#,
        "%{cursor_line}",
        Severity::Info,
    )
    .await?;
    // Single-letter short names, bare or delimited:
    test_statusline(r#":echo "%l:%c""#, "1:1", Severity::Info).await?;
    test_statusline(r#":echo "%{l}x""#, "1x", Severity::Info).await?;
    test_statusline(r#":echo %t"#, "text", Severity::Info).await?;
    // Shell-quoted variables, and bare `%q` for the quoted selection:
    test_statusline(r#":echo %q{t}"#, "'text'", Severity::Info).await?;
    test_key_sequence(
        &mut AppBuilder::new()
            .with_input_text("#[it's $HOME|]#")
            .build()?,
        Some(":echo %q<ret>"),
        Some(&|app| {
            let (status, _) = app.editor.get_status().unwrap();
            assert_eq!(status.as_ref(), r"'it'\''s $HOME'");
        }),
        false,
    )
    .await?;
    test_statusline(
        r#":echo %x"#,
        "'echo': unknown variable 'x'",
        Severity::Error,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn unicode_expansion() -> anyhow::Result<()> {
    test_statusline(r#":echo %u{20}"#, " ", Severity::Info).await?;
    test_statusline(r#":echo %u{0020}"#, " ", Severity::Info).await?;
    test_statusline(r#":echo %u{25CF}"#, "●", Severity::Info).await?;
    // Not a valid Unicode codepoint:
    test_statusline(
        r#":echo %u{deadbeef}"#,
        "'echo': could not interpret 'deadbeef' as a Unicode character code",
        Severity::Error,
    )
    .await?;

    Ok(())
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn shell_expansion() -> anyhow::Result<()> {
    test_statusline(
        r#":echo %sh{echo "hello world"}"#,
        "hello world",
        Severity::Info,
    )
    .await?;

    // Shell expansion is recursive.
    test_statusline(":echo %sh{echo '%{cursor_line}'}", "1", Severity::Info).await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn register_expansion() -> anyhow::Result<()> {
    test_statusline(
        r#":set-register a hello world<ret>:echo %reg{a}"#,
        "hello world",
        Severity::Info,
    )
    .await?;
    test_statusline(r#":echo %reg{a}"#, "", Severity::Info).await?;
    test_statusline(
        r#":echo %reg{abc}"#,
        "'echo': Invalid register `abc`: should only be a single character",
        Severity::Error,
    )
    .await?;

    // Register expansion evaluation is *not* recursive.
    test_statusline(
        r#":set-register a b<ret>:set-register b hello<ret>:echo %reg{%reg{a}}"#,
        "'echo': Invalid register `%reg{a}`: should only be a single character",
        Severity::Error,
    )
    .await?;
    test_statusline(
        r#":set-register a hello<ret>:set-register b %%reg{a}<ret>:echo %reg{b}"#,
        "%reg{a}",
        Severity::Info,
    )
    .await?;

    // However, you can copy the contents of one register into another with this expansion if you
    // want to.
    test_statusline(
        r#":set-register a hello<ret>:set-register b %reg{a}<ret>:echo %reg{b}"#,
        "hello",
        Severity::Info,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn percent_escaping() -> anyhow::Result<()> {
    // A lone percent is the file path, which is the working directory for a scratch buffer.
    let cwd = helix_stdx::env::current_working_dir();
    let cwd = cwd.to_string_lossy();
    test_statusline(r#":echo %"#, &cwd, Severity::Info).await?;
    test_statusline(r#":echo %sh{echo "%"}"#, &cwd, Severity::Info).await?;
    test_statusline(r#":echo %sh{echo 10%%}"#, "10%", Severity::Info).await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn run_command() -> anyhow::Result<()> {
    test_statusline(
        ":run",
        "'run': No run command for this file. Set one with `:sr <command>` or in [editor.run-commands]",
        Severity::Error,
    )
    .await?;

    // The command is stored as written, to be expanded when it runs.
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":sr echo %f % \"%{cursor_line}\"<ret>"),
        Some(&|app| {
            assert_eq!(
                app.editor.run_command.as_deref(),
                Some(r#"echo %f % "%{cursor_line}""#)
            );
        }),
        false,
    )
    .await?;

    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":sr echo hi<ret>:sr --clear<ret>"),
        Some(&|app| {
            assert_eq!(app.editor.run_command, None);
            let (status, _) = app.editor.get_status().unwrap();
            assert_eq!(status, "Run command cleared");
        }),
        false,
    )
    .await?;

    // `#` runs it, expanding variables first.
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":sr echo %l<ret>#"),
        Some(&|app| {
            let (status, severity) = app.editor.get_status().unwrap();
            assert_eq!(*severity, Severity::Info);
            assert!(status.ends_with(": echo 1"), "{status}");
        }),
        false,
    )
    .await?;

    // Without `:set-run-command` the command comes from `editor.run-commands`.
    let file = tempfile::Builder::new().suffix(".md").tempfile()?;
    let mut config = test_config();
    config
        .editor
        .run_commands
        .insert("md".to_string(), "echo %l".to_string());
    test_key_sequence(
        &mut AppBuilder::new()
            .with_file(file.path(), None)
            .with_config(config)
            .build()?,
        Some(":run<ret>"),
        Some(&|app| {
            assert_eq!(
                app.editor.run_command(),
                Some(("echo %l".to_string(), "editor.run-commands.md".to_string()))
            );
            let (status, _) = app.editor.get_status().unwrap();
            assert!(status.ends_with(": echo 1"), "{status}");
        }),
        false,
    )
    .await?;

    Ok(())
}
