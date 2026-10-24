use anyhow::{Result, bail};
use std::{path::Path, process::Command};

pub fn run(root: &Path, args: &[&str]) -> Result<String> {
    run_command(root, args, false)
}

fn run_command(root: &Path, args: &[&str], isolated: bool) -> Result<String> {
    let mut command = Command::new("git");
    if isolated {
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_CONFIG_COUNT", "0");
    }
    let output = command
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    fn run(root: &std::path::Path, args: &[&str]) -> anyhow::Result<String> {
        super::run_command(root, args, true)
    }
    #[test]
    fn real_git_stage_and_commit() -> anyhow::Result<()> {
        let root = tempfile::tempdir()?;
        run(root.path(), &["init", "--template="])?;
        run(root.path(), &["config", "user.name", "YY test"])?;
        run(root.path(), &["config", "user.email", "yy@example.invalid"])?;
        run(root.path(), &["config", "core.autocrlf", "false"])?;
        run(root.path(), &["config", "core.eol", "lf"])?;
        run(root.path(), &["config", "commit.gpgsign", "false"])?;
        std::fs::write(root.path().join("日本 語.txt"), b"test\n")?;
        assert!(run(root.path(), &["status", "--short"])?.contains("??"));
        run(root.path(), &["add", "--all"])?;
        run(
            root.path(),
            &["-c", "core.hooksPath=", "commit", "-m", "Initial"],
        )?;
        assert!(run(root.path(), &["status", "--short"])?.is_empty());
        Ok(())
    }
}
