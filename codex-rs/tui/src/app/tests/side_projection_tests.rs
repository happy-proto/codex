use super::*;
use crate::app_event::CapturedSideStream;
use crate::app_event_sender::SideProjectionRoute;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use codex_app_server_protocol::AgentMessageDeltaNotification;
use pretty_assertions::assert_eq;

fn new_route(thread_id: ThreadId) -> SideProjectionRoute {
    SideProjectionRoute {
        thread_id,
        generation: uuid::Uuid::new_v4(),
    }
}

fn insert_history_event(text: &str) -> AppEvent {
    AppEvent::InsertHistoryCell(Box::new(PlainHistoryCell::new(vec![Line::from(
        text.to_string(),
    )])))
}

#[tokio::test]
async fn workspace_parked_sender_scopes_transcript_events_and_drops_commands() {
    let thread_id = ThreadId::new();
    let route = new_route(thread_id);
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let sender = crate::app_event_sender::AppEventSender::new(tx);
    sender.side_projection.set(route).unwrap();
    let clone = sender.clone();

    sender.send(insert_history_event("background history"));
    clone.send(AppEvent::ConsolidateProposedPlan(
        "background plan".to_string(),
    ));
    clone.interrupt();

    let first = events.try_recv().expect("history event is routed");
    let second = events.try_recv().expect("plan event is routed");
    assert!(events.try_recv().is_err(), "parked command must be dropped");
    for event in [first, second] {
        match event {
            AppEvent::SideProjectionEvent {
                route: event_route,
                event,
            } => {
                assert_eq!(event_route, route);
                assert!(matches!(
                    *event,
                    AppEvent::InsertHistoryCell(_) | AppEvent::ConsolidateProposedPlan(_)
                ));
            }
            other => panic!("expected scoped side projection event, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn workspace_parked_projection_keeps_the_foreground_terminal_title() {
    let (mut projection, _sender, _events, _ops) = make_chatwidget_manual_with_sender().await;
    projection.last_terminal_title = Some("foreground title".to_string());
    projection.park_side_projection(new_route(ThreadId::new()));

    projection
        .clear_managed_terminal_title()
        .expect("parked projection leaves terminal title alone");
    projection.pre_draw_tick();

    assert_eq!(
        projection.last_terminal_title.as_deref(),
        Some("foreground title")
    );
}

#[tokio::test]
async fn workspace_retained_projection_stream_and_history_stay_on_the_background_route() {
    let (mut app, _app_events, _ops) = make_test_app_with_channels().await;
    let main_thread = ThreadId::new();
    let side_thread = ThreadId::new();
    app.primary_thread_id = Some(main_thread);
    app.active_thread_id = Some(main_thread);
    app.chat_widget.handle_thread_session(test_thread_session(
        main_thread,
        app.config.cwd.to_path_buf(),
    ));
    app.transcript_cells = vec![plain_line_cell("foreground transcript")];

    let (mut projection, sender, mut projection_events, _projection_ops) =
        make_chatwidget_manual_with_sender().await;
    projection.handle_thread_session(test_thread_session(
        side_thread,
        app.config.cwd.to_path_buf(),
    ));
    // Session setup can emit ordinary foreground events before this widget is parked.
    while projection_events.try_recv().is_ok() {}
    let route = new_route(side_thread);
    projection.park_side_projection(route);
    sender.side_projection.set(route).unwrap();
    let (_, display, bottom) = app.chat_widget.take_side_display(100);
    let background_history = plain_line_cell("background history");
    app.side_companions.insert(
        side_thread,
        side::SideCompanion {
            route,
            projection: Some(Box::new(projection)),
            commit_ticks: false,
            bottom,
            display,
            cells: vec![background_history],
            view: Default::default(),
        },
    );

    let db = codex_state::StateRuntime::init(app.config.sqlite_config().clone(), "test".into())
        .await
        .unwrap();
    let metadata = codex_state::ThreadMetadataBuilder::new(
        side_thread,
        app.config
            .codex_home
            .join("rollout-side.jsonl")
            .to_path_buf(),
        chrono::Utc::now(),
        codex_protocol::protocol::SessionSource::Cli,
    )
    .build("test");
    db.upsert_thread(&metadata).await.unwrap();
    db.save_generation_speed(
        side_thread,
        "side-turn",
        220,
        10_000_000,
        codex_state::GenerationSpeedCompletion {
            started_at: 10,
            completed_at: 14,
            duration_ms: 4000,
        },
    )
    .await
    .unwrap();
    let foreground_record = db
        .generation_speeds(side_thread)
        .await
        .unwrap()
        .remove("side-turn")
        .unwrap();
    app.chat_widget
        .generation_speeds
        .insert((main_thread, "foreground-turn".into()), foreground_record);
    app.state_db = Some(db);
    app.refresh_parked_generation_speeds(side_thread).await;
    assert!(
        app.chat_widget
            .generation_speeds
            .contains_key(&(main_thread, "foreground-turn".into()))
    );
    assert_eq!(
        app.side_companions[&side_thread]
            .projection
            .as_ref()
            .unwrap()
            .generation_speeds
            .get(&(side_thread, "side-turn".into()))
            .map(codex_state::GenerationSpeedRecord::tokens_per_second),
        Some(22.0)
    );

    // A delta updates the retained widget's source-backed stream while the foreground widget
    // and transcript continue to belong to the main thread.
    app.deliver_side_projection_notification(
        side_thread,
        &turn_started_notification(side_thread, "side-turn"),
    );
    app.deliver_side_projection_notification(
        side_thread,
        &ServerNotification::ItemStarted(codex_app_server_protocol::ItemStartedNotification::new(
            side_thread.to_string(),
            "side-turn".into(),
            ThreadItemPayload::Reasoning {
                id: "side-reasoning".into(),
                summary: Vec::new(),
                content: Vec::new(),
            },
            0,
        )),
    );
    app.deliver_side_projection_notification(
        side_thread,
        &ServerNotification::ReasoningSummaryTextDelta(
            codex_app_server_protocol::ReasoningSummaryTextDeltaNotification {
                thread_id: side_thread.to_string(),
                turn_id: "side-turn".into(),
                item_id: "side-reasoning".into(),
                delta: "Inspecting the retained projection route".into(),
                summary_index: 0,
            },
        ),
    );
    app.deliver_side_projection_notification(
        side_thread,
        &ServerNotification::ItemCompleted(
            codex_app_server_protocol::ItemCompletedNotification::new(
                side_thread.to_string(),
                "side-turn".into(),
                ThreadItemPayload::Reasoning {
                    id: "side-reasoning".into(),
                    summary: vec!["Inspecting the retained projection route".into()],
                    content: Vec::new(),
                },
                None,
                0,
            ),
        ),
    );
    app.deliver_side_projection_notification(
        side_thread,
        &ServerNotification::AgentMessageDelta(AgentMessageDeltaNotification {
            thread_id: side_thread.to_string(),
            turn_id: "side-turn".to_string(),
            item_id: "side-answer".to_string(),
            delta: "live background answer".to_string(),
        }),
    );
    while let Ok(event) = projection_events.try_recv() {
        match event {
            AppEvent::SideProjectionEvent {
                route: event_route,
                event,
            } => app.handle_side_projection_event(event_route, *event),
            other => panic!("expected side projection event, got {other:?}"),
        }
    }
    let stream_text = match app.side_companions[&side_thread]
        .projection
        .as_deref()
        .unwrap()
        .side_stream_snapshot()
        .expect("retained answer stream")
    {
        CapturedSideStream::Answer(cell) => cell
            .transcript_lines(100)
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>(),
        CapturedSideStream::Plan(_) => panic!("expected an answer stream"),
    };
    assert!(
        stream_text.contains("live background answer"),
        "{stream_text}"
    );
    assert_eq!(app.current_displayed_thread_id(), Some(main_thread));
    assert_eq!(app.transcript_cells.len(), 1);
    let initial_background_cells = app.side_companions[&side_thread].cells.len();
    let reasoning = app.side_companions[&side_thread]
        .cells
        .iter()
        .flat_map(|cell| cell.transcript_lines(120))
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        reasoning
            .matches("Inspecting the retained projection route")
            .count(),
        1,
        "{reasoning}"
    );

    // The retained sender's cloned handle routes transcript mutations with its generation.
    sender.clone().send(insert_history_event("new side item"));
    let routed = projection_events.try_recv().expect("scoped history event");
    let (event_route, event) = match routed {
        AppEvent::SideProjectionEvent { route, event } => (route, *event),
        other => panic!("expected side projection event, got {other:?}"),
    };
    assert_eq!(event_route, route);

    let stale = SideProjectionRoute {
        generation: uuid::Uuid::new_v4(),
        ..route
    };
    app.handle_side_projection_event(stale, event);
    assert_eq!(
        app.side_companions[&side_thread].cells.len(),
        initial_background_cells
    );
    sender.send(insert_history_event("new side item"));
    let routed = projection_events
        .try_recv()
        .expect("second scoped history event");
    let (event_route, event) = match routed {
        AppEvent::SideProjectionEvent { route, event } => (route, *event),
        other => panic!("expected side projection event, got {other:?}"),
    };
    assert_eq!(event_route, route);
    app.handle_side_projection_event(event_route, event);
    assert_eq!(
        app.side_companions[&side_thread].cells.len(),
        initial_background_cells + 1
    );
    assert_eq!(app.transcript_cells.len(), 1);
    assert_eq!(app.current_displayed_thread_id(), Some(main_thread));

    app.deliver_side_projection_notification(
        side_thread,
        &ServerNotification::ItemCompleted(
            codex_app_server_protocol::ItemCompletedNotification::new(
                side_thread.to_string(),
                "side-turn".into(),
                ThreadItemPayload::AgentMessage {
                    id: "side-answer".into(),
                    text: "live background answer\nfinal background answer".into(),
                    phase: None,
                    memory_citation: None,
                    delivery: None,
                    questions: None,
                },
                None,
                0,
            ),
        ),
    );
    while let Ok(event) = projection_events.try_recv() {
        match event {
            AppEvent::SideProjectionEvent { route, event } => {
                app.handle_side_projection_event(route, *event)
            }
            other => panic!("unexpected foreground event from projection: {other:?}"),
        }
    }
    let text = app.side_companions[&side_thread]
        .cells
        .iter()
        .flat_map(|cell| cell.transcript_lines(120))
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(text.matches("live background answer").count(), 1, "{text}");
    assert_eq!(text.matches("final background answer").count(), 1, "{text}");
    assert_eq!(app.transcript_cells.len(), 1);
}
