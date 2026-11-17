//! Cached message thumbnails rendered inside the timeline and media panels.

use crate::state::{AppServices, UiState};
use dioxus::prelude::*;

/// Mounted thumbnails kept in memory before the cache is dropped wholesale.
///
/// Data URLs are small enough to keep a scrolled conversation responsive, and
/// the cap keeps a very long session from growing without bound.
const THUMBNAIL_CACHE_LIMIT: usize = 512;

#[component]
pub(crate) fn MessageThumbnail(
    chat_id: i64,
    message_id: i64,
    is_video: bool,
    on_open: EventHandler<()>,
) -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let key = (chat_id, message_id);
    // `peek` does not subscribe. Reading the shared map would rerender every
    // visible thumbnail each time one of them finished, and that rewrite kept
    // the window from processing events.
    let cached_url = ui.conversation.thumbnails.peek().get(&key).cloned();
    let already_cached = cached_url.is_some();
    let mut thumbnails = ui.conversation.thumbnails;
    let mut in_view = use_signal(|| already_cached);
    use_effect(move || {
        if already_cached || *in_view.peek() {
            return;
        }
        spawn(async move {
            let script = format!(
                r#"
                (() => {{
                    const thumbKey = {encoded};
                    const node = document.querySelector(`[data-thumb-key="${{CSS.escape(thumbKey)}}"]`);
                    if (!node) {{
                        dioxus.send("gone");
                        return;
                    }}
                    if (window.__plasmaThumbObservers == null) {{
                        window.__plasmaThumbObservers = new Map();
                    }}
                    const previous = window.__plasmaThumbObservers.get(thumbKey);
                    if (previous) {{
                        previous.disconnect();
                        window.__plasmaThumbObservers.delete(thumbKey);
                    }}
                    const observer = new IntersectionObserver((entries) => {{
                        for (const entry of entries) {{
                            if (entry.isIntersecting) {{
                                dioxus.send("visible");
                                observer.disconnect();
                                window.__plasmaThumbObservers.delete(thumbKey);
                                return;
                            }}
                        }}
                    }}, {{ root: null, rootMargin: "240px 0px", threshold: 0.01 }});
                    window.__plasmaThumbObservers.set(thumbKey, observer);
                    observer.observe(node);
                }})();
                "#,
                encoded = serde_json::to_string(&format!("{chat_id}:{message_id}"))
                    .unwrap_or_else(|_| "\"\"".into()),
            );
            let mut eval = document::eval(&script);
            if let Ok(payload) = eval.recv::<String>().await {
                if payload == "visible" {
                    in_view.set(true);
                }
            }
        });
    });

    let resource = use_resource(move || {
        let services = services.clone();
        let wanted = in_view();
        async move {
            if already_cached || !wanted {
                return Ok(None);
            }
            if let Some((_chat, message)) =
                crate::twitter_fake::find_twitter_fake_message_by_numeric(
                    &services.settings,
                    chat_id,
                    message_id,
                )
            {
                let url = message
                    .thumbnail_url
                    .or(message.url)
                    .filter(|value| !value.is_empty());
                return Ok(url);
            }
            services
                .telegram
                .thumbnail_data_url(chat_id, message_id)
                .await
        }
    });
    use_effect(move || {
        let url = {
            let state = resource.read();
            match &*state {
                Some(Ok(Some(url))) => url.clone(),
                _ => return,
            }
        };
        let mut cache = thumbnails.write();
        if cache.get(&key) == Some(&url) {
            return;
        }
        if cache.len() >= THUMBNAIL_CACHE_LIMIT {
            let drop_count = cache.len() / 2;
            let victims: Vec<_> = cache.keys().take(drop_count).copied().collect();
            for key in victims {
                cache.remove(&key);
            }
        }
        cache.insert(key, url);
    });

    let state = resource.read_unchecked();
    rsx! {
        button {
            r#type: "button",
            class: "message-thumb",
            "data-thumb-key": "{chat_id}:{message_id}",
            title: "Abrir mídia",
            onclick: move |_| on_open.call(()),
            if let Some(url) = cached_url {
                img { src: "{url}", alt: "" }
            } else {
                match &*state {
                    Some(Ok(Some(url))) => rsx! { img { src: "{url}", alt: "" } },
                    Some(Ok(None)) if in_view() => rsx! { span { class: "thumb-placeholder", "▣" } },
                    Some(Ok(None)) => rsx! { div { class: "thumb-skeleton" } },
                    Some(Err(error)) => rsx! { span { class: "thumb-placeholder", title: "{error}", "!" } },
                    None => rsx! { div { class: "thumb-skeleton" } },
                }
            }
            if is_video {
                span { class: "thumb-play", "▶" }
            }
        }
    }
}
