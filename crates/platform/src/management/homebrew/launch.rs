use super::*;

pub(in crate::management) fn launch_paths(
    target: &str,
    cancelled: &AtomicBool,
) -> Result<Vec<PathBuf>, ManagementError> {
    paths_with_program(&detect_brew()?, target, cancelled)
}

pub(super) fn paths_with_program(
    brew: &Path,
    target: &str,
    cancelled: &AtomicBool,
) -> Result<Vec<PathBuf>, ManagementError> {
    let (kind, name) = parse_target(target)?;
    if kind == PackageType::Formula {
        let prefix = process::read(
            brew,
            &["--prefix".into(), "--installed".into(), name.into()],
            cancelled,
        )?;
        let prefix = Path::new(prefix.trim_end());
        let mut paths = Vec::new();
        if prefix.is_absolute() {
            for dir in ["bin", "sbin"] {
                if let Ok(entries) = prefix.join(dir).read_dir() {
                    for entry in entries.flatten() {
                        check_cancelled(cancelled)?;
                        paths.push(entry.path());
                    }
                }
            }
        }
        return Ok(paths);
    }

    let info = process::read(
        brew,
        &[
            "info".into(),
            "--json=v2".into(),
            "--cask".into(),
            "--".into(),
            name.into(),
        ],
        cancelled,
    )?;
    let metadata: serde_json::Value = serde_json::from_str(&info).map_err(|_| invalid_target())?;
    let Some(artifacts) = metadata["casks"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|cask| {
            cask["full_token"].as_str() == Some(name)
                && cask["installed"]
                    .as_str()
                    .is_some_and(|version| !version.is_empty())
        })
        .and_then(|cask| cask["artifacts"].as_array())
    else {
        return Ok(Vec::new());
    };
    let mut paths = Vec::new();
    let mut sources = Vec::new();
    for artifact in artifacts {
        let Some(args) = artifact.get("app").or_else(|| artifact.get("binary")) else {
            continue;
        };
        if let Some(path) = artifact["target"]
            .as_str()
            .map(Path::new)
            .filter(|path| path.is_absolute() && path.exists())
        {
            paths.push(path.to_owned());
        } else if let Some(source) = args.get(0).and_then(|value| value.as_str()) {
            sources.push(Path::new(source));
        }
    }
    // Older Homebrew versions list artifact sources without their installed targets.
    if !sources.is_empty() {
        let files = process::read(
            brew,
            &["list".into(), "--cask".into(), "--".into(), name.into()],
            cancelled,
        )
        .unwrap_or_default();
        check_cancelled(cancelled)?;
        for file in files.lines().map(Path::new) {
            check_cancelled(cancelled)?;
            if file.is_absolute()
                && !file.components().any(|c| c.as_os_str() == ".metadata")
                && sources.iter().any(|source| file.ends_with(source))
                && let Ok(target) = file.canonicalize()
            {
                paths.push(target);
            }
        }
    }
    Ok(paths)
}
