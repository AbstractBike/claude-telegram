// Matrix message sender module

use anyhow::Result;
use matrix_sdk::Room;
use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use metrics::counter;

/// Split a message into chunks of at most `max_len` characters.
///
/// If the message fits within `max_len`, it is returned as a single-element
/// vector. Otherwise the text is split on byte boundaries (using lossy UTF-8
/// conversion to stay safe with multi-byte characters).
pub fn chunk_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }
    text.as_bytes()
        .chunks(max_len)
        .map(|chunk| String::from_utf8_lossy(chunk).to_string())
        .collect()
}

/// Send a text message to a Matrix room, automatically chunking if it exceeds
/// the 4000 character limit.
pub async fn send_text(room: &Room, text: &str) -> Result<()> {
    let room_id = room.room_id().to_string();
    for chunk in chunk_message(text, 4000) {
        let content = RoomMessageEventContent::text_plain(chunk);
        room.send(content).await?;
        counter!("claude_chat_matrix_messages_sent_total",
            "room" => room_id.clone(), "type" => "response")
            .increment(1);
    }
    Ok(())
}
