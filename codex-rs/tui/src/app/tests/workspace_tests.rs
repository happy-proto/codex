//! Docked threads keep task state visible and share a full-width footer.

use super::*;
use crate::bottom_pane::workspace_status::WorkspacePhase;
use pretty_assertions::assert_eq;

#[test]
fn workspace_events_remain_send_with_parked_composers() {
    fn assert_send<T: Send>() {}
    assert_send::<AppEvent>();
    assert_send::<crate::app_event_sender::AppEventSender>();
}

async fn pair() -> Result<(Box<App>, tui::Tui, ThreadId, ThreadId)> {
    let mut app = make_test_app().await;
    enable_side_retention(&mut app)?;
    // These assertions inspect settled task status, not the sidebar opening animation.
    app.local_settings.tui.animations = false;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    tui.set_owned_screen(true)?;
    let main = ThreadId::new();
    let side = ThreadId::new();
    app.primary_thread_id = Some(main);
    app.side_threads.insert(side, SideThreadState::new(main));
    app.sidebars.entry(Some(main)).or_default().show(
        sidebar::SidebarContent::Chat(side),
        sidebar::PaneFocus::Main,
    );
    app.chat_widget
        .handle_thread_session(test_thread_session(side, app.config.cwd.to_path_buf()));
    app.chat_widget.insert_str("side draft\nsecond side line");
    let (_, display, bottom) = app.chat_widget.take_side_display(40);
    app.side_companions.insert(
        side,
        side::SideCompanion {
            route: crate::app_event_sender::SideProjectionRoute {
                thread_id: side,
                generation: uuid::Uuid::new_v4(),
            },
            projection: None,
            commit_ticks: false,
            bottom,
            display,
            cells: vec![plain_line_cell("Side answer")],
            view: Default::default(),
        },
    );
    app.chat_widget
        .handle_thread_session(test_thread_session(main, app.config.cwd.to_path_buf()));
    app.active_thread_id = Some(main);
    app.chat_widget.insert_str("main draft\nsecond main line");
    app.transcript_cells = vec![Arc::new(crate::history_cell::AgentMarkdownCell::new(
        "Main answer".into(),
        app.config.cwd.as_path(),
    ))];
    Ok((app, tui, main, side))
}

#[tokio::test]
async fn workspace_passive_generation_speed_survives_idle_focus_and_rejects_finished_reads()
-> Result<()> {
    let (mut app, mut tui, main, side) = pair().await?;
    let start = |id: &str| {
        let mut event = turn_started_notification(side, id);
        if let ServerNotification::TurnStarted(n) = &mut event {
            n.turn.started_at = Some(chrono::Utc::now().timestamp() - 75);
        }
        event
    };
    app.enqueue_thread_notification(side, start("first"))
        .await?;
    assert!(app.chat_widget.live_generation_speed_target().is_none());
    assert!(app.has_live_generation_speed_target());
    let target = app.side_companions[&side]
        .bottom
        .workspace_generation_speed_target(side)
        .unwrap();
    app.finish_live_generation_speed(target.clone(), Some(23.8));
    let text = side_frame(&mut app, &mut tui, Size::new(150, 24))?;
    assert!(
        text.contains("Working (") && text.contains("23.8 token/s"),
        "{text}"
    );
    assert_eq!(app.current_displayed_thread_id(), Some(main));
    app.enqueue_thread_notification(side, start("first"))
        .await?;
    let recovered = app.side_companions[&side]
        .bottom
        .workspace_generation_speed_target(side)
        .unwrap();
    assert_ne!(target, recovered);
    app.finish_live_generation_speed(target, Some(999.0));
    assert!(!side_frame(&mut app, &mut tui, Size::new(150, 24))?.contains("token/s"));
    app.finish_live_generation_speed(recovered.clone(), Some(23.8));
    assert!(side_frame(&mut app, &mut tui, Size::new(150, 24))?.contains("23.8 token/s"));
    let target = recovered;
    app.enqueue_thread_notification(
        side,
        turn_completed_notification(side, "first", TurnStatus::Completed),
    )
    .await?;
    app.finish_live_generation_speed(target.clone(), Some(999.0));
    let text = side_frame(&mut app, &mut tui, Size::new(150, 24))?;
    assert!(
        text.contains("Worked for") && !text.contains("token/s"),
        "{text}"
    );
    assert!(!app.has_live_generation_speed_target());
    app.enqueue_thread_notification(side, start("second"))
        .await?;
    app.finish_live_generation_speed(target, Some(999.0));
    let text = side_frame(&mut app, &mut tui, Size::new(150, 24))?;
    assert!(!text.contains("token/s"), "{text}");
    let current = app.side_companions[&side]
        .bottom
        .workspace_generation_speed_target(side)
        .unwrap();
    app.finish_live_generation_speed(current.clone(), Some(42.0));
    assert!(side_frame(&mut app, &mut tui, Size::new(150, 24))?.contains("42.0 token/s"));
    app.reconnect.offline = true;
    app.finish_live_generation_speed(current, Some(99.0));
    assert!(!side_frame(&mut app, &mut tui, Size::new(150, 24))?.contains("token/s"));
    Ok(())
}

