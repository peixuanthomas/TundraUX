use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn route_setup_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target_component = self.setup_active_key_component();
        let target = RoutedTarget::Component(target_component);
        if key.phase == InputPhase::Release {
            return (target, ShellCommand::Noop);
        }
        if key.key == InputKey::F(9)
            && key.is_unmodified_action_key()
            && key.phase == InputPhase::Press
            && self.setup_defaults_available()
        {
            return (target, ShellCommand::SetupUseDefaults);
        }
        if key.key == InputKey::F(10) && key.is_unmodified_action_key() {
            return (
                target,
                if key.phase != InputPhase::Press {
                    ShellCommand::Noop
                } else if self.pages.setup.setup_custom_color_target.is_some() {
                    ShellCommand::ApplySetupCustomColor
                } else {
                    ShellCommand::SetupPrimaryAction
                },
            );
        }
        if key.has_non_shift_modifier() {
            if key.phase == InputPhase::Press
                && key.modifiers.is_control()
                && !key.modifiers.alt
                && !key.modifiers.super_key
                && !key.modifiers.hyper
                && !key.modifiers.meta
                && key.key == InputKey::Enter
            {
                return (
                    target,
                    if self.pages.setup.setup_custom_color_target.is_some() {
                        ShellCommand::ApplySetupCustomColor
                    } else {
                        ShellCommand::SetupPrimaryAction
                    },
                );
            }
            if key.phase == InputPhase::Press
                && key.modifiers.alt
                && !key.modifiers.is_control()
                && !key.modifiers.super_key
                && !key.modifiers.hyper
                && !key.modifiers.meta
                && key.key == InputKey::Left
                && self.pages.setup.setup_custom_color_target.is_none()
                && self.pages.setup.setup_step == ui::SetupStep::Timezone
            {
                return (target, ShellCommand::SetupPreviousStep);
            }
            return (target, ShellCommand::RecordInput);
        }
        if key.phase != InputPhase::Press
            && (matches!(key.key, InputKey::Enter | InputKey::Escape)
                || (key.key == InputKey::Char(' ')
                    && self.pages.setup.setup_custom_color_target.is_none()))
        {
            return (target, ShellCommand::RecordInput);
        }

        if self.pages.setup.setup_custom_color_target.is_some() {
            return match &key.key {
                InputKey::Escape => (target, ShellCommand::CancelSetupCustomColor),
                InputKey::Enter => (target, ShellCommand::ApplySetupCustomColor),
                InputKey::Backspace => (target, ShellCommand::SetupCustomColorBackspace),
                InputKey::Left
                | InputKey::Right
                | InputKey::Home
                | InputKey::End
                | InputKey::Delete => (target, ShellCommand::EditSetupCustomColor(key.clone())),
                InputKey::Char(character) => {
                    (target, ShellCommand::AppendSetupCustomColorChar(*character))
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }

        match self.pages.setup.setup_step {
            ui::SetupStep::Language => match &key.key {
                InputKey::Up | InputKey::Left => (target, ShellCommand::SetupPreviousLanguage),
                InputKey::Down | InputKey::Right => (target, ShellCommand::SetupNextLanguage),
                InputKey::Enter | InputKey::Char(' ') => (target, ShellCommand::SetupContinue),
                _ => (target, ShellCommand::RecordInput),
            },
            ui::SetupStep::Timezone => match &key.key {
                InputKey::Up => (target, ShellCommand::SetupPreviousTimezone),
                InputKey::Down => (target, ShellCommand::SetupNextTimezone),
                InputKey::PageUp => (target, ShellCommand::SetupPageTimezoneUp),
                InputKey::PageDown => (target, ShellCommand::SetupPageTimezoneDown),
                InputKey::Home => (target, ShellCommand::SetupFirstTimezone),
                InputKey::End => (target, ShellCommand::SetupLastTimezone),
                InputKey::Enter => (target, ShellCommand::SetupContinue),
                _ => (target, ShellCommand::RecordInput),
            },
            ui::SetupStep::Appearance => match &key.key {
                InputKey::BackTab => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Tab if key.modifiers.shift => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Tab => (target, ShellCommand::SetupFocusNext),
                InputKey::Up => (target, ShellCommand::SetupFocusPrevious),
                InputKey::Down => (target, ShellCommand::SetupFocusNext),
                InputKey::Left => (target, ShellCommand::SetupPreviousAppearanceChoice),
                InputKey::Right => (target, ShellCommand::SetupNextAppearanceChoice),
                InputKey::Enter | InputKey::Char(' ') => {
                    (target, ShellCommand::SubmitSetupAppearance)
                }
                _ => (target, ShellCommand::RecordInput),
            },
        }
    }

    pub(in crate::session) fn route_setup_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();

        match mouse.kind {
            ui::MouseEventKind::Moved => {
                (target_route(hit_target), ShellCommand::Hover(hit_target))
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Up)
                if hit_target == Some(ShellComponent::SetupLanguage)
                    && self.pages.setup.setup_step == ui::SetupStep::Language =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupLanguage),
                    ShellCommand::SetupPreviousLanguage,
                )
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Down)
                if hit_target == Some(ShellComponent::SetupLanguage)
                    && self.pages.setup.setup_step == ui::SetupStep::Language =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupLanguage),
                    ShellCommand::SetupNextLanguage,
                )
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Up)
                if hit_target == Some(ShellComponent::SetupTimezone)
                    && self.pages.setup.setup_step == ui::SetupStep::Timezone =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupTimezone),
                    ShellCommand::SetupPreviousTimezone,
                )
            }
            ui::MouseEventKind::Scroll(ScrollDirection::Down)
                if hit_target == Some(ShellComponent::SetupTimezone)
                    && self.pages.setup.setup_step == ui::SetupStep::Timezone =>
            {
                (
                    RoutedTarget::Component(ShellComponent::SetupTimezone),
                    ShellCommand::SetupNextTimezone,
                )
            }
            ui::MouseEventKind::Down(PointerButton::Left) => {
                if let Some(target) = hit_target
                    && setup_field_for_component(target).is_some()
                    && setup_component_active_for_step(target, self.pages.setup.setup_step)
                {
                    return (
                        RoutedTarget::Component(target),
                        ShellCommand::ActivateSetup {
                            target,
                            coordinates,
                        },
                    );
                }

                (RoutedTarget::None, ShellCommand::RecordInput)
            }
            ui::MouseEventKind::Down(PointerButton::Right) => {
                self.last_click = None;
                (target_route(hit_target), ShellCommand::CaptureOverlayInput)
            }
            _ => (target_route(hit_target), ShellCommand::RecordInput),
        }
    }
}
