use super::*;
use std::fs::{File, OpenOptions};
use std::io::Write;

pub(super) struct PerformanceTrace {
    file: Option<File>,
    inputs: BTreeMap<SessionId, VecDeque<(u64, Instant)>>,
    accepted: BTreeMap<SessionId, (u64, Instant)>,
    presented: BTreeMap<SessionId, u64>,
}

impl PerformanceTrace {
    pub(super) fn new() -> Self {
        Self {
            file: std::env::var_os("TUNDRA_PERF_TRACE").and_then(|path| {
                OpenOptions::new().create(true).append(true).open(path).ok()
            }),
            inputs: BTreeMap::new(),
            accepted: BTreeMap::new(),
            presented: BTreeMap::new(),
        }
    }

    pub(super) fn input_sent(&mut self, id: SessionId, serial: u64) {
        if self.file.is_some() {
            self.inputs
                .entry(id)
                .or_default()
                .push_back((serial, Instant::now()));
        }
    }

    pub(super) fn frame_accepted(&mut self, id: SessionId, sequence: u64) {
        if self.file.is_some() {
            self.accepted.insert(id, (sequence, Instant::now()));
        }
    }

    pub(super) fn did_present(
        &mut self,
        id: SessionId,
        sequence: u64,
        processed_serial: u64,
        terminal_output_us: u64,
    ) {
        let Some(file) = &mut self.file else {
            return;
        };
        let now = Instant::now();
        let accepted = self
            .accepted
            .get(&id)
            .filter(|(accepted_sequence, _)| *accepted_sequence == sequence)
            .map(|(_, accepted)| *accepted);
        if self.presented.insert(id, sequence) != Some(sequence) {
            let _ = serde_json::to_writer(
                &mut *file,
                &serde_json::json!({
                    "event": "frame-presented",
                    "session": id.to_string(),
                    "sequence": sequence,
                    "processed_input_serial": processed_serial,
                    "accepted_to_present_us": accepted.map(|at| now.duration_since(at).as_micros() as u64),
                    "terminal_output_us": terminal_output_us,
                }),
            );
            let _ = file.write_all(b"\n");
        }
        if let Some(inputs) = self.inputs.get_mut(&id) {
            while inputs.front().is_some_and(|(serial, _)| *serial <= processed_serial) {
                let (serial, sent) = inputs.pop_front().expect("checked input");
                let _ = serde_json::to_writer(
                    &mut *file,
                    &serde_json::json!({
                        "event": "input-presented",
                        "session": id.to_string(),
                        "sequence": sequence,
                        "serial": serial,
                        "total_us": now.duration_since(sent).as_micros() as u64,
                        "input_to_accepted_us": accepted.and_then(|at| at.checked_duration_since(sent)).map(|duration| duration.as_micros() as u64),
                        "accepted_to_present_us": accepted.map(|at| now.duration_since(at).as_micros() as u64),
                        "terminal_output_us": terminal_output_us,
                    }),
                );
                let _ = file.write_all(b"\n");
            }
        }
    }
}

impl WindowStartup {
    pub(super) fn record_presented(
        self,
        session: SessionId,
        application: &str,
        worker: Option<WorkerId>,
        auth: Option<&AuthSession>,
        terminal_output_us: u64,
        starting_shown: bool,
        accepted: Option<Instant>,
    ) {
        let (Some(accepted), Some(worker)) = (accepted, worker) else {
            return;
        };
        let presented = Instant::now();
        let total_us = presented.duration_since(self.requested).as_micros() as u64;
        let mut event = runtime_log::RuntimeLogEvent::new(
            runtime_log::LogContext {
                run_id: Some(worker.run.to_string()),
                app: application.into(),
                module: "ux.workspace".into(),
                operation: "app-startup-presented".into(),
                operation_id: Some(session.to_string()),
                owner_id: auth.map(|auth| auth.user_id.clone()),
                ..Default::default()
            },
            runtime_log::LogLevel::Info,
            runtime_log::LogPhase::Succeeded,
            format!("First window frame output in {:.2} ms", total_us as f64 / 1_000.0),
        );
        event.message_args = [
            ("total_us".into(), total_us.into()),
            (
                "accepted_us".into(),
                (accepted.duration_since(self.requested).as_micros() as u64).into(),
            ),
            (
                "accepted_to_present_us".into(),
                (presented.duration_since(accepted).as_micros() as u64).into(),
            ),
            ("terminal_output_us".into(), terminal_output_us.into()),
            ("starting_shown".into(), starting_shown.into()),
        ]
        .into_iter()
        .collect();
        runtime_log::record(event);
    }
}
