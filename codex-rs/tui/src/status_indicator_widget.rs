//! A live task status row rendered above the composer while the agent is busy.
//!
//! The row renders a separately owned clock, the optional interrupt hint, and short inline
//! context (for example, the unified-exec background-process summary). Keeping
//! these pieces on one line avoids vertical layout churn in the bottom pane.
//! Hook activity uses the remaining space or its own line on overflow, so it
//! never displaces background-process controls.

use std::borrow::Cow;
use std::time::Duration;
use std::time::Instant;

use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use crate::app_event_sender::AppEventSender;
use crate::key_hint;
use crate::key_hint::ShortcutHint;
use crate::line_truncation::line_width;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::motion::MotionMode;
use crate::motion::ReducedMotionIndicator;
use crate::motion::activity_indicator;
use crate::render::renderable::Renderable;
use crate::text_formatting::capitalize_first;
use crate::tui::FrameRequester;
use crate::width::display_width;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;

mod timer;
pub(crate) use timer::StatusTimer;

#[path = "summary_shimmer.rs"]
mod summary_shimmer;
use summary_shimmer::summary_shimmer;

pub(crate) const STATUS_DETAILS_DEFAULT_MAX_LINES: usize = 3;
const DETAILS_PREFIX: &str = "  └ ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusDetailsCapitalization {
    CapitalizeFirst,
    Preserve,
}

/// Displays a single-line in-progress status with optional wrapped details.
pub(crate) struct StatusIndicatorWidget {
    /// Animated header text (defaults to "Working").
    header: String,
    header_started_at: Instant,
    details: Option<String>,
    details_max_lines: usize,
    /// Optional suffix rendered after the elapsed/interrupt segment.
    inline_message: Option<String>,
    recent_generation_speed: Option<String>,
    /// Hook activity may move below the status row when it cannot fit in full.
    hook_status_message: Option<String>,
    show_interrupt_hint: bool,
    interrupt_binding: Option<ShortcutHint>,

    app_event_tx: AppEventSender,
    frame_requester: FrameRequester,
    animations_enabled: bool,
    effects: codex_config::types::TuiEffects,
}

// Format elapsed seconds into a compact human-friendly form used by the status line.
// Examples: 0s, 59s, 1m 00s, 59m 59s, 1h 00m 00s, 2h 03m 09s
pub fn fmt_elapsed_compact(elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        return format!("{elapsed_secs}s");
    }
    if elapsed_secs < 3600 {
        let minutes = elapsed_secs / 60;
        let seconds = elapsed_secs % 60;
        return format!("{minutes}m {seconds:02}s");
    }
    let hours = elapsed_secs / 3600;
    let minutes = (elapsed_secs % 3600) / 60;
    let seconds = elapsed_secs % 60;
    format!("{hours}h {minutes:02}m {seconds:02}s")
}

impl StatusIndicatorWidget {
    pub(crate) fn new(
        app_event_tx: AppEventSender,
        frame_requester: FrameRequester,
        animations_enabled: bool,
        effects: codex_config::types::TuiEffects,
    ) -> Self {
        Self {
            header: String::from("Working"),
            header_started_at: Instant::now(),
            details: None,
            details_max_lines: STATUS_DETAILS_DEFAULT_MAX_LINES,
            inline_message: None,
            recent_generation_speed: None,
            hook_status_message: None,
            show_interrupt_hint: true,
            interrupt_binding: Some(key_hint::plain(KeyCode::Esc).into()),
            app_event_tx,
            frame_requester,
            animations_enabled,
            effects,
        }
    }

    pub(crate) fn interrupt(&self) {
        self.app_event_tx.interrupt();
    }

    pub(crate) fn set_recent_generation_speed(&mut self, speed: Option<String>) {
        self.recent_generation_speed = speed;
    }

    pub(crate) fn set_motion_preferences(&mut self, tui: &codex_config::types::Tui) {
        self.animations_enabled = tui.animations;
        self.effects = tui.effects;
    }

    /// Update the animated header label (left of the brackets).
    pub(crate) fn update_header(&mut self, header: String) {
        if self.header != header {
            self.header = header;
            self.header_started_at = Instant::now();
        }
    }

