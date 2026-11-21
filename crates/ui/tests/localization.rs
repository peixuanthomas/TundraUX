use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use i18n::{LanguageSnapshot, with_snapshot};
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::components::{UpdateActivity, UpdateActivityViewModel};
use ui::{LogsViewModel, RenderContext, TundraTheme, logs_hit_test, logs_layout};
use unicode_width::UnicodeWidthStr;

struct LocaleFixture(PathBuf);
impl LocaleFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("tux-ui-locales-{}", std::process::id()));
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets/locales");
        for locale in ["en-US", "zh-CN"] {
            let target = root.join("locales").join(locale);
            fs::create_dir_all(&target).unwrap();
            fs::write(
                target.join("manifest.toml"),
                format!("format_version = 1\ncode = \"{locale}\"\nnative_name = \"{locale}\"\n"),
            )
            .unwrap();
            for group in ["common", "modules"] {
                fs::create_dir_all(target.join(group)).unwrap();
                for entry in fs::read_dir(source.join(locale).join(group)).unwrap() {
                    let entry = entry.unwrap();
                    if entry.file_name().to_string_lossy().starts_with("ui-") {
                        fs::copy(entry.path(), target.join(group).join(entry.file_name())).unwrap();
                    }
                }
            }
        }
        Self(root)
    }
}
impl Drop for LocaleFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn snapshots() -> &'static [Arc<LanguageSnapshot>; 2] {
    static SNAPSHOTS: OnceLock<[Arc<LanguageSnapshot>; 2]> = OnceLock::new();
    SNAPSHOTS.get_or_init(|| {
        let fixture = LocaleFixture::new();
        let chinese = LanguageSnapshot::load(&fixture.0, "zh-CN", 1)
            .unwrap()
            .snapshot;
        [Arc::new(LanguageSnapshot::embedded(1)), Arc::new(chinese)]
    })
}

#[test]
fn retained_update_labels_follow_the_render_snapshot_and_preserve_raw_output() {
    let [english, chinese] = snapshots();
    let mut model = with_snapshot(english, UpdateActivityViewModel::default);
    model.show_build_details = true;
    model
        .output
        .push("Compiling external-crate /tmp/build.rs".into());
    let context = RenderContext::from_theme(
        &TundraTheme::default_dark(),
        Default::default(),
        Default::default(),
    );
    let render = |snapshot: &Arc<LanguageSnapshot>| {
        with_snapshot(snapshot, || {
            let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
            terminal
                .draw(|frame| {
                    UpdateActivity::new(&model).render_scrolled(
                        frame,
                        frame.area(),
                        0,
                        frame.area().height,
                        &context,
                    )
                })
                .unwrap();
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        })
    };
    let english_text = render(english);
    let chinese_text = render(chinese);
    assert!(english_text.contains("Download: waiting"));
    assert!(chinese_text.replace(' ', "").contains("下载：等待中"));
    assert!(chinese_text.replace(' ', "").contains("编译：等待中"));
    assert!(chinese_text.contains("Compiling external-crate /tmp/build.rs"));
    assert_eq!(
        render(english),
        english_text,
        "switching back restores the same display"
    );
}

#[test]
fn localized_log_controls_use_terminal_cell_widths() {
    let [english, chinese] = snapshots();
    for (snapshot, expected_refresh) in [(english, "Refresh"), (chinese, "刷新")] {
        with_snapshot(snapshot, || {
            let main = Rect::new(0, 0, 120, 32);
            let mut model = LogsViewModel {
                can_view_system: true,
                linux_available: true,
                level_filter: i18n::tr!(
                    "ui-logs-level-or-higher",
                    level = i18n::tr!("ui-logs-level-warning")
                ),
                ..Default::default()
            };
            model.diagnostics.can_view_details = true;
            for following in [false, true] {
                model.following = following;
                model.new_events = 12;
                let layout = logs_layout(main, &model);
                let refresh = i18n::tr!("ui-logs-refresh");
                assert!(refresh.ends_with(expected_refresh), "{refresh}");
                let controls = [
                    (ui::LogsHitTarget::Refresh, refresh),
                    (
                        ui::LogsHitTarget::FilterLevel,
                        i18n::tr!("ui-logs-level-filter", level = model.level_filter.as_str()),
                    ),
                    (
                        ui::LogsHitTarget::OpenFolder,
                        i18n::tr!("ui-logs-open-folder"),
                    ),
                    (ui::LogsHitTarget::Open, i18n::tr!("ui-logs-open")),
                    (
                        ui::LogsHitTarget::Follow,
                        if following {
                            i18n::tr!("ui-logs-pause-scroll")
                        } else {
                            i18n::tr!("ui-logs-back-latest", count = model.new_events)
                        },
                    ),
                    (
                        ui::LogsHitTarget::Filters,
                        i18n::tr!("ui-logs-source-filters"),
                    ),
                    (ui::LogsHitTarget::Expert, i18n::tr!("ui-logs-expert")),
                ];
                assert_eq!(layout.controls.len(), controls.len());
                for (target, label) in controls {
                    let control = layout
                        .controls
                        .iter()
                        .find(|control| control.target == target)
                        .unwrap();
                    assert_eq!(usize::from(control.area.width), label.width() + 2);
                    let expected = ui::logs_control_enabled(&model, target).then_some(target);
                    assert_eq!(
                        logs_hit_test(main, &model, (control.area.x, control.area.y)),
                        expected
                    );
                    assert_eq!(
                        logs_hit_test(main, &model, (control.area.right() - 1, control.area.y)),
                        expected
                    );
                }
            }
        });
    }
}