#[tokio::test]
async fn workspace_background_status_updates_without_taking_input_focus() -> Result<()> {
    let (mut app, mut tui, main, side) = pair().await?;
    app.enqueue_thread_notification(side, turn_started_notification(side, "side-turn"))
        .await?;
    let working = side_frame(&mut app, &mut tui, Size::new(120, 24))?;
    assert!(working.contains("Working"), "{working}");
    app.enqueue_thread_notification(
        side,
        turn_completed_notification(side, "side-turn", TurnStatus::Completed),
    )
    .await?;
    let complete = side_frame(&mut app, &mut tui, Size::new(120, 24))?;
    assert!(complete.contains("Worked for"), "{complete}");
    assert_eq!(app.current_displayed_thread_id(), Some(main));
    let row = |frame: &str, text| frame.lines().position(|line| line.contains(text));
    for text in [
        "side draft",
        "second side line",
        "main draft",
        "second main line",
    ] {
        assert!(row(&working, text).is_some(), "missing {text}: {working}");
        assert_eq!(row(&working, text), row(&complete, text), "{text}");
    }
    // Existing cross-thread approvals may open a foreground dialog. Measure the ordinary
    // composer above before introducing that interactive view.
    app.enqueue_thread_notification(side, turn_started_notification(side, "approval-turn"))
        .await?;
    app.enqueue_thread_request(
        side,
        exec_approval_request(side, "approval-turn", "approval", None),
    )
    .await?;
    let approval = side_frame(&mut app, &mut tui, Size::new(120, 24))?;
    assert!(approval.contains("Needs approval"), "{approval}");
    assert_eq!(app.current_displayed_thread_id(), Some(main));
    Ok(())
}

#[tokio::test]
async fn workspace_footer_spans_the_divider_and_resizes_on_short_screens() -> Result<()> {
    let (mut app, mut tui, _, _) = pair().await?;
    for (width, height) in [(120, 24), (100, 10), (150, 6), (80, 12), (120, 24)] {
        let text = side_frame(&mut app, &mut tui, Size::new(width, height))?;
        if width >= 100 {
            let lines = text.lines().collect::<Vec<_>>();
            assert!(
                lines[..lines.len() - 2]
                    .iter()
                    .any(|line| line.contains('│')),
                "{text}"
            );
            assert!(
                lines[lines.len() - 2..]
                    .iter()
                    .all(|line| !line.contains('│')),
                "{text}"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn workspace_clock_survives_replay_and_pending_input_resolution() {
    let id = ThreadId::new();
    let mut store = ThreadEventStore::new(16);
    let mut started = turn_started_notification(id, "turn");
    if let ServerNotification::TurnStarted(n) = &mut started {
        n.turn.started_at = Some(chrono::Utc::now().timestamp() - 75);
    }
    store.push_notification(started.clone());
    let before = store.workspace_status();
    assert!(before.elapsed() >= Duration::from_secs(75));
    let elapsed_before_replay = before.elapsed();
    store.push_notification(started);
    assert!(store.workspace_status().elapsed() >= elapsed_before_replay);
    store.push_request(request_user_input_request(id, "turn", "question"));
    assert_eq!(store.workspace_status().phase, WorkspacePhase::NeedsInput);
    store.push_notification(ServerNotification::ServerRequestResolved(
        codex_app_server_protocol::ServerRequestResolvedNotification {
            thread_id: id.to_string(),
            request_id: AppServerRequestId::Integer(2),
        },
    ));
    assert_eq!(store.workspace_status().phase, WorkspacePhase::Working);
    store.push_notification(turn_completed_notification(id, "turn", TurnStatus::Failed));
    let finished = store.workspace_status();
    assert_eq!(finished.phase, WorkspacePhase::Failed);
    assert_eq!(finished.elapsed(), store.workspace_status().elapsed());
}

#[tokio::test]
async fn workspace_search_and_copy_keep_both_hint_rows_and_the_shared_cursor() -> Result<()> {
    let (mut app, mut tui, _, _) = pair().await?;
    side_frame(&mut app, &mut tui, Size::new(120, 24))?;
    app.transcript_view.begin_search();
    app.transcript_view.paste_search("Main");
    let text = side_frame(&mut app, &mut tui, Size::new(120, 24))?;
    let footer = app
        .transcript_view
        .footer_with_navigation(118, crate::motion::MotionMode::Reduced, "esc latest")
        .expect("search footer");
    for line in &footer.text.lines {
        let content = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(text.contains(content.trim()), "missing {content}: {text}");
    }
    assert_eq!(tui.terminal.last_known_cursor_pos.y, 22);
    app.transcript_view.cancel_search();
    assert!(
        app.transcript_view
            .begin_copy_mode(&app.transcript_cells, None)
    );
    let copied = side_frame(&mut app, &mut tui, Size::new(120, 24))?;
    assert!(copied.contains("enter copy"), "{copied}");
    Ok(())
}
