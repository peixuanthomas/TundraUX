use super::*;
use app::editor::{CursorMove, DocumentKind, EditorCommand, EditorDocument, EditorPosition};

#[test]
fn source_page_navigation_matches_single_steps_after_manual_scroll() {
    let platform = &platform::mock::UnsupportedPlatform;
    let text = (0..100)
        .flat_map(|line| {
            if line % 3 == 0 {
                ["汉e\u{301}".repeat(135), "\n".into()]
            } else {
                ["short line".into(), "\n".into()]
            }
        })
        .collect::<String>();

    for wrap in [false, true] {
        for (key, movement, top_line) in [
            (InputKey::PageUp, CursorMove::Up, 3),
            (InputKey::PageDown, CursorMove::Down, 300),
        ] {
            let mut state = ShellSession::new_for_home_mode(
                ShellLaunchConfig::default(),
                (120, 36),
                ShellHomeMode::User,
            );
            state.open_editor();
            let editor = EditorState::from_document(EditorDocument::from_text(
                Some("navigation.txt".into()),
                DocumentKind::PlainText,
                text.clone(),
            ));
            let offset = editor.source_offset(36, 132).unwrap();
            state.app.dispatch_at(
                app::AppCommand::SetEditorState(Some(editor)),
                Instant::now(),
            );
            state.apply_editor_command(
                EditorCommand::MoveTo {
                    position: EditorPosition::Source(offset),
                    extend_selection: false,
                },
                platform,
            );
            if wrap {
                state.toggle_editor_wrap();
            }
            state.app.dispatch_at(
                app::AppCommand::SetEditorViewport(app::editor::EditorViewport {
                    top_line,
                    left_column: 85,
                }),
                Instant::now(),
            );
            let layout = state.editor_current_layout();
            assert_eq!(layout.wrap_layout.is_some(), wrap);
            let mut single_steps = state.clone();
            for _ in 0..layout.visible_capacity.saturating_sub(2).max(1) {
                single_steps.apply_editor_command(
                    EditorCommand::MoveCursor {
                        movement,
                        extend_selection: true,
                    },
                    platform,
                );
            }
            state.handle_editor_key(
                KeyInput::with_modifiers(
                    key,
                    InputModifiers {
                        shift: true,
                        ..InputModifiers::none()
                    },
                ),
                platform,
            );

            let expected = single_steps.app.editor_state().unwrap();
            let actual = state.app.editor_state().unwrap();
            assert_ne!(actual.cursor.byte_offset, offset);
            assert!(actual.selection.is_some());
            assert_eq!(actual.cursor, expected.cursor);
            assert_eq!(actual.selection, expected.selection);
            assert_eq!(actual.viewport, expected.viewport);
        }
    }
}
