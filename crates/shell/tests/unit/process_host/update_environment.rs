use super::*;
use app::update::{UPDATE_READY_FILE_ENV, UPDATE_ROLLBACK_ENV, UPDATE_TARGET_SHA_ENV};
use workspace_protocol::{RunId, SessionId};

#[test]
fn only_the_shell_worker_receives_update_startup_markers() {
    let markers = [
        UPDATE_READY_FILE_ENV,
        UPDATE_TARGET_SHA_ENV,
        UPDATE_ROLLBACK_ENV,
    ];
    for role in [
        WorkerRole::Shell,
        WorkerRole::Services,
        WorkerRole::App(SessionId(1)),
    ] {
        let mut command = Command::new("tundra-shell");
        for name in markers {
            command.env(name, "from-updater");
        }
        WorkerArgs {
            worker: WorkerId {
                role,
                run: RunId(1),
                generation: ProcessGeneration(1),
            },
            guardian_endpoint: "guardian",
            guard_pid: 1,
            endpoint: "worker",
            shell_peer: None,
        }
        .apply_to(&mut command);
        for name in markers {
            assert_eq!(
                command
                    .get_envs()
                    .any(|(key, value)| key == name && value.is_some()),
                role == WorkerRole::Shell,
                "{role:?}: {name}"
            );
        }
    }
}
