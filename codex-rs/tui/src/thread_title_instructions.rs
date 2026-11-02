use codex_app_server_protocol::AdditionalContextEntry;
use codex_app_server_protocol::AdditionalContextKind;
use std::collections::HashMap;

pub(crate) const FIRST_TURN_TITLE_INSTRUCTIONS: &str = "\
在这个新会话的首轮中，先完成对用户请求的基本调研，再为当前任务命名。通过调研，对实际工作内容、范围和相关背景形成有依据的初步认识后，调用 codex_tui 的任务管理工具 set_thread_title。必要时先查找该工具。调用时省略 threadId，并设置 onlyIfUnnamed 为 true，优先保留已有标题，包括手动设置的标题。
这份认识应来自任务所需的相关信息。查看主要信息来源及其相互关系，并考虑重要的信息缺口或不确定性。仅知道用户想做什么，不代表已经完成基本调研；应收集足够依据，了解实际工作涉及什么，再据此命名。无需等待详尽分析、实现或整个回合全部完成。
标题使用用户的语言，简洁、具体、单行，最多 36 个字符，尽可能少于 5 个词。保留工单编号，不使用引号、Markdown 或末尾标点。
只在首轮执行一次，后续回合不要自动重命名。如果工具不可用，继续完成用户任务。";

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
