//! 中文标点边界的加粗兼容层；在受影响的行内容器中重新配对双星号，不改写 Markdown 源码。

use pulldown_cmark::Event;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::ops::Range;

type LocatedEvent<'a> = (Event<'a>, Range<usize>);

pub(super) fn events<'a, I>(source: &'a str, iter: I) -> impl Iterator<Item = LocatedEvent<'a>>
where
    I: Iterator<Item = LocatedEvent<'a>>,
{
    CjkStrong {
        source,
        iter,
        enabled: source.contains("**") && source.chars().any(is_cjk_letter),
        pending: VecDeque::new(),
    }
}

struct CjkStrong<'a, I> {
    source: &'a str,
    iter: I,
    enabled: bool,
    pending: VecDeque<LocatedEvent<'a>>,
}

impl<'a, I> Iterator for CjkStrong<'a, I>
where
    I: Iterator<Item = LocatedEvent<'a>>,
{
    type Item = LocatedEvent<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.enabled {
            return self.iter.next();
        }
        if let Some(event) = self.pending.pop_front() {
            return Some(event);
        }
        let event = self.iter.next()?;
        if !matches!(
            event.0,
            Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::TableCell)
        ) {
            return Some(event);
        }
        // 按行内容器处理，避免跨段落匹配或缓存整篇会话。
        let mut block = vec![event];
        let mut depth = 1;
        for event in self.iter.by_ref() {
            match &event.0 {
                Event::Start(_) => depth += 1,
                Event::End(_) => depth -= 1,
                _ => {}
            }
            block.push(event);
            if depth == 0 {
                break;
            }
        }
        self.pending = repair(self.source, block);
        self.pending.pop_front()
    }
}

struct Delimiter {
    start: usize,
    frame: Vec<usize>,
    opens: bool,
    closes: bool,
    relaxed_open: bool,
    relaxed_close: bool,
}

fn repair<'a>(source: &'a str, block: Vec<LocatedEvent<'a>>) -> VecDeque<LocatedEvent<'a>> {
    // 多处中文加粗可能让 CommonMark 错配中间的双星号。先展开双星号 Strong，
    // 再统一匹配；下划线和三重星号的原有语义不受影响。
    let expanded = expand_strong(source, &block);
    // 只允许原文中的字面星号；转义、实体、代码和 HTML 不进入匹配集合。
    let mut stars = BTreeMap::new();
    let mut frame = Vec::new();
    for (index, (event, range)) in expanded.iter().enumerate() {
        match event {
            Event::Start(_) => frame.push(index),
            Event::End(_) => {
                frame.pop();
            }
            Event::Text(text) if source.get(range.clone()) == Some(text.as_ref()) => {
                for (offset, byte) in text.bytes().enumerate() {
                    if byte == b'*' {
                        stars.insert(range.start + offset, frame.clone());
                    }
                }
            }
            _ => {}
        }
    }
    let mut delimiters = Vec::new();
    for (&start, frame) in &stars {
        if stars.get(&(start + 1)) != Some(frame)
            || source.as_bytes().get(start.wrapping_sub(1)) == Some(&b'*')
            || source.as_bytes().get(start + 2) == Some(&b'*')
            || source[..start]
                .bytes()
                .rev()
                .take_while(|byte| *byte == b'\\')
                .count()
                % 2
                == 1
        {
            continue;
        }
        let before = source[..start].chars().next_back();
        let after = source[start + 2..].chars().next();
        let before_space = before.is_none_or(char::is_whitespace);
        let after_space = after.is_none_or(char::is_whitespace);
        let before_punct = before.is_some_and(is_punctuation);
        let after_punct = after.is_some_and(is_punctuation);
        let relaxed_open = before.is_some_and(is_cjk_letter) && after_punct;
        let relaxed_close = before_punct && after.is_some_and(is_cjk_letter);
        delimiters.push(Delimiter {
            start,
            frame: frame.clone(),
            opens: !after_space && (!after_punct || before_space || before_punct || relaxed_open),
            closes: !before_space && (!before_punct || after_space || after_punct || relaxed_close),
            relaxed_open,
            relaxed_close,
        });
    }
    if !delimiters
        .iter()
        .any(|delimiter| delimiter.relaxed_open || delimiter.relaxed_close)
    {
        return block.into();
    }
    let mut openers: Vec<Delimiter> = Vec::new();
    let mut replacements = BTreeMap::new();
    for delimiter in delimiters {
        if delimiter.closes
            && let Some(index) = openers
                .iter()
                .rposition(|opener| opener.frame == delimiter.frame)
        {
            let opener = openers.remove(index);
            let range = opener.start..delimiter.start + 2;
            replacements.insert(opener.start, (Event::Start(Tag::Strong), range.clone()));
            replacements.insert(delimiter.start, (Event::End(TagEnd::Strong), range));
        } else if delimiter.opens {
            openers.push(delimiter);
        }
    }
    if replacements.is_empty() {
        return block.into();
    }
    let mut output = VecDeque::new();
    for (event, range) in expanded {
        let Event::Text(ref text) = event else {
            output.push_back((event, range));
            continue;
        };
        if source.get(range.clone()) != Some(text.as_ref()) {
            output.push_back((event, range));
            continue;
        }
        let mut cursor = range.start;
        for position in range.clone() {
            if let Some(replacement) = replacements.get(&position) {
                if cursor < position {
                    output.push_back((
                        Event::Text(source[cursor..position].into()),
                        cursor..position,
                    ));
                }
                output.push_back(replacement.clone());
                cursor = (position + 2).min(range.end);
            } else if replacements.contains_key(&position.wrapping_sub(1)) && cursor == position {
                // 双星号被解析器分成相邻 Text 事件时，消费第二个星号。
                cursor += 1;
            }
        }
        if cursor < range.end {
            output.push_back((
                Event::Text(source[cursor..range.end].into()),
                cursor..range.end,
            ));
        }
    }
    output
}

