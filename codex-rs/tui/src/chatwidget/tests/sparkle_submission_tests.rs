//! Covers idle sparkle eligibility across submissions, live completions, and replay.

use super::*;
use crate::terminal_palette::with_test_default_colors;
use crate::terminal_probe::DefaultColors;
use codex_app_server_protocol::ImageReference;
use pretty_assertions::assert_eq;
use pretty_assertions::assert_ne;

fn visible(chat: &ChatWidget) -> bool {
    with_test_default_colors(
        DefaultColors {
            fg: (230, 216, 255),
            bg: (36, 27, 53),
        },
        || {
            render_bottom_popup(chat, /*width*/ 80)
                .chars()
                .any(|ch| "⠁⠂⠄⠈⠐⠠⡀⢀".contains(ch))
        },
    )
}

#[tokio::test]
async fn ordinary_and_prepared_image_submissions_hide_stars_until_live_completion() {
    let image = UserInput::Image {
        image: ImageReference::Inline {
            url: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/lFkAAAAASUVORK5CYII=".into(),
        },
        detail: None,
    };
    let text = "describe";
    for prepared_images in [None, Some(vec![image])] {
        let (mut chat, _events, mut ops) = make_chatwidget_manual(Some("gpt-6-astra")).await;
        chat.thread_id = Some(ThreadId::new());
        chat.local_settings.tui.animations = true;
        chat.local_settings.tui.effects.starfield = true;
        chat.bottom_pane
            .mark_fresh_task_for_sparkle("gpt-6-astra", &chat.local_settings.tui);
        let untouched = chat.bottom_pane.composer_draft_snapshot().sparkle_draft;
        assert_eq!(untouched, Default::default());
        assert!(visible(&chat));

        let mut expected_items = prepared_images.clone().unwrap_or_default();
        expected_items.push(UserInput::Text {
            text: text.into(),
            text_elements: Vec::new(),
        });
        let message = UserMessage::from(text);
        let (accepted, _command) = match prepared_images {
            Some(images) => chat.submit_user_message_with_prepared_images(
                message,
                UserMessageHistoryRecord::UserMessageText,
                ShellEscapePolicy::Disallow,
                UserMessageSource::Prompt,
                Some(images),
            ),
            None => chat.submit_user_message_with_history_and_shell_escape_policy(
                message,
                UserMessageHistoryRecord::UserMessageText,
                ShellEscapePolicy::Disallow,
                UserMessageSource::Prompt,
            ),
        };

        assert!(accepted);
        assert_ne!(
            chat.bottom_pane.composer_draft_snapshot().sparkle_draft,
            untouched
        );
        assert!(chat.composer_is_empty());
        assert!(chat.input_queue.user_turn_pending_start);
        assert!(!chat.turn_lifecycle.agent_turn_running);
        assert!(!visible(&chat));
        chat.refresh_model_display();
        assert!(!visible(&chat));
        chat.set_reasoning_effort(Some(ReasoningEffortConfig::Medium));
        assert!(!visible(&chat));
        chat.set_model("gpt-6.1-sol");
        chat.on_sparkle_model_selected_from_picker("gpt-6.1-sol");
        assert!(!visible(&chat));
        chat.local_settings.tui.animations = false;
        chat.refresh_model_display();
        assert!(!visible(&chat));
        chat.local_settings.tui.animations = true;
        chat.refresh_model_display();
        assert!(!visible(&chat));
        chat.local_settings.tui.effects.starfield = false;
        chat.refresh_model_display();
        assert!(!visible(&chat));
        chat.local_settings.tui.effects.starfield = true;
        chat.refresh_model_display();
        assert!(!visible(&chat));
        let Op::UserTurn { items, .. } = next_submit_op(&mut ops) else {
            unreachable!("next_submit_op only returns UserTurn")
        };
        assert_eq!(items, expected_items);
        handle_turn_started(&mut chat, "submitted-turn");
        assert!(!visible(&chat));
        handle_turn_completed(&mut chat, "submitted-turn", /*duration_ms*/ None);
        assert!(visible(&chat));
    }
}

#[tokio::test]
async fn an_empty_bang_shell_request_shows_help_and_keeps_idle_stars_without_a_user_turn() {
    let (mut chat, mut events, mut ops) = make_chatwidget_manual(Some("gpt-6.1-sol")).await;
    chat.thread_id = Some(ThreadId::new());
    chat.local_settings.tui.animations = true;
    chat.local_settings.tui.effects.starfield = true;
    chat.bottom_pane
        .mark_fresh_task_for_sparkle("gpt-6.1-sol", &chat.local_settings.tui);
    assert!(visible(&chat));

    chat.submit_user_message(UserMessage::from("!"));
    let help = drain_insert_history(&mut events)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(help.contains(USER_SHELL_COMMAND_HELP_TITLE));
    assert!(ops.try_recv().is_err());
    assert!(!chat.is_user_turn_pending_or_running());
    assert!(chat.composer_is_empty());
    assert!(visible(&chat));
}

