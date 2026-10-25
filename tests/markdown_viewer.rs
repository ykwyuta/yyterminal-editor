use anyhow::Result;
use std::{
    io::Write,
    process::{Command, Stdio},
};
#[test]
fn standalone_plain_color_and_stdin_share_rendering() -> Result<()> {
    let demo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/markdown-preview.md");
    let exe = env!("CARGO_BIN_EXE_markdownViewer");
    let output = Command::new(exe)
        .args(["--plain", "--width", "80"])
        .arg(&demo)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout)?;
    for expected in [
        "Pixel house",
        "40x28",
        "Connections:",
        "検証",
        "Open Markdown",
        "customer_id FK",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
    assert!(!text.contains('\u{1b}'));
    let output = Command::new(exe)
        .args(["--print", "--color"])
        .arg(demo)
        .output()?;
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout)?;
    assert!(text.contains("\u{1b}[38;2;"));
    assert!(text.contains('▀'));
    let mut child = Command::new(exe)
        .args(["--plain", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    let mut input = child.stdin.take().unwrap();
    input.write_all(b"# stdin\n\n```mermaid\nflowchart LR\n A-->B\n```\n")?;
    drop(input);
    let output = child.wait_with_output()?;
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout)?;
    assert!(text.contains("stdin") && text.contains("Connections:"));
    Ok(())
}
#[test]
fn invalid_options_and_missing_input_fail_without_a_terminal() -> Result<()> {
    let exe = env!("CARGO_BIN_EXE_markdownViewer");
    for args in [
        vec!["--width", "0", "-"],
        vec!["--unknown"],
        vec!["missing-file.md"],
    ] {
        assert!(!Command::new(exe).args(args).output()?.status.success());
    }
    assert!(Command::new(exe).arg("--help").output()?.status.success());
    Ok(())
}
