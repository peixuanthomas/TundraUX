use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerSearchRequest {
    root: PathBuf,
    query: String,
    show_hidden: bool,
    show_system: bool,
    revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExplorerSearchResults {
    pub entries: Vec<ExplorerEntry>,
    pub skipped: usize,
}

impl ExplorerState {
    pub fn subfolder_search(&self) -> Option<ExplorerSearchRequest> {
        if !self.include_subfolders || self.query.is_empty() || self.current_location.is_trash() {
            return None;
        }
        Some(ExplorerSearchRequest {
            root: self.current_path.clone(),
            query: self.query.to_lowercase(),
            show_hidden: self.show_hidden,
            show_system: self.show_system,
            revision: self.search_revision,
        })
    }

    pub fn invalidate_subfolder_search(&mut self) {
        self.search_revision = self.search_revision.wrapping_add(1);
        self.subfolder_results = Default::default();
    }
}

impl ExplorerController {
    pub fn search_subfolders(
        &self,
        request: &ExplorerSearchRequest,
        session: &AuthSession,
        platform: &dyn Platform,
        cancelled: &AtomicBool,
    ) -> ExplorerSearchResults {
        let mut result = ExplorerSearchResults::default();
        let mut directories = vec![request.root.clone()];
        while let Some(directory) = directories.pop() {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            if self
                .file_service
                .authorize(Some(session), PermissionAction::ReadFile, &directory)
                .is_err()
                || !platform.file_attributes(&directory).is_ok_and(|attr| {
                    attr.is_dir && !attr.symlink && !attr.junction && !attr.reparse_point
                })
            {
                result.skipped += 1;
                continue;
            }
            let Ok(listing) = platform.read_directory(&directory) else {
                result.skipped += 1;
                continue;
            };
            result.skipped += listing.warnings.len();
            for entry in listing.entries {
                if cancelled.load(Ordering::Relaxed) {
                    return result;
                }
                let Some(attr) = &entry.attributes else {
                    continue;
                };
                if (!request.show_hidden && attr.hidden) || (!request.show_system && attr.system) {
                    continue;
                }
                if attr.is_dir && !attr.symlink && !attr.junction && !attr.reparse_point {
                    directories.push(entry.path.clone());
                }
                // The ordinary directory listing already supplies the root's matches.
                if directory != request.root && entry.name.to_lowercase().contains(&request.query) {
                    result.entries.push(ExplorerEntry::from_metadata(
                        entry.path,
                        entry.name,
                        entry.attributes,
                        entry.open_policy,
                    ));
                }
            }
        }
        result
    }
}
