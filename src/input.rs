//! Bounded input batching without losing command or mouse event ordering.
use crossterm::event::{self, Event, MouseEventKind};
use std::{
    io,
    time::{Duration, Instant},
};

const TEXT_BATCH_LIMIT: usize = 64 * 1024;

fn poll_input(timeout: Duration) -> io::Result<bool> {
    // The Unix poll backend checks its deadline before buffered events. Give it
    // a nonzero timeout; idle text input gains at most 1 ms of batching latency.
    #[cfg(unix)]
    let timeout = timeout.max(Duration::from_millis(1));
    event::poll(timeout)
}

fn text_key(event: &Event) -> Option<char> {
    use event::{KeyCode, KeyEventKind, KeyModifiers};
    let Event::Key(key) = event else { return None };
    if key.kind == KeyEventKind::Release || !(key.modifiers - KeyModifiers::SHIFT).is_empty() {
        return None;
    }
    match key.code {
        KeyCode::Char(c) if !c.is_control() => Some(c),
        KeyCode::Enter => Some('\n'),
        // Keep the editor's existing four-space Tab behavior.
        KeyCode::Tab => Some('\t'),
        _ => None,
    }
}
#[derive(Default)]
pub struct Queue {
    pending: Option<Event>,
}
fn wheel(event: &Event) -> bool {
    matches!(event, Event::Mouse(m) if matches!(m.kind,MouseEventKind::ScrollUp|MouseEventKind::ScrollDown|MouseEventKind::ScrollLeft|MouseEventKind::ScrollRight))
}
fn same_wheel(a: &Event, b: &Event) -> bool {
    wheel(a) && a == b
}
impl Queue {
    /// Coalesce already available text keys only when the caller is editing text.
    /// A lone key keeps its normal editing/undo semantics; commands remain pending.
    pub fn text_burst(&mut self, first: &Event) -> io::Result<Option<String>> {
        self.text_burst_with(first, poll_input, event::read)
    }

    fn text_burst_with(
        &mut self,
        first: &Event,
        mut poll: impl FnMut(Duration) -> io::Result<bool>,
        mut read: impl FnMut() -> io::Result<Event>,
    ) -> io::Result<Option<String>> {
        let Some(c) = text_key(first) else {
            return Ok(None);
        };
        let mut text = if c == '\t' {
            "    ".into()
        } else {
            c.to_string()
        };
        let deadline = Instant::now() + Duration::from_millis(8);
        let mut count = 1;
        while text.len() < TEXT_BATCH_LIMIT && Instant::now() < deadline {
            let next = if let Some(event) = self.pending.take() {
                event
            } else if poll(Duration::ZERO)? {
                read()?
            } else {
                break;
            };
            // Windows can interleave a release record after every pasted key.
            // App::key ignores releases, so they must not split a text burst.
            if matches!(&next, Event::Key(key) if key.kind == event::KeyEventKind::Release) {
                continue;
            }
            if let Some(c) = text_key(&next) {
                if c == '\t' {
                    text.push_str("    ")
                } else {
                    text.push(c)
                }
                count += 1;
            } else {
                self.pending = Some(next);
                break;
            }
        }
        Ok((count > 1).then_some(text))
    }

