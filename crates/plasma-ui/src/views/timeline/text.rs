//! Clickable pieces of a message: links and bot commands.

use dioxus::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum MessageSpan {
    Text(String),
    Link(String),
    Command(String),
}

pub(super) fn message_spans(text: &str) -> Vec<MessageSpan> {
    let mut spans = Vec::new();
    let mut cursor = 0;
    let bytes = text.len();
    while cursor < bytes {
        if let Some(end) = take_link(text, cursor) {
            push_span(&mut spans, MessageSpan::Link(text[cursor..end].to_string()));
            cursor = end;
            continue;
        }
        if let Some(end) = take_command(text, cursor) {
            push_span(
                &mut spans,
                MessageSpan::Command(text[cursor..end].to_string()),
            );
            cursor = end;
            continue;
        }
        let next = text[cursor..]
            .char_indices()
            .nth(1)
            .map(|(offset, _)| cursor + offset)
            .unwrap_or(bytes);
        push_span(
            &mut spans,
            MessageSpan::Text(text[cursor..next].to_string()),
        );
        cursor = next;
    }
    spans
}

fn push_span(spans: &mut Vec<MessageSpan>, span: MessageSpan) {
    if let (Some(MessageSpan::Text(previous)), MessageSpan::Text(next)) = (spans.last_mut(), &span)
    {
        previous.push_str(next);
        return;
    }
    if !matches!(span, MessageSpan::Text(ref text) if text.is_empty()) {
        spans.push(span);
    }
}

fn take_link(text: &str, index: usize) -> Option<usize> {
    if !at_boundary(text, index) {
        return None;
    }
    let rest = &text[index..];
    let lower = rest.to_ascii_lowercase();
    let scheme = ["https://", "http://", "www.", "tg://"]
        .into_iter()
        .find(|prefix| lower.starts_with(prefix))
        .is_some()
        || lower.starts_with("t.me/")
        || lower.starts_with("telegram.me/")
        || lower.starts_with("telegram.dog/");
    if !scheme {
        return None;
    }
    let mut end = rest
        .find(char::is_whitespace)
        .map(|offset| index + offset)
        .unwrap_or(text.len());
    while end > index {
        let ch = text[..end].chars().next_back()?;
        if matches!(
            ch,
            ')' | ']' | '}' | '"' | '\'' | ',' | '.' | ';' | '!' | '?'
        ) {
            end -= ch.len_utf8();
        } else {
            break;
        }
    }
    (end > index).then_some(end)
}

fn take_command(text: &str, index: usize) -> Option<usize> {
    if !at_boundary(text, index) {
        return None;
    }
    let rest = &text[index..];
    let mut chars = rest.chars();
    if chars.next() != Some('/') {
        return None;
    }
    let mut consumed = '/'.len_utf8();
    let mut name_len = 0usize;
    let mut bot_len = None::<usize>;
    for ch in chars {
        if bot_len.is_none() && ch == '@' && name_len > 0 {
            bot_len = Some(0);
            consumed += ch.len_utf8();
            continue;
        }
        let in_bot = bot_len.is_some();
        let current = if in_bot {
            bot_len.unwrap_or(0)
        } else {
            name_len
        };
        let allowed = if current == 0 {
            ch.is_ascii_alphabetic()
        } else {
            ch.is_ascii_alphanumeric() || ch == '_'
        };
        if !allowed || current >= 32 {
            break;
        }
        if in_bot {
            bot_len = Some(current + 1);
        } else {
            name_len += 1;
        }
        consumed += ch.len_utf8();
    }
    if name_len == 0 {
        return None;
    }
    if bot_len == Some(0) {
        consumed -= '@'.len_utf8();
    }
    Some(index + consumed)
}

fn at_boundary(text: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    text[..index]
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '"' | '\''))
}

#[component]
pub(super) fn MessageText(
    text: String,
    on_open_link: EventHandler<String>,
    on_send_command: EventHandler<String>,
) -> Element {
    let spans = message_spans(&text);
    rsx! {
        p { class: "message-text",
            for (index, span) in spans.into_iter().enumerate() {
                match span {
                    MessageSpan::Text(value) => rsx! {
                        span { key: "{index}", "{value}" }
                    },
                    MessageSpan::Link(url) => rsx! {
                        button {
                            key: "{index}",
                            r#type: "button",
                            class: "message-link",
                            title: "Abrir link",
                            onclick: move |event| {
                                event.stop_propagation();
                                on_open_link.call(url.clone());
                            },
                            "{url}"
                        }
                    },
                    MessageSpan::Command(command) => rsx! {
                        button {
                            key: "{index}",
                            r#type: "button",
                            class: "message-command",
                            title: "Enviar comando",
                            onclick: move |event| {
                                event.stop_propagation();
                                on_send_command.call(command.clone());
                            },
                            "{command}"
                        }
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_commands_are_separate_spans() {
        let spans = message_spans("veja https://t.me/c/3977858280/18/601 e /mystatus.");
        assert_eq!(
            spans,
            vec![
                MessageSpan::Text("veja ".to_string()),
                MessageSpan::Link("https://t.me/c/3977858280/18/601".to_string()),
                MessageSpan::Text(" e ".to_string()),
                MessageSpan::Command("/mystatus".to_string()),
                MessageSpan::Text(".".to_string()),
            ]
        );
    }

    #[test]
    fn command_at_the_start_includes_the_bot() {
        let spans = message_spans("/mystatus@SomeBot agora");
        assert_eq!(
            spans[0],
            MessageSpan::Command("/mystatus@SomeBot".to_string())
        );
        assert_eq!(spans[1], MessageSpan::Text(" agora".to_string()));
    }
}
