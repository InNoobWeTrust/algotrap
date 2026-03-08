use teloxide::prelude::*;
use teloxide::types::InputFile;
use tracing::warn;

/// Send analysis results (chart + text) to a Telegram chat.
pub async fn send_analysis(
    bot: &Bot,
    chat_id: ChatId,
    symbol: &str,
    analysis_text: &str,
    chart_png: Option<&[u8]>,
) -> Result<(), Box<dyn core::error::Error + Send + Sync>> {
    // Send chart image if available
    if let Some(png_bytes) = chart_png {
        let input_file = InputFile::memory(png_bytes.to_vec()).file_name("chart.png");
        bot.send_photo(chat_id, input_file)
            .caption(format!("📊 {} Multi-TF Chart", symbol))
            .await?;
    }

    // Send analysis text (split if too long for Telegram's 4096 char limit)
    for chunk in split_message(analysis_text, 4000) {
        if let Err(e) = bot.send_message(chat_id, &chunk).await {
            warn!("Failed to send message chunk: {e}");
        }
    }

    Ok(())
}

/// Split a message into chunks for Telegram's character limit.
///
/// Splits on line boundaries where possible, only breaking mid-line
/// when a single line exceeds `max_len`.
fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        if current.len() + line.len() + 1 > max_len {
            if !current.is_empty() {
                chunks.push(current.clone());
                current.clear();
            }
            // If a single line is too long, split it
            if line.len() > max_len {
                let mut remaining = line;
                while remaining.len() > max_len {
                    chunks.push(remaining[..max_len].to_string());
                    remaining = &remaining[max_len..];
                }
                current.push_str(remaining);
            } else {
                current.push_str(line);
            }
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}