    pub fn read(&mut self, timeout: Duration) -> io::Result<Option<(Event, usize)>> {
        self.read_with(timeout, poll_input, event::read)
    }
    fn read_with(
        &mut self,
        timeout: Duration,
        mut poll: impl FnMut(Duration) -> io::Result<bool>,
        mut read: impl FnMut() -> io::Result<Event>,
    ) -> io::Result<Option<(Event, usize)>> {
        let first = if let Some(event) = self.pending.take() {
            event
        } else {
            if !poll(timeout)? {
                return Ok(None);
            }
            read()?
        };
        let mut count = 1;
        if wheel(&first) {
            // Bound work even if a touchpad continuously produces events.
            while count < 64 && poll(Duration::ZERO)? {
                let next = read()?;
                if same_wheel(&first, &next) {
                    count += 1;
                } else {
                    self.pending = Some(next);
                    break;
                }
            }
        }
        Ok(Some((first, count)))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
    use std::{cell::RefCell, collections::VecDeque};
    #[test]
    fn text_bursts_preserve_unicode_newlines_tabs_and_command_order() {
        let key = |code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let save = Event::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        let mut released = KeyEvent::new(KeyCode::Char('日'), KeyModifiers::NONE);
        released.kind = event::KeyEventKind::Release;
        let events = RefCell::new(VecDeque::from([
            Event::Key(released),
            key(KeyCode::Enter),
            key(KeyCode::Tab),
            key(KeyCode::Char('語')),
            save.clone(),
            key(KeyCode::Char('x')),
        ]));
        let mut queue = Queue::default();
        let text = queue
            .text_burst_with(
                &key(KeyCode::Char('日')),
                |_| Ok(!events.borrow().is_empty()),
                || Ok(events.borrow_mut().pop_front().unwrap()),
            )
            .unwrap();
        assert_eq!(text.as_deref(), Some("日\n    語"));
        assert_eq!(
            queue
                .read_with(Duration::ZERO, |_| Ok(false), || unreachable!())
                .unwrap(),
            Some((save, 1))
        );
        assert_eq!(events.borrow().len(), 1);
        assert_eq!(
            queue
                .text_burst_with(&key(KeyCode::Enter), |_| Ok(false), || unreachable!())
                .unwrap(),
            None
        );
        let mut release = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        release.kind = event::KeyEventKind::Release;
        let release = Event::Key(release);
        assert_eq!(
            queue
                .text_burst_with(&release, |_| unreachable!(), || unreachable!())
                .unwrap(),
            None
        );
    }

    #[test]
    fn large_text_bursts_are_bounded_and_lossless() {
        let key = Event::Key(KeyEvent::new(KeyCode::Char('界'), KeyModifiers::NONE));
        let events = RefCell::new(VecDeque::from(vec![key; TEXT_BATCH_LIMIT]));
        let mut queue = Queue::default();
        let mut actual = String::new();
        let mut batches = 0;
        loop {
            let Some(first) = events.borrow_mut().pop_front() else {
                break;
            };
            let text = queue
                .text_burst_with(
                    &first,
                    |_| Ok(!events.borrow().is_empty()),
                    || Ok(events.borrow_mut().pop_front().unwrap()),
                )
                .unwrap()
                .unwrap_or_else(|| "界".into());
            assert!(text.len() <= TEXT_BATCH_LIMIT + 3);
            actual.push_str(&text);
            batches += 1;
        }
        assert_eq!(actual, "界".repeat(TEXT_BATCH_LIMIT));
        assert!(batches > 1);
    }

    #[test]
    fn wheel_bursts_preserve_distance_keys_click_order_and_batch_bound() {
        let mouse = Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        let key = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let events = RefCell::new(VecDeque::from(vec![mouse.clone(); 70]));
        let click = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(event::MouseButton::Left),
            column: 10,
            row: 5,
            modifiers: KeyModifiers::NONE,
        });
        let mut reversed = mouse.clone();
        if let Event::Mouse(m) = &mut reversed {
            m.kind = MouseEventKind::ScrollUp;
        }
        events.borrow_mut().push_back(key.clone());
        events.borrow_mut().push_back(click.clone());
        events.borrow_mut().push_back(mouse.clone());
        events.borrow_mut().push_back(reversed.clone());
        let mut queue = Queue::default();
        let mut next = || {
            queue
                .read_with(
                    Duration::ZERO,
                    |_| Ok(!events.borrow().is_empty()),
                    || Ok(events.borrow_mut().pop_front().unwrap()),
                )
                .unwrap()
        };
        assert_eq!(next(), Some((mouse.clone(), 64)));
        assert_eq!(next(), Some((mouse.clone(), 6)));
        assert_eq!(next(), Some((key, 1)));
        assert_eq!(next(), Some((click, 1)));
        assert_eq!(next(), Some((mouse, 1)));
        assert_eq!(next(), Some((reversed, 1)));
        assert_eq!(next(), None);
    }
}
