use std::io::Write;
use std::process::{Command, Output, Stdio};

const CHILD: &str = "TUNDRA_TEST_REPL_ENVIRONMENT_CHILD";

fn run_child_if_requested() {
    if let Ok(mode) = std::env::var(CHILD) {
        let mut args = vec!["repl"];
        if mode == "embedded" {
            args.push("--embedded");
        }
        std::process::exit(cli::run_with_platform(
            args,
            &platform::mock::UnsupportedPlatform,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        ));
    }
}

fn run_repl(test: &str, input: &str, embedded: bool) -> Output {
    let mut command = Command::new(std::env::current_exe().unwrap());
    #[cfg(target_os = "linux")]
    command.env("SHELL", "/bin/bash");
    let mut child = command
        .args(["--exact", test, "--nocapture"])
        .env(CHILD, if embedded { "embedded" } else { "standalone" })
        .env_remove("TUNDRA_COMMAND_LINE_STARTUP_HINT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn embedded_repl_screen_keyboard_points_to_external_cli_and_keeps_reading() {
    run_child_if_requested();
    let output = run_repl(
        "embedded_repl_screen_keyboard_points_to_external_cli_and_keeps_reading",
        "/debug screen-keyboard\n/help\nexit\n",
        true,
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(
        stderr.contains("screen keyboard requires an external terminal"),
        "{stderr}"
    );
    assert!(
        stderr.contains("tundra-cli debug screen-keyboard"),
        "{stderr}"
    );
    assert!(
        stdout.contains("Usage: tundra-cli"),
        "the REPL should process the next command: {stdout}"
    );
    assert!(!stdout.contains("Typed text"));
}

#[cfg(unix)]
#[test]
fn a_system_command_exit_76_does_not_become_an_embedded_debug_request() {
    run_child_if_requested();
    for embedded in [false, true] {
        let output = run_repl(
            "a_system_command_exit_76_does_not_become_an_embedded_debug_request",
            "/bin/sh -c 'exit 76'\nprintf 'AFTER_STATUS_76\\n'\nexit\n",
            embedded,
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(stdout.contains("[system exit code: 76]"), "{stdout}");
        assert!(stdout.contains("AFTER_STATUS_76"), "{stdout}");
        assert!(
            !stdout.contains("Triggering a real Shell panic"),
            "{stdout}"
        );
    }
}

#[test]
fn repl_defaults_to_system_commands_and_hints_do_not_execute_alternatives() {
    run_child_if_requested();
    for embedded in [false, true] {
        let output = run_repl(
            "repl_defaults_to_system_commands_and_hints_do_not_execute_alternatives",
            "  echo DEFAULT_COMMAND_RAN\n/echo MUST_NOT_RUN\n/config invalid\nconfig invalid\n/reset\n\n/repl\n/\n/help\nexit\n",
            embedded,
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(stdout.contains("DEFAULT_COMMAND_RAN"), "{stdout}");
        assert!(!stdout.contains("MUST_NOT_RUN"), "{stdout}");
        assert!(
            stderr.contains("Remove the '/' prefix: echo MUST_NOT_RUN"),
            "{stderr}"
        );
        assert_eq!(
            stderr.matches("This looks like a system command").count(),
            1
        );
        assert!(
            stderr.contains("Prefix it with '/': /config invalid"),
            "{stderr}"
        );
        assert!(stdout.contains("Reset cancelled."), "{stdout}");
        assert!(!stderr.contains("Usage: tundra-cli"), "{stderr}");
        assert!(
            stderr.contains("repl cannot be started from inside repl"),
            "{stderr}"
        );
        assert!(
            stderr.contains("'/' must be followed by a UX command"),
            "{stderr}"
        );
        assert_eq!(stdout.matches("Type RESET").count(), 1, "{stdout}");
        assert!(stdout.contains("doctor"), "{stdout}");
        assert!(stdout.contains("scheduled-tasks"), "{stdout}");
        assert!(!stdout.contains("默认执行系统命令"), "{stdout}");
        assert!(!stderr.contains("这可能是"), "{stderr}");
    }
}

#[test]
fn embedded_reset_requests_the_workspace_reset_after_confirmation() {
    run_child_if_requested();
    let output = run_repl(
        "embedded_reset_requests_the_workspace_reset_after_confirmation",
        "/new\n/reset\nRESET\n",
        true,
    );
    assert_eq!(output.status.code(), Some(cli::EMBEDDED_RESET_EXIT_CODE));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout)
            .matches("Type RESET")
            .count(),
        1
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown command"));
}

#[test]
fn bare_help_is_passed_to_the_system_without_ux_interception() {
    run_child_if_requested();
    for embedded in [false, true] {
        let output = run_repl(
            "bare_help_is_passed_to_the_system_without_ux_interception",
            "help\necho AFTER_SYSTEM_HELP\nexit\n",
            embedded,
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(stdout.contains("/help: UX help"));
        assert_eq!(stdout.contains("/help: UX help · exit: leave"), embedded);
        assert!(stdout.contains("[system exit code: 0]"), "{stdout}");
        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                stdout.contains("System commands use your default shell."),
                !embedded
            );
            assert_eq!(
                stdout.contains("help: shell help (if supported)"),
                !embedded
            );
            assert!(stdout.contains("GNU bash"), "{stdout}");
            assert!(!stderr.contains("Permission denied"), "{stderr}");
            assert!(
                !stderr.contains("could not retain command state"),
                "{stderr}"
            );
        }
        assert!(stdout.contains("AFTER_SYSTEM_HELP"), "{stdout}");
        assert!(!stdout.contains("Usage: tundra-cli"), "{stdout}");
        assert!(!stderr.contains("Prefix it with '/'"), "{stderr}");
    }
}

#[test]
fn ux_errors_stay_brief_and_explicit_help_still_works() {
    run_child_if_requested();
    let output = run_repl(
        "ux_errors_stay_brief_and_explicit_help_still_works",
        "/config invalid\n/not-a-ux-command\n/config set\n/help config\nexit\n",
        true,
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert_eq!(stderr.matches("ERROR:").count(), 3, "{stderr}");
    assert!(!stderr.contains("Usage:"), "{stderr}");
    assert_eq!(
        stdout.matches("Usage: tundra-cli config").count(),
        1,
        "{stdout}"
    );
}

#[test]
fn repl_reuses_system_command_state_across_builtin_commands() {
    run_child_if_requested();
    let input = if cfg!(windows) {
        "set TUNDRA_RETAINED=kept\n/help\necho result:%TUNDRA_RETAINED%\nset TUNDRA_RETAINED=\nif not defined TUNDRA_RETAINED echo removed\nexit\n"
    } else {
        "export TUNDRA_RETAINED=kept\n/help\nprintf 'result:%s\\n' \"$TUNDRA_RETAINED\"\nunset TUNDRA_RETAINED\ntest \"${TUNDRA_RETAINED+x}\" != x && printf 'removed\\n'\nexit\n"
    };
    for embedded in [false, true] {
        let output = run_repl(
            "repl_reuses_system_command_state_across_builtin_commands",
            input,
            embedded,
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(stdout.contains("result:kept"), "{stdout}\n{stderr}");
        assert!(stdout.contains("removed"), "{stdout}\n{stderr}");
        assert!(stdout.contains("Usage: tundra-cli"), "{stdout}\n{stderr}");
        assert!(
            !stderr.contains("could not retain command state"),
            "{stderr}"
        );
    }
}
