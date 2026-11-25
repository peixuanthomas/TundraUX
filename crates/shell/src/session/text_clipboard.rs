use super::ShellSession;
use crate::session_services::{SessionServicesClient, client::service_error};
use std::{
    fmt, io,
    sync::{Arc, Mutex},
};
use ui::components::TextClipboard;
use workspace_protocol::{ServiceError, ServiceOperation, ServiceValue};

#[derive(Clone, Default)]
pub(crate) struct MemoryTextClipboard(Arc<Mutex<(Option<String>, String)>>);

impl fmt::Debug for MemoryTextClipboard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemoryTextClipboard")
            .finish_non_exhaustive()
    }
}

impl PartialEq for MemoryTextClipboard {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for MemoryTextClipboard {}

pub(crate) struct SessionTextClipboard {
    services: Option<&'static Arc<SessionServicesClient>>,
    session: Option<String>,
    memory: MemoryTextClipboard,
}

impl SessionTextClipboard {
    pub(crate) fn standalone() -> Self {
        Self {
            services: None,
            session: None,
            memory: MemoryTextClipboard::default(),
        }
    }

    fn memory(&self) -> std::sync::MutexGuard<'_, (Option<String>, String)> {
        let mut memory = self
            .memory
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if memory.0 != self.session {
            *memory = (self.session.clone(), String::new());
        }
        memory
    }
}

impl TextClipboard for SessionTextClipboard {
    fn read(&self) -> io::Result<String> {
        if let Some(services) = self.services {
            match services.request_text_clipboard(
                ServiceOperation::ReadTextClipboard,
                self.session.as_deref(),
            )? {
                ServiceValue::Versioned {
                    value: serde_json::Value::String(text),
                    ..
                } => Ok(text),
                _ => Err(service_error(ServiceError::InvalidRequest)),
            }
        } else {
            Ok(self.memory().1.clone())
        }
    }

    fn write(&self, text: String) -> io::Result<()> {
        if let Some(services) = self.services {
            match services.request_text_clipboard(
                ServiceOperation::WriteTextClipboard { text },
                self.session.as_deref(),
            )? {
                ServiceValue::Empty => Ok(()),
                _ => Err(service_error(ServiceError::InvalidRequest)),
            }
        } else {
            self.memory().1 = text;
            Ok(())
        }
    }
}

impl ShellSession {
    pub(super) fn text_clipboard(&self) -> SessionTextClipboard {
        SessionTextClipboard {
            services: SessionServicesClient::current(),
            session: self.app.auth_session().map(|auth| auth.session_id.clone()),
            memory: self.text_clipboard.clone(),
        }
    }
}
