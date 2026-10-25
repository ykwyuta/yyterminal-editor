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
        "║ Terminal Markdown preview ║",
        "9. Numbering can start at nine.",
        "◦ A nested bullet",
        "▪ A deeper bullet",
        "[x] Finished task",
        "[ ] Pending task",
        "▎ Detail with a vertical marker",
        "·· Smallest heading",
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

#[test]
fn standalone_large_markdown_prints_table_after_former_limits() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("large.md");
    let source = "ordinary content beyond former preview limits\n".repeat(70_000)
        + "\n| Item | Quantity |\n|---|---:|\n| FINAL_ROW | 123456 |\n";
    std::fs::write(&path, source)?;
    let output = Command::new(env!("CARGO_BIN_EXE_markdownViewer"))
        .args(["--plain", "--width", "320"])
        .arg(path)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout)?;
    assert!(text.contains("FINAL_ROW") && text.contains("123456") && text.contains('╞'));
    assert!(text.lines().count() > 65_535);
    assert!(!text.contains("limit reached") && !text.contains("truncated"));
    Ok(())
}

#[test]
fn syntax_colors_code_fences_and_plain_output_keeps_source_text() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("syntax.md");
    std::fs::write(&path, "```rust\nlet text = \"日本語\"; // note\n```\n")?;
    let colored = Command::new(env!("CARGO_BIN_EXE_markdownViewer"))
        .args(["--print", "--color"])
        .arg(&path)
        .output()?;
    assert!(colored.status.success());
    let output = String::from_utf8(colored.stdout)?;
    for color in ["198;120;221", "152;195;121", "127;135;145"] {
        assert!(
            output.contains(&format!("\u{1b}[38;2;{color}m")),
            "{output}"
        );
    }
    let plain = Command::new(env!("CARGO_BIN_EXE_markdownViewer"))
        .arg("--plain")
        .arg(&path)
        .output()?;
    assert!(plain.status.success());
    let output = String::from_utf8(plain.stdout)?;
    assert!(output.contains("let text = \"日本語\"; // note"));
    assert!(!output.contains('\u{1b}'));
    Ok(())
}