    /// Update the details text shown below the header.
    pub(crate) fn update_details(
        &mut self,
        details: Option<String>,
        capitalization: StatusDetailsCapitalization,
        max_lines: usize,
    ) {
        self.details_max_lines = max_lines.max(1);
        self.details = details
            .filter(|details| !details.is_empty())
            .map(|details| {
                let trimmed = details.trim_start();
                match capitalization {
                    StatusDetailsCapitalization::CapitalizeFirst => capitalize_first(trimmed),
                    StatusDetailsCapitalization::Preserve => trimmed.to_string(),
                }
            });
    }

    /// Update the inline suffix text shown after the elapsed/interrupt hint.
    ///
    /// Callers should provide plain, already-contextualized text. Passing
    /// verbose status prose here can cause frequent width truncation and hide
    /// the more important elapsed/interrupt hint.
    pub(crate) fn update_inline_message(&mut self, message: Option<String>) {
        self.inline_message = message
            .map(|message| message.trim().to_string())
            .filter(|message| !message.is_empty());
    }

    pub(crate) fn update_hook_status_message(&mut self, message: Option<String>) {
        self.hook_status_message = message;
    }

    pub(crate) fn header(&self) -> &str {
        &self.header
    }

    /// A new retry notification starts a fresh wait even when its text repeats.
    pub(crate) fn restart_retry_countdown(&mut self) {
        if self.retry_wait().is_some() {
            self.header_started_at = Instant::now();
        }
    }

    // Reuse the existing retry notification text without extending the wire protocol.
    // Count down from receipt; the core continues to own the actual retry deadline.
    fn retry_wait(&self) -> Option<(&str, Duration)> {
        let message = self.header.strip_prefix("Reconnecting...")?;
        let (attempt, wait) = message.rsplit_once(" (retrying in ")?;
        let seconds = wait.strip_suffix("s)")?.parse::<u64>().ok()?;
        Some((attempt.trim(), Duration::from_secs(seconds)))
    }

    fn header_at(&self, now: Instant) -> Cow<'_, str> {
        let Some((attempt, wait)) = self.retry_wait() else {
            return Cow::Borrowed(&self.header);
        };
        let remaining = wait.saturating_sub(now.saturating_duration_since(self.header_started_at));
        let attempt = if attempt.is_empty() {
            String::new()
        } else {
            format!(" ({attempt})")
        };
        if remaining.is_zero() {
            return Cow::Owned(format!("Reconnecting...{attempt}"));
        }
        let seconds = remaining
            .as_secs()
            .saturating_add(u64::from(remaining.subsec_nanos() > 0));
        Cow::Owned(format!(
            "Retrying in {}{attempt}",
            fmt_elapsed_compact(seconds),
        ))
    }

    #[cfg(test)]
    pub(crate) fn details(&self) -> Option<&str> {
        self.details.as_deref()
    }

    pub(crate) fn set_interrupt_hint_visible(&mut self, visible: bool) {
        self.show_interrupt_hint = visible;
    }

    pub(crate) fn set_interrupt_binding(&mut self, binding: Option<ShortcutHint>) {
        self.interrupt_binding = binding;
    }

    pub(crate) fn with_timer<'a>(&'a self, timer: &'a StatusTimer) -> impl Renderable + 'a {
        StatusIndicator { row: self, timer }
    }

    /// Wrap the details text into a fixed width and return the lines, truncating if necessary.
    fn wrapped_details_lines(&self, width: u16) -> Vec<Line<'static>> {
        let Some(details) = self.details.as_deref() else {
            return Vec::new();
        };
        if width == 0 {
            return Vec::new();
        }

        let prefix_width = UnicodeWidthStr::width(DETAILS_PREFIX);
        let opts = RtOptions::new(usize::from(width))
            .initial_indent(Line::from(DETAILS_PREFIX.dim()))
            .subsequent_indent(Line::from(Span::from(" ".repeat(prefix_width)).dim()))
            .break_words(/*break_words*/ true);

        let mut out = word_wrap_lines(details.lines().map(|line| vec![line.dim()]), opts);

        if out.len() > self.details_max_lines {
            out.truncate(self.details_max_lines);
            let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
            let max_base_len = content_width.saturating_sub(1);
            if let Some(last) = out.last_mut()
                && let Some(span) = last.spans.last_mut()
            {
                let trimmed: String = span.content.as_ref().chars().take(max_base_len).collect();
                *span = format!("{trimmed}…").dim();
            }
        }

        out
    }
}

struct StatusIndicator<'a> {
    row: &'a StatusIndicatorWidget,
    timer: &'a StatusTimer,
}

