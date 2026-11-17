use serde::{Deserialize, Serialize};
use tdlib_rs::{
    enums::{InputMessageContent, ReactionType},
    functions,
    types::{InputMessageText, ReactionTypeEmoji},
};

use super::*;

pub async fn tdlib_send_message(
    state: &TdlibManager,
    chat_id: i64,
    text: String,
    reply_to_id: Option<i64>,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> plasma_core::Result<TdlibSendResult> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibSendResult {
                success: false,
                message: None,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Ok(TdlibSendResult {
            success: false,
            message: None,
            error: Some("Mensagem vazia.".to_string()),
        });
    }

    let content = InputMessageContent::InputMessageText(InputMessageText {
        text: plain_formatted_text(trimmed),
        link_preview_options: None,
        clear_draft: true,
    });

    match functions::send_message(
        chat_id,
        message_topic_from_id(topic_id, topic_kind.as_deref()),
        reply_to_from_id(reply_to_id),
        None,
        content,
        client_id,
    )
    .await
    {
        Ok(tdlib_rs::enums::Message::Message(message)) => Ok(TdlibSendResult {
            success: true,
            message: Some(tdlib_message_to_app(message)),
            error: None,
        }),
        Err(error) => Ok(TdlibSendResult {
            success: false,
            message: None,
            error: Some(error.message),
        }),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibActionResult {
    pub success: bool,
    pub error: Option<String>,
}

/// Adds or removes an emoji reaction on a message.
pub async fn tdlib_toggle_message_reaction(
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
    emoji: String,
    add: bool,
) -> plasma_core::Result<TdlibActionResult> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibActionResult {
                success: false,
                error: status.error.or(Some(status.state)),
            });
        }
    };
    let reaction = ReactionType::Emoji(ReactionTypeEmoji { emoji });
    let message_id = tdlib_message_id_from_app_id(message_id);
    let result = if add {
        functions::add_message_reaction(chat_id, message_id, reaction, false, true, client_id).await
    } else {
        functions::remove_message_reaction(chat_id, message_id, reaction, client_id).await
    };
    Ok(match result {
        Ok(()) => TdlibActionResult {
            success: true,
            error: None,
        },
        Err(error) => TdlibActionResult {
            success: false,
            error: Some(error.message),
        },
    })
}

pub async fn tdlib_send_media(
    state: &TdlibManager,
    chat_id: i64,
    file_path: String,
    caption: Option<String>,
    reply_to_id: Option<i64>,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> plasma_core::Result<TdlibSendResult> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibSendResult {
                success: false,
                message: None,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    if file_path.trim().is_empty() {
        return Ok(TdlibSendResult {
            success: false,
            message: None,
            error: Some("Arquivo inválido.".to_string()),
        });
    }

    let content = input_content_for_local_media(&file_path, caption.as_deref().unwrap_or(""));
    match functions::send_message(
        chat_id,
        message_topic_from_id(topic_id, topic_kind.as_deref()),
        reply_to_from_id(reply_to_id),
        None,
        content,
        client_id,
    )
    .await
    {
        Ok(tdlib_rs::enums::Message::Message(message)) => Ok(TdlibSendResult {
            success: true,
            message: Some(tdlib_message_to_app(message)),
            error: None,
        }),
        Err(error) => Ok(TdlibSendResult {
            success: false,
            message: None,
            error: Some(error.message),
        }),
    }
}

pub async fn tdlib_forward_message(
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
    to_chat_id: i64,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> plasma_core::Result<TdlibForwardResult> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibForwardResult {
                success: false,
                messages: Vec::new(),
                error: status.error.or(Some(status.state)),
            });
        }
    };

    match functions::forward_messages(
        to_chat_id,
        message_topic_from_id(topic_id, topic_kind.as_deref()),
        chat_id,
        vec![tdlib_message_id_from_app_id(message_id)],
        None,
        false,
        false,
        client_id,
    )
    .await
    {
        Ok(tdlib_rs::enums::Messages::Messages(messages)) => Ok(TdlibForwardResult {
            success: true,
            messages: messages
                .messages
                .into_iter()
                .flatten()
                .map(tdlib_message_to_app)
                .collect(),
            error: None,
        }),
        Err(error) => Ok(TdlibForwardResult {
            success: false,
            messages: Vec::new(),
            error: Some(error.message),
        }),
    }
}
