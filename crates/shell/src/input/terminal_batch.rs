//! Bounded acquisition of already parsed Crossterm events. No byte parsing here.
use crossterm::event;
use futures_core::Stream;
use std::{io, time::Duration};
use std::{
    pin::Pin,
    task::{Context, Poll, Waker},
};

const MAX_READY_TERMINAL_EVENTS_PER_FRAME: usize = 4_096;
const MAX_READY_CHILD_TERMINAL_EVENTS_PER_FRAME: usize = 256;

pub(crate) struct TerminalEvents {
    stream: Option<event::EventStream>,
    pending: Option<event::Event>,
    waker: Waker,
}

impl TerminalEvents {
    pub(crate) fn new() -> Self {
        Self {
            stream: Some(event::EventStream::new()),
            pending: None,
            waker: crate::process_host::event_loop::waker(),
        }
    }

    pub(crate) fn suspend(&mut self) {
        self.stream = None;
        self.pending = None;
    }

    pub(crate) fn resume(&mut self) {
        self.stream = Some(event::EventStream::new());
    }

    fn ready(&mut self) -> io::Result<bool> {
        if self.pending.is_some() {
            return Ok(true);
        }
        let Some(stream) = &mut self.stream else {
            return Ok(false);
        };
        let mut context = Context::from_waker(&self.waker);
        match Pin::new(stream).poll_next(&mut context) {
            Poll::Ready(Some(Ok(event))) => {
                self.pending = Some(event);
                Ok(true)
            }
            Poll::Ready(Some(Err(error))) => Err(error),
            Poll::Ready(None) => Err(io::ErrorKind::UnexpectedEof.into()),
            Poll::Pending => Ok(false),
        }
    }

    pub(crate) fn poll(&mut self, generation: u64, timeout: Duration) -> io::Result<bool> {
        if self.ready()? {
            return Ok(true);
        }
        crate::process_host::event_loop::wait_since(generation, timeout);
        self.ready()
    }

    pub(crate) fn read(&mut self) -> io::Result<event::Event> {
        self.pending
            .take()
            .ok_or_else(|| io::ErrorKind::WouldBlock.into())
    }

    pub(crate) fn read_batch(
        &mut self,
        first: event::Event,
        child: bool,
    ) -> io::Result<Vec<event::Event>> {
        let source = std::cell::RefCell::new(self);
        if child {
            collect_ready_child_terminal_event_batch(
                first,
                || source.borrow_mut().ready(),
                || source.borrow_mut().read(),
            )
        } else {
            collect_ready_terminal_event_batch(
                first,
                || source.borrow_mut().ready(),
                || source.borrow_mut().read(),
            )
        }
    }
}

fn collect_ready_child_terminal_event_batch(
    first: event::Event,
    mut poll_ready: impl FnMut() -> io::Result<bool>,
    mut read_ready: impl FnMut() -> io::Result<event::Event>,
) -> io::Result<Vec<event::Event>> {
    let mut events = Vec::new();
    let mut next = first;
    loop {
        let host_boundary = match &next {
            event::Event::Key(key) => key.code == event::KeyCode::F(8) && key.modifiers.is_empty(),
            event::Event::Paste(_) => false,
            _ => true,
        };
        events.push(next);
        // The host must apply navigation and toggles before acquiring keys for
        // the next page; native text and commands can share one render frame.
        if host_boundary
            || events.len() == MAX_READY_CHILD_TERMINAL_EVENTS_PER_FRAME
            || !poll_ready()?
        {
            break;
        }
        next = read_ready()?;
    }
    Ok(events)
}

fn collect_ready_terminal_event_batch(
    first: event::Event,
    mut poll_ready: impl FnMut() -> io::Result<bool>,
    mut read_ready: impl FnMut() -> io::Result<event::Event>,
) -> io::Result<Vec<event::Event>> {
    // Coalesce hover and resize backlogs, but retain drag paths for pointer cancellation.
    // Dispatch keys and paste immediately so motion floods cannot delay typing.
    let mut events = Vec::new();
    let first_requires_dispatch = terminal_event_requires_immediate_dispatch(&first);
    push_coalesced_terminal_event(&mut events, first);
    if first_requires_dispatch {
        return Ok(events);
    }

    let mut raw_event_count = 1;
    while raw_event_count < MAX_READY_TERMINAL_EVENTS_PER_FRAME && poll_ready()? {
        let next = read_ready()?;
        let requires_dispatch = terminal_event_requires_immediate_dispatch(&next);
        push_coalesced_terminal_event(&mut events, next);
        raw_event_count += 1;
        if requires_dispatch {
            break;
        }
    }
    Ok(events)
}

fn terminal_event_requires_immediate_dispatch(event: &event::Event) -> bool {
    matches!(event, event::Event::Key(_) | event::Event::Paste(_))
}

fn push_coalesced_terminal_event(events: &mut Vec<event::Event>, next: event::Event) {
    if events
        .last()
        .is_some_and(|previous| terminal_event_can_replace(previous, &next))
    {
        *events
            .last_mut()
            .expect("the previous event was checked above") = next;
    } else {
        events.push(next);
    }
}

fn terminal_event_can_replace(previous: &event::Event, next: &event::Event) -> bool {
    match (previous, next) {
        (event::Event::Mouse(previous), event::Event::Mouse(next))
            if previous.modifiers == next.modifiers =>
        {
            matches!(
                (&previous.kind, &next.kind),
                (event::MouseEventKind::Moved, event::MouseEventKind::Moved)
            )
        }
        (event::Event::Resize(_, _), event::Event::Resize(_, _)) => true,
        _ => false,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/input/terminal_batch.rs"]
mod tests;