impl StatusIndicator<'_> {
    // Share width decisions between height measurement and rendering, including
    // wide Unicode characters, remapped interrupt hints, and elapsed-time text.
    fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let row = self.row;
        let now = Instant::now();
        let elapsed_duration = self.timer.display_started_at.map_or_else(
            || self.timer.elapsed_at(now),
            |started_at| now.saturating_duration_since(started_at),
        );
        let pretty_elapsed = fmt_elapsed_compact(elapsed_duration.as_secs());
        let progress =
            MotionMode::from_animations_enabled(row.animations_enabled && row.effects.progress);
        let shimmer =
            MotionMode::from_animations_enabled(row.animations_enabled && row.effects.shimmer);

        let mut spans = Vec::with_capacity(5);
        if let Some(indicator) = activity_indicator(
            Some(self.timer.last_resume_at),
            progress,
            ReducedMotionIndicator::Hidden,
        ) {
            spans.push(indicator);
            spans.push(" ".into());
        }
        spans.extend(summary_shimmer(
            &row.header_at(now),
            now.saturating_duration_since(row.header_started_at),
            shimmer,
        ));
        if !spans.is_empty() {
            spans.push(" ".into());
        }
        if row.show_interrupt_hint
            && let Some(interrupt_binding) = row.interrupt_binding
        {
            spans.push(format!("({pretty_elapsed} • ").dim());
            spans.extend(interrupt_binding.spans());
            spans.push(" to interrupt)".dim());
        } else {
            spans.push(format!("({pretty_elapsed})").dim());
        }
        if let Some(speed) = &row.recent_generation_speed {
            spans.push(format!(" · {speed}").dim());
        }
        if let Some(message) = &row.inline_message {
            // Keep optional context after elapsed/interrupt text so that core
            // interrupt affordances stay in a fixed visual location.
            spans.push(" · ".dim());
            spans.push(message.clone().dim());
        }

        let mut header = Line::from(spans);
        let mut hook_overflow = None;
        if let Some(message) = &row.hook_status_message {
            if line_width(&header) + display_width(" · ") + display_width(message)
                <= usize::from(width)
            {
                header.spans.extend([" · ".dim(), message.clone().dim()]);
            } else {
                hook_overflow = Some(truncate_line_with_ellipsis_if_overflow(
                    Line::from(vec![DETAILS_PREFIX.dim(), message.clone().dim()]),
                    usize::from(width),
                ));
            }
        }
        let mut lines = Vec::new();
        lines.push(truncate_line_with_ellipsis_if_overflow(
            header,
            usize::from(width),
        ));
        lines.extend(hook_overflow);
        lines.extend(row.wrapped_details_lines(width));
        lines
    }
}

