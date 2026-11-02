use codex_app_server_protocol::AdditionalContextEntry;
use codex_app_server_protocol::AdditionalContextKind;
use std::collections::HashMap;

pub(crate) const FIRST_TURN_TITLE_INSTRUCTIONS: &str = "\
During the first turn of this new conversation, first carry out a basic investigation of the user's request. Once the actual task, its scope, and the relevant context are reasonably clear, use the codex_tui task-management tool set_thread_title to name the current task. Discover the tool if necessary. Omit threadId and set onlyIfUnnamed to true so an existing title, including a manual title, takes precedence.
If the request depends on a file, message, URL, or other external reference, read the relevant content and follow up on important context needed for that basic investigation. Base the title on what you have established, rather than only the initial request or a quick glance at one reference. You do not need to wait until the entire task or turn is complete.
Use a concise, specific, single-line title in the user's language, at most 36 characters and under five words where possible. Preserve ticket references. Do not use quotes, Markdown, or trailing punctuation.
Do this once during the first turn. Do not automatically rename the conversation on later turns. If the tool is unavailable, continue the user's task without it.";

pub(crate) fn first_turn_title_context() -> HashMap<String, AdditionalContextEntry> {
    HashMap::from([(
        "codex_tui_thread_title".to_string(),
        AdditionalContextEntry {
            value: FIRST_TURN_TITLE_INSTRUCTIONS.to_string(),
            kind: AdditionalContextKind::Application,
            name: None,
        },
    )])
}
