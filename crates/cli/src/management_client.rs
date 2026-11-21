//! The CLI and Shell use the same authorized helper and task connection.
use crate::management_command::ManagementCli;
#[cfg(target_os = "linux")]
use platform::management::problem;
use std::io::Write;

pub(crate) fn run(call: ManagementCli, stdout: &mut impl Write, stderr: &mut impl Write) -> i32 {
    if let ManagementCli::Help(group) = &call {
        return crate::management_command::help(stdout, group).map_or(1, |_| 0);
    }
    let ManagementCli::Run(request) = call else {
        unreachable!()
    };
    #[cfg(target_os = "linux")]
    {
        match linux::execute(&request, stdout, stderr) {
            Ok(code) => code,
            Err(error) => {
                let code = problem::OperationProblem::from_error(&error).exit_code;
                let _ = writeln!(stderr, "{error}");
                if request.json {
                    let _ = serde_json::to_writer(
                        &mut *stdout,
                        &serde_json::json!({"status":"failed","problem":problem::OperationProblem::from_error(&error)}),
                    );
                    let _ = writeln!(stdout);
                }
                code
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = stdout;
        let _ = request;
        let _ = writeln!(stderr, "Linux management is unavailable on this platform.");
        4
    }
}

#[cfg(target_os = "linux")]
mod linux;
