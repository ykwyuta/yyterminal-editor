use anyhow::Result;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};
use std::{
    io::{Read, Write},
    path::Path,
    sync::mpsc::{self, Receiver},
    thread,
};

pub struct Shell {
    pub screen: yy_term::Terminal,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    rx: Receiver<Vec<u8>>,
    size: (u16, u16),
}
impl Shell {
    pub fn new(cwd: &Path) -> Result<Self> {
        let pair = portable_pty::native_pty_system().openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let program = std::env::var_os("SHELL").unwrap_or_else(|| {
            if cfg!(windows) {
                "cmd.exe".into()
            } else {
                "/bin/sh".into()
            }
        });
        let mut command = CommandBuilder::new(program);
        command.cwd(cwd);
        command.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let (tx, rx) = mpsc::sync_channel(32);
        thread::spawn(move || {
            let mut buf = [0; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            screen: yy_term::Terminal::new(80, 24, 2000),
            master: pair.master,
            child,
            writer,
            rx,
            size: (80, 24),
        })
    }
    pub fn poll(&mut self) -> Result<()> {
        for bytes in self.rx.try_iter().take(64) {
            self.screen.feed(&bytes);
        }
        let replies = self.screen.take_responses();
        if !replies.is_empty() {
            self.send(&replies)?;
        }
        Ok(())
    }
    pub fn send(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        let size = (cols.max(1), rows.max(1));
        if self.size != size {
            self.master.resize(PtySize {
                cols: size.0,
                rows: size.1,
                pixel_width: 0,
                pixel_height: 0,
            })?;
            self.screen.resize(size.0 as usize, size.1 as usize);
            self.size = size;
        }
        Ok(())
    }
}
impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[cfg(test)]
mod tests {
    #[test]
    #[cfg(unix)]
    fn real_pty_round_trip_and_resize() -> anyhow::Result<()> {
        let mut shell = super::Shell::new(std::path::Path::new("/tmp"))?;
        shell.resize(100, 30)?;
        // Marker is assembled by the shell; the echoed input cannot satisfy the check.
        shell.send(b"printf '\\131\\131\\120\\124\\131\\117\\113\\n'\r")?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            shell.poll()?;
            if shell.screen.screen_text().contains("YYPTYOK") {
                break;
            }
            anyhow::ensure!(
                std::time::Instant::now() < deadline,
                "PTY output: {}",
                shell.screen.screen_text()
            );
            std::thread::yield_now();
        }
        assert_eq!(shell.screen.cols(), 100);
        Ok(())
    }
}
