//! Layout and paint the two conversations. Only the displayed thread owns a live composer.

use super::App;
use super::agents_overview::AGENTS_OVERVIEW_VIEW_ID;
use super::side::SideCompanion;
use crate::bottom_pane::CommandPopupPlacement;
use crate::bottom_pane::ComposerGap;
use crate::bottom_pane::ComposerRenderOptions;
use crate::custom_terminal::Frame;
use crate::render::renderable::Renderable;
use codex_protocol::ThreadId;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;

pub(super) struct SidePaneLayout {
    pub(super) focused: Rect,
    pub(super) passive_thread: ThreadId,
    pub(super) footer: Rect,
    passive: Rect,
    divider: u16,
}

/// A wide terminal can show a pair once the outgoing conversation has been captured.
pub(super) fn layout(app: &App, tui: &crate::tui::Tui, area: Rect) -> Option<SidePaneLayout> {
    if area.width < 100
        || !app.side_retention_enabled(tui)
        || app
            .chat_widget
            .selected_index_for_present_view(AGENTS_OVERVIEW_VIEW_ID)
            .is_some()
    {
        return None;
    }
    let active = app.current_displayed_thread_id()?;
    let side = app.side_threads.get(&active);
    let passive_thread = match side {
        Some(child) => child.parent_thread_id,
        None => app.side_thread_for_parent(active)?,
    };
    if !app.side_companions.contains_key(&passive_thread) {
        return None;
    }
    let width = area.width.saturating_sub(2);
    let hint = app.prompt_navigation_footer(width).or_else(|| {
        app.transcript_view.footer_with_navigation(
            width,
            crate::motion::MotionMode::from_animations_enabled(app.local_settings.tui.animations),
            "esc latest",
        )
    });
    let hint_height = hint
        .as_ref()
        .map_or(1, |footer| footer.text.height().max(1) as u16);
    let footer_height = area.height.min(1u16.saturating_add(hint_height));
    let footer = Rect {
        y: area.bottom() - footer_height,
        height: footer_height,
        ..area
    };
    let area = Rect {
        height: area.height - footer_height,
        ..area
    };
    let child_width = (area.width / 3).max(36);
    let [parent, child] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(child_width)])
            .spacing(2)
            .areas(area);
    let (focused, passive) = if side.is_some() {
        (child, parent)
    } else {
        (parent, child)
    };
    Some(SidePaneLayout {
        focused,
        passive,
        passive_thread,
        footer,
        divider: parent.right(),
    })
}

impl SidePaneLayout {
    /// Paint the divider, retained transcript and draft without claiming the terminal cursor.
    pub(super) fn paint_companion(&self, companion: &mut SideCompanion, frame: &mut Frame<'_>) {
        let area = self.passive;
        for y in area.y..area.bottom() {
            frame.buffer[(self.divider, y)]
                .set_symbol("│")
                .set_style(ratatui::style::Style::default().dim());
        }
        let gap = ComposerGap::default();
        let bottom = companion.bottom.parked_renderable(ComposerRenderOptions {
            shared_footer: Some(Rect::default()),
            passive: true,
            separate_status_line: true,
            composer_gap: Some(&gap),
            max_height: Some(
                ((u32::from(area.height) * 2 / 3) as u16)
                    .max(8)
                    .min(area.height),
            ),
            extend_composer_background_left: area.x > 0,
            command_popup_placement: CommandPopupPlacement::Hidden,
            ..Default::default()
        });
        let bottom_height = bottom.desired_height(area.width).min(area.height);
        let above = area.height - bottom_height;
        let transcript = Rect {
            height: above,
            ..area
        };
        companion.view.render_parked(
            transcript,
            frame.buffer,
            &companion.cells,
            companion.display.active.as_deref(),
            &companion.display.auxiliary,
        );
        frame
            .image_placements
            .extend(companion.view.image_placements());

        let composer = Rect {
            y: area.y + above,
            height: bottom_height,
            ..area
        };
        bottom.render(composer, frame.buffer);
    }
}
