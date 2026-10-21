//! Variable-height virtual list spike (5,000 synthetic messages).
//!
//! This is intentionally a small, dependency-free implementation: it proves
//! that the WebView renderer can keep a long chat timeline mounted without
//! rendering every row. The real timeline will reuse the same range logic with
//! measured heights instead of estimates.

use dioxus::prelude::*;

const MESSAGE_COUNT: usize = 5_000;
const OVERSCAN: usize = 6;

#[derive(Clone, Debug, PartialEq)]
struct SpikeMessage {
    id: usize,
    author: &'static str,
    text: String,
    height: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct SpikeLayout {
    offsets: Vec<f64>,
    heights: Vec<f64>,
    total_height: f64,
}

const AUTHORS: [&str; 5] = ["Ana", "Bruno", "Camila", "Diego", "Elisa"];

fn synthetic_messages() -> Vec<SpikeMessage> {
    (0..MESSAGE_COUNT)
        .map(|index| {
            let author = AUTHORS[index % AUTHORS.len()];
            let text = if index % 13 == 0 {
                format!(
                    "Mensagem #{index}: esta linha é longa de propósito para simular um \
                     parágrafo com várias linhas no meio da conversa. O objetivo é validar \
                     alturas variáveis, overscan e ancoragem por índice sem renderizar as \
                     cinco mil mensagens de uma vez."
                )
            } else if index % 7 == 0 {
                format!(
                    "Mensagem #{index}: uma linha um pouco maior para variar a altura das bolhas."
                )
            } else {
                format!("Mensagem #{index}: conteúdo curto.")
            };
            let height = estimate_height(&text);
            SpikeMessage {
                id: index,
                author,
                text,
                height,
            }
        })
        .collect()
}

/// Rough text height used by the spike. The production timeline will measure
/// rendered rows and feed the measured heights back into the same layout.
fn estimate_height(text: &str) -> f64 {
    let chars = text.chars().count() as f64;
    let lines = (chars / 64.0).ceil().max(1.0);
    30.0 + lines * 20.0
}

fn build_layout(messages: &[SpikeMessage]) -> SpikeLayout {
    let mut offsets = Vec::with_capacity(messages.len());
    let mut heights = Vec::with_capacity(messages.len());
    let mut top = 0.0;
    for message in messages {
        offsets.push(top);
        heights.push(message.height);
        top += message.height;
    }
    SpikeLayout {
        offsets,
        heights,
        total_height: top,
    }
}

fn visible_range(
    offsets: &[f64],
    scroll_top: f64,
    viewport_height: f64,
    row_count: usize,
) -> (usize, usize) {
    if row_count == 0 {
        return (0, 0);
    }

    let start = offsets
        .partition_point(|offset| *offset < scroll_top)
        .saturating_sub(1 + OVERSCAN);

    let bottom = scroll_top + viewport_height.max(1.0);
    let end = (offsets.partition_point(|offset| *offset < bottom) + OVERSCAN).min(row_count);

    (start.min(end), end)
}

#[component]
pub fn VirtualTimeline() -> Element {
    let messages = use_memo(synthetic_messages);
    let layout = use_memo(move || build_layout(&messages.read()));
    let mut scroll_top = use_signal(|| 0.0);
    let mut viewport_height = use_signal(|| 620.0f64);
    let range = use_memo(move || {
        let layout = layout.read();
        visible_range(
            &layout.offsets,
            scroll_top(),
            viewport_height(),
            layout.heights.len(),
        )
    });

    let (start, end) = *range.read();
    let total_height = layout.read().total_height;
    let visible = end.saturating_sub(start);
    let rows: Vec<(SpikeMessage, f64)> = {
        let messages = messages.read();
        let layout = layout.read();
        (start..end)
            .map(|index| (messages[index].clone(), layout.offsets[index]))
            .collect()
    };

    rsx! {
        section { class: "spike",
            header { class: "spike-header",
                div {
                    h2 { "Spike de virtualização" }
                    p { "{MESSAGE_COUNT} mensagens sintéticas, alturas variáveis e overscan de {OVERSCAN}." }
                }
                div { class: "spike-stats",
                    span { class: "pill", "{visible} no DOM" }
                    span { class: "pill", "scroll {scroll_top():.0}px" }
                }
            }
            div {
                class: "spike-scroll",
                onscroll: move |event| {
                    scroll_top.set(event.scroll_top());
                    viewport_height.set(event.client_height() as f64);
                },
                div {
                    class: "spike-canvas",
                    style: "height: {total_height}px;",
                    for (message, top) in rows {
                        article {
                            key: "{message.id}",
                            class: "spike-row",
                            style: "top: {top}px; height: {message.height}px;",
                            div { class: "avatar", "{message.author.chars().next().unwrap_or('?')}" }
                            div { class: "spike-bubble",
                                div { class: "spike-meta",
                                    strong { "{message.author}" }
                                    span { "#{message.id}" }
                                }
                                p { "{message.text}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_offsets_are_cumulative_and_total_matches() {
        let messages = synthetic_messages();
        let layout = build_layout(&messages);
        assert_eq!(layout.offsets.len(), MESSAGE_COUNT);
        assert_eq!(layout.heights.len(), MESSAGE_COUNT);
        assert_eq!(layout.offsets[0], 0.0);
        assert_eq!(layout.total_height, layout.heights.iter().sum::<f64>());
        for index in 1..MESSAGE_COUNT {
            assert_eq!(
                layout.offsets[index],
                layout.offsets[index - 1] + layout.heights[index - 1]
            );
        }
    }

    #[test]
    fn visible_range_is_bounded_at_both_ends() {
        let messages = synthetic_messages();
        let layout = build_layout(&messages);

        let (start, end) = visible_range(&layout.offsets, 0.0, 620.0, MESSAGE_COUNT);
        assert_eq!(start, 0);
        assert!(end > start);
        assert!(end < MESSAGE_COUNT / 10);

        let middle = layout.total_height / 2.0;
        let (start, end) = visible_range(&layout.offsets, middle, 620.0, MESSAGE_COUNT);
        assert!(start > 0);
        assert!(end > start);
        assert!(end < MESSAGE_COUNT);

        let (start, end) = visible_range(
            &layout.offsets,
            layout.total_height - 620.0,
            620.0,
            MESSAGE_COUNT,
        );
        assert!(start < MESSAGE_COUNT);
        assert_eq!(end, MESSAGE_COUNT);
    }

    #[test]
    fn empty_range_is_safe() {
        assert_eq!(visible_range(&[], 0.0, 620.0, 0), (0, 0));
    }

    #[test]
    fn long_messages_are_taller_than_short_ones() {
        assert!(estimate_height(&"a".repeat(400)) > estimate_height("curta"));
    }
}