#[tokio::test]
async fn consecutive_live_completions_enable_idle_stars_for_every_model_and_preserve_drafts() {
    for model in ["gpt-6-astra", "gpt-6.1-sol", "gpt-6-luna", "custom-model"] {
        let (mut chat, _events, _ops) = make_chatwidget_manual(Some(model)).await;
        chat.thread_id = Some(ThreadId::new());
        chat.local_settings.tui.animations = true;
        chat.local_settings.tui.effects.starfield = true;
        chat.local_settings.tui.disable_paste_burst = Some(true);
        assert!(!visible(&chat), "unconfirmed session: {model}");

        for turn_id in ["first-live-turn", "second-live-turn"] {
            handle_turn_started(&mut chat, turn_id);
            assert!(!visible(&chat), "busy: {model}, {turn_id}");
            handle_turn_completed(&mut chat, turn_id, /*duration_ms*/ None);
            assert!(visible(&chat), "idle: {model}, {turn_id}");
        }

        handle_turn_started(&mut chat, "turn-with-draft");
        chat.handle_paste("keep this draft".into());
        let draft = chat.bottom_pane.composer_draft_snapshot();
        handle_turn_completed(&mut chat, "turn-with-draft", /*duration_ms*/ None);
        assert_eq!(chat.composer_text_with_pending(), "keep this draft");
        assert_eq!(
            chat.bottom_pane.composer_draft_snapshot().sparkle_draft,
            draft.sparkle_draft
        );
        assert!(!visible(&chat), "nonempty draft: {model}");
        for _ in "keep this draft".chars() {
            chat.handle_key_event(KeyCode::Backspace.into());
        }
        assert_eq!(chat.composer_text_with_pending(), "");
        assert!(visible(&chat), "cleared draft: {model}");
    }
}

#[tokio::test]
async fn replay_completions_do_not_enable_stars_but_a_subsequent_live_completion_does() {
    for replay_kind in [
        ReplayKind::ResumeInitialMessages,
        ReplayKind::ThreadSnapshot,
    ] {
        let (mut chat, _events, _ops) = make_chatwidget_manual(Some("gpt-6.1-sol")).await;
        chat.thread_id = Some(ThreadId::new());
        chat.local_settings.tui.animations = true;
        chat.local_settings.tui.effects.starfield = true;
        assert!(!visible(&chat));
        chat.handle_server_notification(
            ServerNotification::TurnCompleted(TurnCompletedNotification {
                thread_id: chat.thread_id.unwrap().to_string(),
                turn: app_server_turn(
                    "replayed-turn",
                    AppServerTurnStatus::Completed,
                    /*duration_ms*/ None,
                    /*error*/ None,
                ),
            }),
            Some(replay_kind),
        );
        assert!(!visible(&chat));
        handle_turn_started(&mut chat, "live-after-replay");
        assert!(!visible(&chat));
        handle_turn_completed(&mut chat, "live-after-replay", /*duration_ms*/ None);
        assert!(visible(&chat));

        // Replacing the transcript also parks an already eligible idle composer.
        chat.replay_thread_turns(
            vec![app_server_turn(
                "restored-turn",
                AppServerTurnStatus::Completed,
                /*duration_ms*/ None,
                /*error*/ None,
            )],
            replay_kind,
        );
        assert!(!visible(&chat));
        handle_turn_started(&mut chat, "live-after-transcript-restore");
        assert!(!visible(&chat));
        handle_turn_completed(
            &mut chat,
            "live-after-transcript-restore",
            /*duration_ms*/ None,
        );
        assert!(visible(&chat));
    }
}

#[tokio::test]
async fn interrupted_and_failed_live_turns_restore_idle_stars_without_an_error_payload() {
    for status in [
        AppServerTurnStatus::Interrupted,
        AppServerTurnStatus::Failed,
    ] {
        let (mut chat, _events, _ops) = make_chatwidget_manual(Some("gpt-6.1-sol")).await;
        chat.thread_id = Some(ThreadId::new());
        chat.local_settings.tui.animations = true;
        chat.local_settings.tui.effects.starfield = true;
        handle_turn_started(&mut chat, "terminal-live-turn");
        assert!(!visible(&chat));
        chat.handle_server_notification(
            ServerNotification::TurnCompleted(TurnCompletedNotification {
                thread_id: chat.thread_id.unwrap().to_string(),
                turn: app_server_turn(
                    "terminal-live-turn",
                    status,
                    /*duration_ms*/ None,
                    /*error*/ None,
                ),
            }),
            /*replay_kind*/ None,
        );
        assert!(!chat.is_user_turn_pending_or_running());
        assert!(chat.composer_is_empty());
        assert!(visible(&chat));
    }
}

#[tokio::test]
async fn a_completion_that_submits_queued_follow_up_work_keeps_idle_stars_hidden() {
    let (mut chat, _events, mut ops) = make_chatwidget_manual(Some("gpt-6.1-sol")).await;
    chat.thread_id = Some(ThreadId::new());
    chat.local_settings.tui.animations = true;
    chat.local_settings.tui.effects.starfield = true;
    handle_turn_started(&mut chat, "first-turn");
    chat.input_queue
        .queued_user_messages
        .push_back(UserMessage::from("follow up").into());
    chat.refresh_pending_input_preview();

    handle_turn_completed(&mut chat, "first-turn", /*duration_ms*/ None);
    let Op::UserTurn { items, .. } = next_submit_op(&mut ops) else {
        unreachable!("next_submit_op only returns UserTurn")
    };
    assert_eq!(
        items,
        vec![UserInput::Text {
            text: "follow up".into(),
            text_elements: Vec::new(),
        }]
    );
    assert!(chat.input_queue.user_turn_pending_start);
    assert!(chat.composer_is_empty());
    assert!(!visible(&chat));
    handle_turn_started(&mut chat, "follow-up-turn");
    assert!(!visible(&chat));
    handle_turn_completed(&mut chat, "follow-up-turn", /*duration_ms*/ None);
    assert!(visible(&chat));
}
