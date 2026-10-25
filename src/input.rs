//! Bounded wheel batching: no input loss and no stale hit-testing after scrolling.
use crossterm::event::{self, Event, MouseEventKind};
use std::{io, time::Duration};
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
    pub fn read(&mut self, timeout: Duration) -> io::Result<Option<(Event, usize)>> {
        self.read_with(timeout, event::poll, event::read)
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
