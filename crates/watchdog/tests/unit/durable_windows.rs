use super::*;
use windows_sys::Win32::Foundation::{
    ERROR_SHARING_VIOLATION, ERROR_UNABLE_TO_MOVE_REPLACEMENT, ERROR_UNABLE_TO_REMOVE_REPLACED,
};

#[test]
fn windows_replacement_retries_only_errors_with_unchanged_paths() {
    let mut results = [
        Err(io::Error::from_raw_os_error(
            ERROR_UNABLE_TO_REMOVE_REPLACED as i32,
        )),
        Err(io::Error::from_raw_os_error(ERROR_SHARING_VIOLATION as i32)),
        Ok(()),
    ]
    .into_iter();
    retry_replacement(|| results.next().unwrap()).unwrap();
    assert!(results.next().is_none());

    let mut attempts = 0;
    let error = retry_replacement(|| {
        attempts += 1;
        Err(io::Error::from_raw_os_error(
            ERROR_UNABLE_TO_MOVE_REPLACEMENT as i32,
        ))
    })
    .unwrap_err();
    assert_eq!(attempts, 1);
    assert_eq!(
        error.raw_os_error(),
        Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT as i32)
    );
}