impl Renderable for StatusIndicator<'_> {
    fn desired_height(&self, width: u16) -> u16 {
        self.lines(width).len() as u16
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        if self.row.animations_enabled
            || self.timer.display_started_at.is_some()
            || self.row.retry_wait().is_some()
        {
            let interval_ms = if self.row.animations_enabled
                && (self.row.effects.progress || self.row.effects.shimmer)
            {
                32
            } else {
                1_000
            };
            self.row
                .frame_requester
                .schedule_frame_in(Duration::from_millis(interval_ms));
        }
        Paragraph::new(Text::from(self.lines(area.width))).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use crate::app_event_sender::AppEventSender;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use tokio::sync::mpsc::unbounded_channel;

    use pretty_assertions::assert_eq;

    #[test]
    fn retry_countdown_expires_and_clears_on_recovery() {
        let (tx, _rx) = unbounded_channel();
        let mut row = StatusIndicatorWidget::new(
            AppEventSender::new(tx),
            FrameRequester::test_dummy(),
            false,
            Default::default(),
        );
        row.update_header("Reconnecting... 1/100 (retrying in 115s)".into());
        let started = row.header_started_at;
        assert_eq!(row.header_at(started), "Retrying in 1m 55s (1/100)");
        assert_eq!(
            row.header_at(started + Duration::from_millis(114_500)),
            "Retrying in 1s (1/100)",
        );
        assert_eq!(
            row.header_at(started + Duration::from_secs(115)),
            "Reconnecting... (1/100)",
        );
        assert_eq!(
            row.header_at(started + Duration::from_secs(120)),
            "Reconnecting... (1/100)",
        );
        row.update_header("Working".into());
        assert_eq!(row.header_at(Instant::now()), "Working");
        assert!(row.retry_wait().is_none());
    }

    #[test]
    fn retry_countdown_renders_in_status_row_and_fits_narrow_widths() {
        let (tx, _rx) = unbounded_channel();
        let mut row = StatusIndicatorWidget::new(
            AppEventSender::new(tx),
            FrameRequester::test_dummy(),
            false,
            Default::default(),
        );
        row.update_header("Reconnecting... 2/100 (retrying in 449s)".into());
        let timer = StatusTimer::default();
        let indicator = StatusIndicator {
            row: &row,
            timer: &timer,
        };
        let header = indicator.lines(120)[0].to_string();
        assert!(header.starts_with("Retrying in 7m 29s (2/100)"));
        assert!(header.contains("esc to interrupt"));
        for width in [10, 30, 120] {
            assert!(display_width(&indicator.lines(width)[0].to_string()) <= usize::from(width));
        }
    }

    #[test]
    fn retry_countdown_survives_reapplying_same_status() {
        let (tx, _rx) = unbounded_channel();
        let mut row = StatusIndicatorWidget::new(
            AppEventSender::new(tx),
            FrameRequester::test_dummy(),
            false,
            Default::default(),
        );
        let header = "Reconnecting... 1/5 (retrying in 183s)";
        row.update_header(header.into());
        row.header_started_at = Instant::now() - Duration::from_secs(2);
        let started = row.header_started_at;

        // Restoring the current status after stream idle is not a new retry.
        row.update_header(header.into());
        let timer = StatusTimer::default();
        let rendered = StatusIndicator {
            row: &row,
            timer: &timer,
        }
        .lines(120)[0]
            .to_string();
        assert!(rendered.contains("Retrying in 3m 01s (1/5)"), "{rendered}");
        assert_eq!(row.header_started_at, started);
    }

    #[test]
    fn retry_countdown_restarts_for_repeated_network_waits() {
        let (tx, _rx) = unbounded_channel();
        let mut row = StatusIndicatorWidget::new(
            AppEventSender::new(tx),
            FrameRequester::test_dummy(),
            false,
            Default::default(),
        );
        let header = "Reconnecting... (retrying in 60s)";
        row.update_header(header.into());
        row.header_started_at = Instant::now() - Duration::from_secs(60);
        assert_eq!(row.header_at(Instant::now()), "Reconnecting...");
        row.update_header(header.into());
        row.restart_retry_countdown();
        assert_eq!(row.header_at(row.header_started_at), "Retrying in 1m 00s");
    }

    #[tokio::test]
    async fn retry_countdown_redraws_without_animations() {
        let (tx, _rx) = unbounded_channel();
        let (requester, mut frames) = FrameRequester::test_channel();
        let mut row = StatusIndicatorWidget::new(
            AppEventSender::new(tx),
            requester,
            false,
            Default::default(),
        );
        row.update_header("Reconnecting... 1/5 (retrying in 10s)".into());
        let timer = StatusTimer::default();
        let area = Rect::new(0, 0, 100, 1);
        row.with_timer(&timer)
            .render(area, &mut Buffer::empty(area));
        assert!(frames.try_recv().is_ok());
    }

    #[test]
    fn recent_generation_speed_renders_with_existing_activity_and_clears() {
        let (tx, _rx) = unbounded_channel();
        let mut row = StatusIndicatorWidget::new(
            AppEventSender::new(tx),
            FrameRequester::test_dummy(),
            false,
            Default::default(),
        );
        let timer = StatusTimer::default();
        row.set_recent_generation_speed(Some("42.3 token/s".into()));
        row.update_inline_message(Some("1 background terminal".into()));
        let header = |row: &StatusIndicatorWidget, width| {
            StatusIndicator { row, timer: &timer }.lines(width)[0].to_string()
        };
        let text = header(&row, 120);
        assert!(text.contains("esc to interrupt"));
        assert!(text.contains("42.3 token/s · 1 background terminal"));
        for width in [10, 30, 120] {
            assert!(display_width(&header(&row, width)) <= usize::from(width));
        }
        row.set_recent_generation_speed(None);
        assert!(!header(&row, 120).contains("token/s"));
    }

    #[test]
    fn changed_summary_restarts_shimmer_but_repeated_summary_keeps_phase() {
        let (tx, _rx) = unbounded_channel();
        let mut row = StatusIndicatorWidget::new(
            AppEventSender::new(tx),
            FrameRequester::test_dummy(),
            /*animations_enabled*/ true,
            Default::default(),
        );
        let previous = Instant::now() - Duration::from_secs(/*secs*/ 1);
        row.header_started_at = previous;
        row.update_header("Working".to_owned());
        assert_eq!(row.header_started_at, previous);
        row.update_header("Mapping the app structure".to_owned());
        assert!(row.header_started_at > previous);
    }

    #[test]
    fn fmt_elapsed_compact_formats_seconds_minutes_hours() {
        assert_eq!(fmt_elapsed_compact(/*elapsed_secs*/ 0), "0s");
        assert_eq!(fmt_elapsed_compact(/*elapsed_secs*/ 1), "1s");
        assert_eq!(fmt_elapsed_compact(/*elapsed_secs*/ 59), "59s");
        assert_eq!(fmt_elapsed_compact(/*elapsed_secs*/ 60), "1m 00s");
        assert_eq!(fmt_elapsed_compact(/*elapsed_secs*/ 61), "1m 01s");
        assert_eq!(fmt_elapsed_compact(3 * 60 + 5), "3m 05s");
        assert_eq!(fmt_elapsed_compact(59 * 60 + 59), "59m 59s");
        assert_eq!(fmt_elapsed_compact(/*elapsed_secs*/ 3600), "1h 00m 00s");
        assert_eq!(fmt_elapsed_compact(3600 + 60 + 1), "1h 01m 01s");
        assert_eq!(fmt_elapsed_compact(25 * 3600 + 2 * 60 + 3), "25h 02m 03s");
    }

    #[test]
    fn renders_with_working_header() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let timer = StatusTimer::default();
        let w = StatusIndicatorWidget::new(
            tx,
            crate::tui::FrameRequester::test_dummy(),
            /*animations_enabled*/ true,
            Default::default(),
        );

        // Render into a fixed-size test terminal and snapshot the backend.
        let mut terminal = Terminal::new(TestBackend::new(80, 2)).expect("terminal");
        terminal
            .draw(|f| w.with_timer(&timer).render(f.area(), f.buffer_mut()))
            .expect("draw");
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn renders_truncated() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let timer = StatusTimer::default();
        let w = StatusIndicatorWidget::new(
            tx,
            crate::tui::FrameRequester::test_dummy(),
            /*animations_enabled*/ true,
            Default::default(),
        );

        // Render into a fixed-size test terminal and snapshot the backend.
        let mut terminal = Terminal::new(TestBackend::new(20, 2)).expect("terminal");
        terminal
            .draw(|f| w.with_timer(&timer).render(f.area(), f.buffer_mut()))
            .expect("draw");
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn renders_wrapped_details_panama_two_lines() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut w = StatusIndicatorWidget::new(
            tx,
            crate::tui::FrameRequester::test_dummy(),
            /*animations_enabled*/ false,
            Default::default(),
        );
        w.update_details(
            Some("A man a plan a canal panama".to_string()),
            StatusDetailsCapitalization::CapitalizeFirst,
            STATUS_DETAILS_DEFAULT_MAX_LINES,
        );
        w.set_interrupt_hint_visible(/*visible*/ false);

        // Freeze time-dependent rendering (elapsed + spinner) to keep the snapshot stable.
        let mut timer = StatusTimer::default();
        timer.pause_at(timer.last_resume_at);

        // Prefix is 4 columns, so a width of 30 yields a content width of 26: one column
        // short of fitting the whole phrase (27 cols), forcing exactly one wrap without ellipsis.
        let mut terminal = Terminal::new(TestBackend::new(30, 3)).expect("terminal");
        terminal
            .draw(|f| w.with_timer(&timer).render(f.area(), f.buffer_mut()))
            .expect("draw");
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn renders_without_spinner_when_animations_disabled() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let w = StatusIndicatorWidget::new(
            tx,
            crate::tui::FrameRequester::test_dummy(),
            /*animations_enabled*/ false,
            Default::default(),
        );
        let mut timer = StatusTimer::default();
        timer.pause_at(timer.last_resume_at);

        let mut terminal = Terminal::new(TestBackend::new(80, 1)).expect("terminal");
        terminal
            .draw(|f| w.with_timer(&timer).render(f.area(), f.buffer_mut()))
            .expect("draw");
        let line = terminal.backend().buffer().content()[..80]
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();

        assert!(line.starts_with("Working (0s • esc to interrupt)"));
    }

    #[test]
    fn renders_remapped_interrupt_hint() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut w = StatusIndicatorWidget::new(
            tx,
            crate::tui::FrameRequester::test_dummy(),
            /*animations_enabled*/ false,
            Default::default(),
        );
        w.set_interrupt_binding(Some(key_hint::plain(KeyCode::F(12)).into()));
        let mut timer = StatusTimer::default();
        timer.pause_at(timer.last_resume_at);

        let mut terminal = Terminal::new(TestBackend::new(80, 1)).expect("terminal");
        terminal
            .draw(|f| w.with_timer(&timer).render(f.area(), f.buffer_mut()))
            .expect("draw");
        insta::assert_snapshot!(terminal.backend());
    }

    #[test]
    fn hook_status_reflows_without_displacing_controls_or_details() {
        let (tx, _rx) = unbounded_channel::<AppEvent>();
        let mut w = StatusIndicatorWidget::new(
            AppEventSender::new(tx),
            crate::tui::FrameRequester::test_dummy(),
            /*animations_enabled*/ false,
            Default::default(),
        );
        let mut timer = StatusTimer::default();
        timer.pause_at(timer.last_resume_at);
        w.update_hook_status_message(Some("checking 日本語 ｶﾞﾊﾟ policy".to_string()));
        w.update_details(
            Some("existing details".to_string()),
            StatusDetailsCapitalization::Preserve,
            STATUS_DETAILS_DEFAULT_MAX_LINES,
        );

        for (background, snapshot) in [
            (None, "hook_status_reflows_without_background_activity"),
            (
                Some("1 background terminal running · /ps to view · /stop to close"),
                "hook_status_reflows_with_background_activity",
            ),
        ] {
            w.update_inline_message(background.map(str::to_string));
            let mut expected = "Working (0s • esc to interrupt)".to_string();
            if let Some(background) = background {
                expected.push_str(&format!(" · {background}"));
            }
            expected.push_str(" · checking 日本語 ｶﾞﾊﾟ policy");
            let fit_width = display_width(&expected) as u16;
            let mut frames = Vec::new();
            for width in [fit_width, fit_width - 1, 24, fit_width] {
                let height = w.with_timer(&timer).desired_height(width);
                assert_eq!(height, if width >= fit_width { 2 } else { 3 });
                let mut terminal =
                    Terminal::new(TestBackend::new(width, height)).expect("terminal");
                terminal
                    .draw(|f| w.with_timer(&timer).render(f.area(), f.buffer_mut()))
                    .expect("draw");
                frames.push(format!("{width} columns:\n{}", terminal.backend()));
            }
            insta::assert_snapshot!(snapshot, frames.join("\n"));
        }
    }

    #[test]
    fn details_overflow_adds_ellipsis() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut w = StatusIndicatorWidget::new(
            tx,
            crate::tui::FrameRequester::test_dummy(),
            /*animations_enabled*/ true,
            Default::default(),
        );
        w.update_details(
            Some("abcd abcd abcd abcd".to_string()),
            StatusDetailsCapitalization::CapitalizeFirst,
            STATUS_DETAILS_DEFAULT_MAX_LINES,
        );

        let lines = w.wrapped_details_lines(/*width*/ 6);
        assert_eq!(lines.len(), STATUS_DETAILS_DEFAULT_MAX_LINES);
        let last = lines.last().expect("expected last details line");
        assert!(
            last.spans[1].content.as_ref().ends_with("…"),
            "expected ellipsis in last line: {last:?}"
        );
    }

    #[test]
    fn details_args_can_disable_capitalization_and_limit_lines() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut w = StatusIndicatorWidget::new(
            tx,
            crate::tui::FrameRequester::test_dummy(),
            /*animations_enabled*/ true,
            Default::default(),
        );
        w.update_details(
            Some("cargo test -p codex-core and then cargo test -p codex-tui".to_string()),
            StatusDetailsCapitalization::Preserve,
            /*max_lines*/ 1,
        );

        assert_eq!(
            w.details(),
            Some("cargo test -p codex-core and then cargo test -p codex-tui")
        );

        let lines = w.wrapped_details_lines(/*width*/ 24);
        assert_eq!(lines.len(), 1);
        let last = lines.last().expect("expected one details line");
        assert!(
            last.spans
                .last()
                .is_some_and(|span| span.content.as_ref().contains('…')),
            "expected one-line details to be ellipsized, got {last:?}"
        );
    }
}

#[cfg(test)]
#[path = "status_indicator_widget/effects_tests.rs"]
mod effects_tests;
