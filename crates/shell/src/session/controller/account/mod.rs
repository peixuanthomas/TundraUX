mod input;
use crate::session::*;
impl ShellSession {
    pub(in crate::session) fn setup_next_language(&mut self) {
        let count = self.language_options().len();
        if count == 0 {
            return;
        }
        self.pages.setup.setup_selected_language_index =
            (self.pages.setup.setup_selected_language_index + 1) % count;
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn setup_previous_language(&mut self) {
        let count = self.language_options().len();
        if count == 0 {
            return;
        }
        self.pages.setup.setup_selected_language_index =
            (self.pages.setup.setup_selected_language_index + count - 1) % count;
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn setup_select_timezone_delta(&mut self, delta: isize) {
        let count = app::setup_timezone_options().len();
        if count == 0 {
            self.pages.setup.setup_timezone_window_start = 0;
            return;
        }

        let current = self
            .pages
            .setup
            .setup_selected_timezone_index
            .min(count - 1) as isize;
        let next = (current + delta).clamp(0, count as isize - 1) as usize;
        self.pages.setup.setup_selected_timezone_index = next;
        self.sync_setup_timezone_window();
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn setup_select_first_timezone(&mut self) {
        self.pages.setup.setup_selected_timezone_index = 0;
        self.sync_setup_timezone_window();
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn setup_select_last_timezone(&mut self) {
        let count = app::setup_timezone_options().len();
        if count == 0 {
            return;
        }
        self.pages.setup.setup_selected_timezone_index = count - 1;
        self.sync_setup_timezone_window();
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn sync_setup_timezone_window(&mut self) {
        let count = app::setup_timezone_options().len();
        if count == 0 {
            self.pages.setup.setup_selected_timezone_index = 0;
            self.pages.setup.setup_timezone_window_start = 0;
            return;
        }

        self.pages.setup.setup_selected_timezone_index = self
            .pages
            .setup
            .setup_selected_timezone_index
            .min(count - 1);
        let visible_rows = self.setup_timezone_visible_row_count().min(count).max(1);
        let max_start = count.saturating_sub(visible_rows);
        self.pages.setup.setup_timezone_window_start =
            self.pages.setup.setup_timezone_window_start.min(max_start);

        if self.pages.setup.setup_selected_timezone_index
            < self.pages.setup.setup_timezone_window_start
        {
            self.pages.setup.setup_timezone_window_start =
                self.pages.setup.setup_selected_timezone_index;
        }

        let window_end = self
            .pages
            .setup
            .setup_timezone_window_start
            .saturating_add(visible_rows);
        if self.pages.setup.setup_selected_timezone_index >= window_end {
            self.pages.setup.setup_timezone_window_start = self
                .pages
                .setup
                .setup_selected_timezone_index
                .saturating_add(1)
                .saturating_sub(visible_rows)
                .min(max_start);
        }
    }

    pub(in crate::session) fn setup_timezone_visible_row_count(&self) -> usize {
        ui::setup_timezone_visible_rows(self.touch_setup_viewport().content).max(1)
    }

    pub(in crate::session) fn setup_previous_step(&mut self) {
        if self.pages.setup.setup_custom_color_target.is_some() {
            return;
        }
        let (step, field, component) = match self.pages.setup.setup_step {
            ui::SetupStep::Language => return,
            ui::SetupStep::Appearance if self.platform_kind != PlatformKind::Linux => (
                ui::SetupStep::Timezone,
                ui::SetupField::TimezoneList,
                ShellComponent::SetupTimezone,
            ),
            _ => (
                ui::SetupStep::Language,
                ui::SetupField::LanguageList,
                ShellComponent::SetupLanguage,
            ),
        };
        self.pages.setup.setup_step = step;
        self.pages.setup.setup_focused_field = field;
        self.pages.common.focused_component = component;
        if step == ui::SetupStep::Timezone {
            self.sync_setup_timezone_window();
        }
        self.pages.common.page_touch.setup_scroll = 0;
        self.pages.common.error_message = None;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn setup_defaults_available(&self) -> bool {
        self.pages.setup.setup_custom_color_target.is_none()
    }

    pub(in crate::session) fn setup_use_defaults(&mut self) {
        if !self.setup_defaults_available() {
            return;
        }
        if self.pages.setup.setup_step == ui::SetupStep::Language {
            self.setup_continue();
        }
        if self.pages.setup.setup_step == ui::SetupStep::Timezone {
            self.setup_continue();
        }
        if self.pages.setup.setup_step != ui::SetupStep::Appearance {
            return;
        }
        let defaults = storage::AppearanceConfig::default();
        self.pages.setup.setup_border_shape = defaults.border_shape;
        self.pages.setup.setup_theme_color = defaults.border_color;
        self.pages.setup.setup_accent_color = defaults.accent_color;
        self.finish_first_run_setup();
    }

    pub(in crate::session) fn setup_primary_action(&mut self) {
        if self.pages.setup.setup_custom_color_target.is_some() {
            return;
        }
        match self.pages.setup.setup_step {
            ui::SetupStep::Language | ui::SetupStep::Timezone => self.setup_continue(),
            ui::SetupStep::Appearance => self.finish_first_run_setup(),
        }
    }

    pub(in crate::session) fn setup_continue(&mut self) {
        self.pages.common.page_touch.setup_scroll = 0;
        match self.pages.setup.setup_step {
            ui::SetupStep::Language => {
                let code = self.selected_setup_language_value();
                match self.prepare_language(&code) {
                    Ok((catalog, loaded)) => self.publish_language(catalog, loaded),
                    Err(error) => {
                        self.report_language_failure(&error);
                        return;
                    }
                }
                if self.platform_kind == PlatformKind::Linux {
                    self.pages.setup.setup_step = ui::SetupStep::Appearance;
                    self.pages.setup.setup_focused_field = ui::SetupField::AppearanceShape;
                    self.pages.common.focused_component = ShellComponent::SetupAppearanceShape;
                } else {
                    self.pages.setup.setup_step = ui::SetupStep::Timezone;
                    self.pages.setup.setup_focused_field = ui::SetupField::TimezoneList;
                    self.pages.common.focused_component = ShellComponent::SetupTimezone;
                    self.sync_setup_timezone_window();
                }
            }
            ui::SetupStep::Timezone => {
                self.pages.setup.setup_step = ui::SetupStep::Appearance;
                self.pages.setup.setup_focused_field = ui::SetupField::AppearanceShape;
                self.pages.common.focused_component = ShellComponent::SetupAppearanceShape;
            }
            ui::SetupStep::Appearance => {}
        }
        self.pages.common.error_message = None;
        self.refresh_hit_map();
    }

    pub(in crate::session) fn move_setup_appearance_focus(&mut self, direction: i8) {
        if self.pages.setup.setup_step != ui::SetupStep::Appearance {
            return;
        }
        let order = [
            (
                ui::SetupField::AppearanceShape,
                ShellComponent::SetupAppearanceShape,
            ),
            (
                ui::SetupField::AppearanceThemeColor,
                ShellComponent::SetupAppearanceThemeColor,
            ),
            (
                ui::SetupField::AppearanceThemeCustom,
                ShellComponent::SetupAppearanceThemeCustom,
            ),
            (
                ui::SetupField::AppearanceAccentColor,
                ShellComponent::SetupAppearanceAccentColor,
            ),
            (
                ui::SetupField::AppearanceAccentCustom,
                ShellComponent::SetupAppearanceAccentCustom,
            ),
            (
                ui::SetupField::AppearanceSubmit,
                ShellComponent::SetupAppearanceSubmit,
            ),
        ];
        let next = match order
            .iter()
            .position(|(field, _)| *field == self.pages.setup.setup_focused_field)
        {
            Some(current) => {
                (current as isize + direction as isize).rem_euclid(order.len() as isize) as usize
            }
            None if direction < 0 => order.len().saturating_sub(1),
            None => 0,
        };
        let (field, component) = order[next];
        self.pages.setup.setup_focused_field = field;
        self.pages.common.focused_component = component;
        self.pages.common.error_message = None;
        self.ensure_touch_auth_focus_visible();
    }

    pub(in crate::session) fn focus_setup_component(&mut self, component: ShellComponent) {
        if !setup_component_active_for_step(component, self.pages.setup.setup_step) {
            return;
        }

        let Some(field) = setup_field_for_component(component) else {
            return;
        };

        self.pages.setup.setup_focused_field = field;
        self.pages.common.focused_component = component;
        self.ensure_touch_auth_focus_visible();
    }

    pub(in crate::session) fn setup_active_key_component(&self) -> ShellComponent {
        match self.pages.setup.setup_step {
            ui::SetupStep::Language => ShellComponent::SetupLanguage,
            ui::SetupStep::Timezone => ShellComponent::SetupTimezone,
            ui::SetupStep::Appearance => {
                if self.pages.setup.setup_custom_color_target.is_some() {
                    ShellComponent::SetupCustomColorDialog
                } else {
                    let component = setup_component_for_field(self.pages.setup.setup_focused_field);
                    if setup_component_active_for_step(component, self.pages.setup.setup_step) {
                        component
                    } else {
                        ShellComponent::SetupAppearanceShape
                    }
                }
            }
        }
    }

    pub(in crate::session) fn activate_setup(
        &mut self,
        target: ShellComponent,
        coordinates: CellPosition,
    ) {
        match self.pages.setup.setup_step {
            ui::SetupStep::Language => {
                if target == ShellComponent::SetupLanguage
                    && let Some(index) = self.setup_language_index_at(coordinates)
                {
                    self.pages.setup.setup_selected_language_index = index;
                    self.pages.common.error_message = None;
                }
                self.focus_setup_component(ShellComponent::SetupLanguage);
            }
            ui::SetupStep::Timezone => {
                if target == ShellComponent::SetupTimezone
                    && let Some(index) = self.setup_timezone_index_at(coordinates)
                {
                    self.pages.setup.setup_selected_timezone_index = index;
                    self.sync_setup_timezone_window();
                    self.pages.common.error_message = None;
                }
                self.focus_setup_component(ShellComponent::SetupTimezone);
            }
            ui::SetupStep::Appearance => {
                self.activate_setup_appearance(target, coordinates);
            }
        }
    }

    pub(in crate::session) fn activate_setup_appearance(
        &mut self,
        target: ShellComponent,
        coordinates: CellPosition,
    ) {
        if self.pages.setup.setup_custom_color_target.is_some() {
            return;
        }
        self.pages.common.error_message = None;

        match target {
            ShellComponent::SetupAppearanceShape => {
                {
                    let main = self.touch_setup_viewport().content;
                    for (shape, area) in ui::setup_appearance_shape_option_areas(main) {
                        if rect_contains(area, coordinates) {
                            self.pages.setup.setup_border_shape = match shape {
                                ui::BorderShape::Rounded => storage::BorderShape::Rounded,
                                ui::BorderShape::Square => storage::BorderShape::Square,
                            };
                            break;
                        }
                    }
                }
                self.focus_setup_component(target);
            }
            ShellComponent::SetupAppearanceThemeColor
            | ShellComponent::SetupAppearanceAccentColor => {
                {
                    let main = self.touch_setup_viewport().content;
                    let field = if target == ShellComponent::SetupAppearanceThemeColor {
                        ui::SetupField::AppearanceThemeColor
                    } else {
                        ui::SetupField::AppearanceAccentColor
                    };
                    if let Some((index, _)) = ui::setup_appearance_palette_option_areas(main, field)
                        .into_iter()
                        .find(|(_, area)| rect_contains(*area, coordinates))
                    {
                        self.set_setup_standard_color(field, index);
                    }
                }
                self.focus_setup_component(target);
            }
            ShellComponent::SetupAppearanceThemeCustom => {
                self.focus_setup_component(target);
                self.open_setup_custom_color(ui::SetupCustomColorTarget::Theme);
            }
            ShellComponent::SetupAppearanceAccentCustom => {
                self.focus_setup_component(target);
                self.open_setup_custom_color(ui::SetupCustomColorTarget::Accent);
            }
            ShellComponent::SetupAppearanceSubmit => {
                self.focus_setup_component(target);
                self.setup_primary_action();
            }
            ShellComponent::SetupCustomColorDialog => {}
            _ => {}
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn setup_select_appearance_choice(&mut self, delta: isize) {
        match self.pages.setup.setup_focused_field {
            ui::SetupField::AppearanceShape => {
                self.pages.setup.setup_border_shape = match self.pages.setup.setup_border_shape {
                    storage::BorderShape::Rounded => storage::BorderShape::Square,
                    storage::BorderShape::Square => storage::BorderShape::Rounded,
                };
            }
            field @ (ui::SetupField::AppearanceThemeColor
            | ui::SetupField::AppearanceAccentColor) => {
                let options = ui::setup_standard_color_options();
                if options.is_empty() {
                    return;
                }
                let current_value = match field {
                    ui::SetupField::AppearanceThemeColor => {
                        self.pages.setup.setup_theme_color.to_string()
                    }
                    _ => self.pages.setup.setup_accent_color.to_string(),
                };
                let current = options
                    .iter()
                    .position(|option| option.value.eq_ignore_ascii_case(&current_value));
                let mut next =
                    current.unwrap_or_else(|| if delta < 0 { 0 } else { options.len() - 1 });
                for _ in 0..options.len() {
                    next = (next as isize + delta).rem_euclid(options.len() as isize) as usize;
                    let Ok(candidate) = options[next].value.parse::<storage::BorderColor>() else {
                        continue;
                    };
                    if field == ui::SetupField::AppearanceAccentColor
                        && candidate == self.pages.setup.setup_theme_color
                    {
                        continue;
                    }
                    self.set_setup_standard_color(field, next);
                    break;
                }
            }
            _ => return,
        }
        self.pages.common.error_message = None;
    }

    pub(in crate::session) fn set_setup_standard_color(
        &mut self,
        field: ui::SetupField,
        index: usize,
    ) {
        let options = ui::setup_standard_color_options();
        let Some(option) = options.get(index) else {
            return;
        };
        let Ok(color) = option.value.parse::<storage::BorderColor>() else {
            return;
        };
        match field {
            ui::SetupField::AppearanceThemeColor => {
                self.pages.setup.setup_theme_color = color;
                self.ensure_setup_accent_differs();
            }
            ui::SetupField::AppearanceAccentColor
                if color != self.pages.setup.setup_theme_color =>
            {
                self.pages.setup.setup_accent_color = color;
            }
            _ => {}
        }
    }

    pub(in crate::session) fn ensure_setup_accent_differs(&mut self) {
        if self.pages.setup.setup_accent_color != self.pages.setup.setup_theme_color {
            return;
        }
        if let Some(color) = ui::setup_standard_color_options()
            .iter()
            .filter_map(|option| option.value.parse::<storage::BorderColor>().ok())
            .find(|color| *color != self.pages.setup.setup_theme_color)
        {
            self.pages.setup.setup_accent_color = color;
        }
    }

    pub(in crate::session) fn activate_setup_appearance_control(&mut self) {
        match self.pages.setup.setup_focused_field {
            ui::SetupField::AppearanceThemeCustom => {
                self.open_setup_custom_color(ui::SetupCustomColorTarget::Theme);
            }
            ui::SetupField::AppearanceAccentCustom => {
                self.open_setup_custom_color(ui::SetupCustomColorTarget::Accent);
            }
            ui::SetupField::AppearanceSubmit => self.finish_first_run_setup(),
            _ => self.move_setup_appearance_focus(1),
        }
        self.refresh_hit_map();
    }

    pub(in crate::session) fn open_setup_custom_color(
        &mut self,
        target: ui::SetupCustomColorTarget,
    ) {
        self.capture_modal_focus_context();
        self.pages.setup.setup_custom_color_target = Some(target);
        self.pages.setup.setup_custom_color_editing = Default::default();
        self.pages.setup.setup_custom_color_input = match target {
            ui::SetupCustomColorTarget::Theme => self.pages.setup.setup_theme_color.to_string(),
            ui::SetupCustomColorTarget::Accent => self.pages.setup.setup_accent_color.to_string(),
        };
        self.pages.setup.setup_custom_color_error = None;
        self.synchronize_overlay_focus();
    }

    pub(in crate::session) fn append_setup_custom_color_char(&mut self, character: char) {
        if self.pages.setup.setup_custom_color_target.is_none()
            || character.is_control()
            || self.pages.setup.setup_custom_color_input.chars().count() >= 32
        {
            return;
        }
        self.edit_setup_custom_color(&KeyInput::new(InputKey::Char(character)));
        self.pages.setup.setup_custom_color_error = None;
    }

    pub(in crate::session) fn setup_custom_color_backspace(&mut self) {
        if self.pages.setup.setup_custom_color_target.is_some() {
            self.edit_setup_custom_color(&KeyInput::new(InputKey::Backspace));
            self.pages.setup.setup_custom_color_error = None;
        }
    }

    pub(in crate::session) fn edit_setup_custom_color(&mut self, key: &KeyInput) {
        let state = &mut *self.pages.setup;
        state.setup_custom_color_editing.handle_event(
            &mut state.setup_custom_color_input,
            InputEvent::Key(key.clone()),
            Rect::default(),
        );
        state.setup_custom_color_error = None;
    }

    pub(in crate::session) fn apply_setup_custom_color(&mut self) {
        let Some(target) = self.pages.setup.setup_custom_color_target else {
            return;
        };
        let color = match self
            .pages
            .setup
            .setup_custom_color_input
            .parse::<storage::BorderColor>()
        {
            Ok(color) => color,
            Err(_) => {
                self.pages.setup.setup_custom_color_error =
                    Some(i18n::msg!("account-invalid-color").into());
                return;
            }
        };
        match target {
            ui::SetupCustomColorTarget::Theme => {
                self.pages.setup.setup_theme_color = color;
                self.ensure_setup_accent_differs();
            }
            ui::SetupCustomColorTarget::Accent if color == self.pages.setup.setup_theme_color => {
                self.pages.setup.setup_custom_color_error =
                    Some(i18n::msg!("account-accent-must-differ").into());
                return;
            }
            ui::SetupCustomColorTarget::Accent => self.pages.setup.setup_accent_color = color,
        }
        self.cancel_setup_custom_color();
    }

    pub(in crate::session) fn cancel_setup_custom_color(&mut self) {
        self.pages.setup.setup_custom_color_target = None;
        self.pages.setup.setup_custom_color_input.clear();
        self.pages.setup.setup_custom_color_error = None;
        self.pages.common.focused_component =
            setup_component_for_field(self.pages.setup.setup_focused_field);
        self.refresh_hit_map();
    }

    pub(in crate::session) fn setup_language_index_at(
        &self,
        coordinates: CellPosition,
    ) -> Option<usize> {
        let viewport = self.touch_setup_viewport();
        let row = setup_row_at(
            ui::setup_language_list_area(viewport.content, self.language_catalog.options().len()),
            viewport.content_point(coordinates)?,
        )?;
        (row < self.language_options().len()).then_some(row)
    }

    pub(in crate::session) fn setup_timezone_index_at(
        &self,
        coordinates: CellPosition,
    ) -> Option<usize> {
        let viewport = self.touch_setup_viewport();
        let row = setup_row_at(
            ui::setup_timezone_list_area(viewport.content),
            viewport.content_point(coordinates)?,
        )?;
        let count = app::setup_timezone_options().len();
        if count == 0 {
            return None;
        }

        let visible_rows = self.setup_timezone_visible_row_count().min(count);
        let start = self
            .pages
            .setup
            .setup_timezone_window_start
            .min(count.saturating_sub(visible_rows));
        let index = start.saturating_add(row);
        (row < visible_rows && index < count).then_some(index)
    }

    pub(in crate::session) fn selected_setup_language_value(&self) -> String {
        let options = self.language_options();
        setup_language_code_at(&options, self.pages.setup.setup_selected_language_index)
            .unwrap_or_else(|| "en-US".to_string())
    }

    pub(in crate::session) fn selected_setup_timezone_value(&self) -> String {
        let options = app::setup_timezone_options();
        setup_timezone_id_at(&options, self.pages.setup.setup_selected_timezone_index)
            .unwrap_or_else(|| "UTC".to_string())
    }

    pub(in crate::session) fn finish_first_run_setup(&mut self) {
        let (Some(storage), Some(session)) = (
            self.storage_manager.clone(),
            self.pending_personalization_session.clone(),
        ) else {
            return;
        };
        if self.pages.setup.setup_theme_color == self.pages.setup.setup_accent_color {
            self.pages.setup.setup_focused_field = ui::SetupField::AppearanceAccentColor;
            self.pages.common.focused_component = ShellComponent::SetupAppearanceAccentColor;
            self.pages.common.error_message = Some(i18n::msg!("account-accent-must-differ").into());
            self.notify_status(i18n::msg!("account-setup-incomplete"));
            return;
        }
        let appearance = storage::AppearanceConfig {
            border_shape: self.pages.setup.setup_border_shape,
            border_color: self.pages.setup.setup_theme_color,
            accent_color: self.pages.setup.setup_accent_color,
            icon_display_mode: if self.terminal_image_support {
                self.default_icon_display_mode
            } else {
                storage::IconDisplayMode::Ascii
            },
            ..storage::AppearanceConfig::default()
        };
        let language = self.selected_setup_language_value();
        let timezone = (self.platform_kind != PlatformKind::Linux)
            .then(|| self.selected_setup_timezone_value());
        let save = move || {
            let mut config = storage.load_config()?;
            config.language = language;
            if let Some(timezone) = timezone {
                config.timezone = timezone;
            }
            // A failed configuration write leaves first-run setup available on retry.
            storage.save_config(&config)?;
            let profile = storage.update_profile(&session.username, |profile| {
                profile.appearance = appearance;
                profile.system_status_dashboard =
                    storage::SystemStatusDashboardConfig::for_role(session.role.as_str());
            })?;
            Ok::<_, storage::StorageError>((session, config, profile))
        };
        if crate::session_services::SessionServicesClient::current().is_some() {
            let draft = (*self.pages.setup).clone();
            let queued =
                self.submit_workspace_job(save, Result::is_ok, move |state, result| match result {
                    Ok((session, config, profile)) => {
                        state.replace_storage_config(config);
                        if (*state.pages.setup).clone() == draft {
                            state.open_home_for_session(session, profile);
                        }
                    }
                    Err(error) => state.report_personalization_save_error(error),
                });
            if let Err(error) = queued {
                self.report_personalization_save_error(error);
            }
        } else {
            match save() {
                Ok((session, config, profile)) => {
                    self.replace_storage_config(config);
                    self.open_home_for_session(session, profile);
                }
                Err(error) => self.report_personalization_save_error(error),
            }
        }
    }

    fn report_personalization_save_error(&mut self, error: impl ToString) {
        self.pages.common.error_message = Some(error.to_string().into());
        self.notify_status(i18n::msg!("account-personalization-save-failed"));
    }

    pub(in crate::session) fn attach_system_session(&mut self, session: AuthSession) {
        let profile = self
            .storage_manager
            .as_ref()
            .map(|storage| storage.load_profile(&session.username))
            .transpose();
        match profile {
            Ok(Some(Some(profile))) => self.open_home_for_session(session, profile),
            Ok(_) => {
                self.reset_navigation(ShellScreen::FirstRunSetup);
                self.pages.setup.setup_selected_language_index = self
                    .language_catalog
                    .options()
                    .iter()
                    .position(|option| option.code == self.language.code())
                    .unwrap_or(0);
                self.pending_personalization_session = Some(session);
                self.notify_status(i18n::msg!("account-first-sign-in-setup"));
            }
            Err(error) => {
                self.pages.common.error_message = Some(
                    i18n::msg!("account-profile-load-failed", error = error.to_string()).into(),
                );
                self.shutdown_requested = true;
            }
        }
    }

    pub(in crate::session) fn open_home_for_session(
        &mut self,
        session: AuthSession,
        profile: storage::UserProfile,
    ) {
        self.reset_system_status_trackers();
        let storage::UserProfile {
            mut appearance,
            system_status_dashboard,
        } = profile;
        if self.platform_kind == PlatformKind::Linux
            && !self.terminal_image_support
            && self.ascii_assets.theme_id() == ui::DEFAULT_THEME_ID
            && appearance.icon_display_mode == storage::IconDisplayMode::Image
        {
            self.apply_ascii_icon_fallback(&session.username, &mut appearance);
        }
        self.app.dispatch_at(
            app::AppCommand::SetAuthSession(Some(session.clone())),
            Instant::now(),
        );
        if let Some(snapshot) = self.app.system_status_snapshot().cloned() {
            self.evaluate_system_status_alerts(&snapshot);
        }
        self.app.dispatch_at(
            app::AppCommand::SetActiveAppearance(Some(appearance)),
            Instant::now(),
        );
        self.app.dispatch_at(
            app::AppCommand::SetActiveSystemStatusDashboard(Some(system_status_dashboard)),
            Instant::now(),
        );
        self.home_mode = ShellHomeMode::User;
        if self.debug_home {
            if PermissionService::new(self.debug_policy)
                .authorize(Some(&session), PermissionAction::EnterDebugMode, None)
                .allowed
            {
                self.home_mode = ShellHomeMode::Debug;
            } else {
                self.notify_toast(i18n::msg!("account-debug-mode-denied"));
            }
        }
        self.pending_personalization_session = None;
        self.reset_navigation(ShellScreen::Home);
        self.load_clock_for_session(&session);
        self.refresh_hit_map();
    }

    pub(in crate::session) fn apply_ascii_icon_fallback(
        &mut self,
        username: &str,
        appearance: &mut storage::AppearanceConfig,
    ) {
        appearance.icon_display_mode = storage::IconDisplayMode::Ascii;
        if let Some(storage) = &self.storage_manager
            && let Err(error) = storage.update_profile(username, |profile| {
                profile.appearance.icon_display_mode = storage::IconDisplayMode::Ascii;
            })
        {
            self.notify_alert_with_tone(
                i18n::msg!(
                    "account-ascii-fallback-save-failed",
                    error = error.to_string()
                ),
                ui::NotificationTone::Warning,
            );
        }
    }

    pub(in crate::session) fn open_user_management(&mut self) {
        if self.platform_kind == PlatformKind::Linux {
            self.open_management(platform::management::ManagementKind::Users);
        }
    }
}
