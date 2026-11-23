use std::io::{self, BufWriter, Write};
use std::process::{Command, Stdio};

use super::{CellIconFontInstall, CellIconFontRemoval};
use crate::ManagedCommand;

const SCRIPT: &str = include_str!("windows.ps1");

pub(super) fn install(font: &[u8]) -> io::Result<CellIconFontInstall> {
    match run("Install-CurrentUserFont $font", font)?.as_str() {
        "installed" => Ok(CellIconFontInstall::Installed),
        "available" => Ok(CellIconFontInstall::Available),
        "conflict" => Ok(CellIconFontInstall::Conflict),
        response => Err(io::Error::other(format!(
            "unexpected Windows font result: {response}"
        ))),
    }
}

pub(super) fn uninstall() -> io::Result<CellIconFontRemoval> {
    match run("Remove-CurrentUserFont", &[])?.as_str() {
        "removed" => Ok(CellIconFontRemoval::Removed),
        "missing" => Ok(CellIconFontRemoval::Missing),
        "preserved" => Ok(CellIconFontRemoval::Preserved),
        response => Err(io::Error::other(format!(
            "unexpected Windows font result: {response}"
        ))),
    }
}

fn powershell() -> Command {
    #[cfg(windows)]
    {
        use std::path::PathBuf;

        let root = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        Command::new(root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
    }
    #[cfg(target_os = "linux")]
    Command::new("powershell.exe")
}

fn run(action: &str, font: &[u8]) -> io::Result<String> {
    let result = run_with(powershell(), action, font);
    #[cfg(target_os = "linux")]
    if result
        .as_ref()
        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
        && let Some(command) = wsl_powershell_path()
    {
        return run_with(command, action, font);
    }
    result
}

#[cfg(target_os = "linux")]
fn wsl_powershell_path() -> Option<Command> {
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    let mut path = std::env::var_os("SystemRoot").unwrap_or_else(|| OsString::from(r"C:\Windows"));
    path.push(r"\System32\WindowsPowerShell\v1.0\powershell.exe");
    let output = Command::new("wslpath")
        .arg("-u")
        .arg(path)
        .output_managed()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = Path::new(OsStr::from_bytes(output.stdout.trim_ascii_end()));
    path.is_absolute().then(|| Command::new(path))
}

fn run_with(mut command: Command, action: &str, font: &[u8]) -> io::Result<String> {
    let mut child = command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "& ([scriptblock]::Create([Console]::In.ReadToEnd()))",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_managed()?;

    let send_script = (|| {
        let mut input = BufWriter::new(child.stdin.take().expect("piped font script input"));
        writeln!(input, "$ErrorActionPreference = 'Stop'")?;
        writeln!(
            input,
            "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)"
        )?;
        input.write_all(SCRIPT.as_bytes())?;
        writeln!(input, "\ntry {{")?;
        write!(input, "$font = Convert-FontHex '")?;
        for byte in font {
            write!(input, "{byte:02x}")?;
        }
        writeln!(
            input,
            "'\n{action}\n}} catch {{ [Console]::Error.WriteLine($_.ToString()); exit 1 }}"
        )?;
        input.flush()
    })();
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Windows cell icon font operation failed: {}",
            String::from_utf8_lossy(&output.stderr).trim(),
        )));
    }
    send_script?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(all(test, windows))]
#[path = "../../tests/unit/cell_icon_font_windows.rs"]
mod tests;