fn expand_strong<'a>(source: &'a str, block: &[LocatedEvent<'a>]) -> Vec<LocatedEvent<'a>> {
    let mut expanded = Vec::new();
    let mut stack = Vec::new();
    for (event, range) in block {
        match event {
            Event::Start(Tag::Strong) => {
                let double = range.len() >= 4
                    && source.get(range.start..range.start + 2) == Some("**")
                    && source.get(range.end - 2..range.end) == Some("**")
                    && source.as_bytes().get(range.start.wrapping_sub(1)) != Some(&b'*')
                    && source.as_bytes().get(range.start + 2) != Some(&b'*')
                    && source.as_bytes().get(range.end - 3) != Some(&b'*')
                    && source.as_bytes().get(range.end) != Some(&b'*');
                stack.push(double);
                if double {
                    expanded.push((Event::Text("**".into()), range.start..range.start + 2));
                } else {
                    expanded.push((event.clone(), range.clone()));
                }
            }
            Event::End(TagEnd::Strong) if stack.pop() == Some(true) => {
                expanded.push((Event::Text("**".into()), range.end - 2..range.end));
            }
            _ => expanded.push((event.clone(), range.clone())),
        }
    }
    expanded
}

// 本层针对中文与相邻全角标点；其它 Unicode 标点沿用原解析器规则。
fn is_punctuation(c: char) -> bool {
    c.is_ascii_punctuation()
        || matches!(c, '\u{2010}'..='\u{2027}' | '\u{2030}'..='\u{205e}'
            | '\u{3001}'..='\u{3003}' | '\u{3008}'..='\u{3011}'
            | '\u{3014}'..='\u{301f}' | '\u{fe10}'..='\u{fe19}'
            | '\u{fe30}'..='\u{fe52}' | '\u{fe54}'..='\u{fe61}'
            | '\u{fe63}' | '\u{fe68}' | '\u{fe6a}'..='\u{fe6b}'
            | '\u{ff01}'..='\u{ff0f}' | '\u{ff1a}'..='\u{ff20}'
            | '\u{ff3b}'..='\u{ff40}' | '\u{ff5b}'..='\u{ff65}')
}

fn is_cjk_letter(c: char) -> bool {
    !is_punctuation(c)
        && matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3100}'..='\u{312f}'
            | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}'
            | '\u{ac00}'..='\u{d7af}' | '\u{f900}'..='\u{faff}'
            | '\u{20000}'..='\u{3fffd}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use pulldown_cmark::Parser;

    fn parse(source: &str) -> Vec<LocatedEvent<'_>> {
        events(source, Parser::new(source).into_offset_iter()).collect()
    }

    #[test]
    fn cjk_strong_preserves_literal_markdown_and_standard_ascii() {
        for source in [
            "`**内容。**后续`",
            "```text\n**内容。**后续\n```",
            r"\*\*内容。\*\*后续",
            "&#42;&#42;内容。&#42;&#42;后续",
            "**内容。 **后续",
            "** 内容。**后续",
            "**内容。\n\n**后续",
            "**Sentence.**Next",
            "**普通加粗**",
            "***加粗斜体***",
            "**未结束。",
        ] {
            assert_eq!(
                parse(source),
                Parser::new(source).into_offset_iter().collect::<Vec<_>>(),
                "{source}"
            );
        }
    }

    #[test]
    fn cjk_strong_keeps_decoded_entities_and_balanced_inline_structure() {
        for source in [
            "**内容 &amp; 后续。**继续",
            "**内容 *斜体*。**继续",
            "**内容 `代码`。**继续",
            "**[链接](https://example.com)。**继续",
            "前文**（内容）**后文",
            "**内容:通过.**后续",
        ] {
            let parsed = parse(source);
            let mut stack = Vec::new();
            let mut strong = 0;
            for (event, range) in &parsed {
                assert!(range.start <= range.end && range.end <= source.len());
                assert!(source.is_char_boundary(range.start) && source.is_char_boundary(range.end));
                match event {
                    Event::Start(tag) => {
                        strong += usize::from(matches!(tag, Tag::Strong));
                        stack.push(tag.to_end());
                    }
                    Event::End(tag) => assert_eq!(stack.pop(), Some(*tag)),
                    Event::Text(text) => assert!(!text.contains("**"), "{source}: {parsed:?}"),
                    _ => {}
                }
            }
            assert_eq!(strong, 1, "{source}: {parsed:?}");
            assert!(stack.is_empty());
        }
        assert!(
            parse("**内容 &amp; 后续。**继续")
                .iter()
                .any(|(event, _)| matches!(event, Event::Text(text) if text.contains('&')))
        );
    }

    #[test]
    fn cjk_strong_does_not_cross_link_boundaries() {
        let source = "**前文[内容。**后续](https://example.com)";
        assert_eq!(
            parse(source),
            Parser::new(source).into_offset_iter().collect::<Vec<_>>()
        );
    }
}
