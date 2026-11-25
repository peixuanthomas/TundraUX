use super::*;
use crate::explorer::{is_editor_document_path, is_editor_document_path_with_extensions};
use std::path::Path;

#[test]
fn default_editor_suffixes_cover_all_syntax_definitions() {
    for (pattern, _) in definitions::FILE_ASSOCIATIONS {
        let Some(extension) = pattern.strip_prefix("**/*.") else {
            continue;
        };
        let path = std::path::PathBuf::from(format!("document.{extension}"));
        assert!(is_editor_document_path(&path), "{pattern}");
    }
}

#[test]
fn default_editor_associations_cover_lsh_names_and_paths() {
    for (pattern, _) in definitions::FILE_ASSOCIATIONS {
        if pattern.starts_with("**/*.") {
            continue;
        }
        let name = pattern.strip_prefix("**/").unwrap();
        let path = Path::new(name);
        assert!(is_editor_document_path(path), "{pattern}");
        assert!(
            is_editor_document_path(&Path::new("workspace").join(path)),
            "{pattern} under a workspace directory"
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(std::ffi::OsStr::from_bytes(b"workspace\xff/.git/config"));
        assert!(is_editor_document_path(path));
    }
}

#[test]
fn editor_name_associations_require_the_matching_path_components() {
    for name in [
        "other.lock",
        "OldCargo.lock",
        "Gemfile.backup",
        "config",
        "project/not.git/config",
        "project/.git/subdirectory/config",
        "project/.config/other/config",
        "project/not-rebase-merge/done",
    ] {
        assert!(!is_editor_document_path(Path::new(name)), "{name}");
    }
    #[cfg(unix)]
    assert!(!is_editor_document_path(Path::new(r"project\.git\config")));
}

#[test]
fn custom_editor_suffixes_replace_the_default_name_associations() {
    for name in ["Cargo.lock", "Gemfile", ".git/config", "main.rs"] {
        let path = Path::new(name);
        assert!(
            !is_editor_document_path_with_extensions(path, &[] as &[&str]),
            "{name}"
        );
    }
    let custom = ["rs", "lock"];
    for name in ["Gemfile", "Cargo.toml"] {
        assert!(
            !is_editor_document_path_with_extensions(Path::new(name), &custom),
            "{name}"
        );
    }
    for name in ["main.rs", "Cargo.lock"] {
        assert!(
            is_editor_document_path_with_extensions(Path::new(name), &custom),
            "{name}"
        );
    }
    let reordered = storage::DEFAULT_EDITOR_EXPLORER_OPEN_EXTENSIONS
        .iter()
        .rev()
        .copied()
        .collect::<Vec<_>>();
    assert!(!is_editor_document_path_with_extensions(
        Path::new("Gemfile"),
        &reordered
    ));
}
