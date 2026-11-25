use std::{cell::RefCell, io, rc::Rc};

use super::{Key, KeyInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextClipboardAction {
    Copy,
    Cut,
    Paste,
}

impl TextClipboardAction {
    pub fn for_key(key: &KeyInput) -> Option<Self> {
        let modifiers = key.modifiers;
        if !(modifiers.is_control() || modifiers.super_key)
            || modifiers.alt
            || modifiers.hyper
            || modifiers.meta
        {
            return None;
        }
        match key.key {
            Key::Char('c' | 'C') => Some(Self::Copy),
            Key::Char('x' | 'X') => Some(Self::Cut),
            Key::Char('v' | 'V') => Some(Self::Paste),
            _ => None,
        }
    }
}

pub trait TextClipboard {
    fn read(&self) -> io::Result<String>;
    fn write(&self, text: String) -> io::Result<()>;
}

struct ClipboardScope {
    clipboard: Box<dyn TextClipboard>,
    error: RefCell<Option<io::Error>>,
}

thread_local! {
    static ACTIVE: RefCell<Option<Rc<ClipboardScope>>> = const { RefCell::new(None) };
}

/// Gives the focused control access to its host's clipboard for one input dispatch.
pub fn with_text_clipboard<T>(
    clipboard: impl TextClipboard + 'static,
    handle: impl FnOnce() -> T,
) -> (T, Option<io::Error>) {
    struct Restore(Option<Rc<ClipboardScope>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ACTIVE.with(|active| active.replace(self.0.take()));
        }
    }

    let scope = Rc::new(ClipboardScope {
        clipboard: Box::new(clipboard),
        error: RefCell::new(None),
    });
    let _restore = Restore(ACTIVE.with(|active| active.replace(Some(scope.clone()))));
    let result = handle();
    (result, scope.error.take())
}

pub fn read_text_clipboard() -> Option<String> {
    access(|clipboard| clipboard.read())
}

pub(super) fn write_text_clipboard(text: String) -> bool {
    access(|clipboard| clipboard.write(text)).is_some()
}

fn access<T>(operation: impl FnOnce(&dyn TextClipboard) -> io::Result<T>) -> Option<T> {
    let active = ACTIVE.with(|active| active.borrow().clone())?;
    match operation(active.clipboard.as_ref()) {
        Ok(value) => Some(value),
        Err(error) => {
            active.error.replace(Some(error));
            None
        }
    }
}
