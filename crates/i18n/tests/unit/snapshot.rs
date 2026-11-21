use super::*;
use crate::msg;

#[test]
fn identical_english_fallbacks_share_bundle_indices_and_render() {
    let snapshot = LanguageSnapshot::embedded(1);
    assert!(Arc::ptr_eq(&snapshot.english, &snapshot.embedded));
    assert_eq!(msg!("notifications-action-ok").render(&snapshot), "OK");
}

#[test]
fn empty_disk_english_shares_builtin_bundle_without_changing_current_language() {
    let snapshot = LanguageSnapshotData {
        code: "fr-FR".into(),
        generation: 3,
        current: vec!["notifications-action-ok = Choisir\n".into()],
        english: Vec::new(),
    }
    .restore_validated()
    .unwrap();
    assert!(Arc::ptr_eq(&snapshot.english, &snapshot.embedded));
    assert!(!Arc::ptr_eq(&snapshot.current, &snapshot.english));
    assert_eq!(snapshot.code(), "fr-FR");
    assert_eq!(snapshot.generation(), 3);
    assert_eq!(msg!("notifications-action-ok").render(&snapshot), "Choisir");
    assert_eq!(msg!("resources-recovery-ok").render(&snapshot), "OK");
}

#[test]
fn shared_resources_keep_fallback_layers_and_snapshot_generations_independent() {
    let mut data = LanguageSnapshotData {
        code: "fr-FR".into(),
        generation: 1,
        current: vec!["notifications-action-ok = Choisir\n".into()],
        english: vec!["notifications-action-ok = Customized English\n".into()],
    };
    let current = data.clone().restore_validated().unwrap();
    assert!(!Arc::ptr_eq(&current.english, &current.embedded));
    assert_eq!(msg!("notifications-action-ok").render(&current), "Choisir");

    data.current = vec!["notifications-action-ok = { $missing }\n".into()];
    let english = data.clone().restore_validated().unwrap();
    assert_eq!(
        msg!("notifications-action-ok").render(&english),
        "Customized English"
    );

    data.english = data.current.clone();
    let embedded = data.clone().restore_validated().unwrap();
    assert_eq!(msg!("notifications-action-ok").render(&embedded), "OK");

    data.code = DEFAULT_LANGUAGE.into();
    data.generation = 2;
    data.current.clear();
    data.english = vec!["notifications-action-ok = Updated English\n".into()];
    let next = data.restore_validated().unwrap();
    assert_eq!(next.code(), DEFAULT_LANGUAGE);
    assert_eq!(next.generation(), 2);
    assert!(!Arc::ptr_eq(&next.english, &next.embedded));
    assert_eq!(
        msg!("notifications-action-ok").render(&next),
        "Updated English"
    );
    assert_eq!(current.code(), "fr-FR");
    assert_eq!(current.generation(), 1);
    assert_eq!(msg!("notifications-action-ok").render(&current), "Choisir");
    assert_eq!(
        msg!("notifications-action-ok").render(&english),
        "Customized English"
    );
}

#[test]
fn shared_parsing_still_rejects_duplicate_declarations_in_one_bundle() {
    let error = LanguageSnapshotData {
        code: "fr-FR".into(),
        generation: 1,
        current: vec!["test-duplicate = Value\n".into(); 2],
        english: Vec::new(),
    }
    .restore_validated()
    .unwrap_err();
    assert_eq!(error.kind, LanguageErrorKind::Duplicate);
    assert_eq!(error.path, Some("<memory>/current/1.ftl".into()));
}
