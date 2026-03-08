use teloxide::prelude::*;
use teloxide::types::{InputFile, InputMedia, InputMediaPhoto};
use tracing::warn;

/// Send analysis results (all TF charts + text) to a Telegram chat.
///
/// Charts are sent as a media group (album), followed by the analysis text.
pub async fn send_analysis(
    bot: &Bot,
    chat_id: ChatId,
    symbol: &str,
    analysis_text: &str,
    tf_charts: &[(String, Vec<u8>)],
) -> Result<(), Box<dyn core::error::Error + Send + Sync>> {
    // Send chart images as a media group (album)
    if !tf_charts.is_empty() {
        let media: Vec<InputMedia> = tf_charts
            .iter()
            .enumerate()
            .map(|(i, (tf_label, png_bytes))| {
                let file_name = format!("{symbol}_{tf_label}.png");
                let input_file = InputFile::memory(png_bytes.clone()).file_name(file_name);
                let mut photo = InputMediaPhoto::new(input_file);
                // Caption on the first photo only (Telegram shows it under the album)
                if i == 0 {
                    photo = photo.caption(format!("📊 {symbol} — {tf_label}"));
                } else {
                    photo = photo.caption(tf_label.as_str());
                }
                InputMedia::Photo(photo)
            })
            .collect();

        // Telegram allows max 10 media per group
        for chunk in media.chunks(10) {
            if let Err(e) = bot.send_media_group(chat_id, chunk.to_vec()).await {
                warn!("Failed to send media group: {e}");
            }
        }
    }

    // Prepend a decorated header with the ticker
    let header = format_ticker_header(symbol);
    let full_message = format!("{header}\n\n{analysis_text}");

    // Send analysis text (split if too long for Telegram's 4096 char limit)
    for chunk in split_message(&full_message, 4000) {
        if let Err(e) = bot.send_message(chat_id, &chunk).await {
            warn!("Failed to send message chunk: {e}");
        }
    }

    Ok(())
}

/// Convert ASCII to Unicode Mathematical Bold Sans-Serif for a visually distinct ticker.
/// e.g. "BTC-USDT" → "𝗕𝗧𝗖-𝗨𝗦𝗗𝗧"
fn to_bold_sans(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' => char::from_u32(0x1D5D4 + (c as u32 - 'A' as u32)).unwrap_or(c),
            'a'..='z' => char::from_u32(0x1D5EE + (c as u32 - 'a' as u32)).unwrap_or(c),
            '0'..='9' => char::from_u32(0x1D7EC + (c as u32 - '0' as u32)).unwrap_or(c),
            _ => c,
        })
        .collect()
}

fn format_ticker_header(symbol: &str) -> String {
    let bold_symbol = to_bold_sans(symbol);
    let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC");
    format!("━━━━━━ 🔔 {bold_symbol} ━━━━━━\n🕐 {ts}")
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
