use teloxide::prelude::*;
use teloxide::types::{InputFile, InputMedia, InputMediaPhoto};
use tracing::warn;

use crate::config::EnvConf;

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

/// Send a compact entry alert with direction + confidence badge.
pub async fn send_alert(
    bot: &Bot,
    chat_id: ChatId,
    symbol: &str,
    direction: &str,
    confidence: f64,
    summary: &str,
    tf_charts: &[(String, Vec<u8>)],
) -> Result<(), Box<dyn core::error::Error + Send + Sync>> {
    // Send charts if available
    if !tf_charts.is_empty() {
        let media: Vec<InputMedia> = tf_charts
            .iter()
            .enumerate()
            .map(|(i, (tf_label, png_bytes))| {
                let file_name = format!("{symbol}_{tf_label}.png");
                let input_file = InputFile::memory(png_bytes.clone()).file_name(file_name);
                let mut photo = InputMediaPhoto::new(input_file);
                if i == 0 {
                    photo = photo.caption(format!("🎯 {symbol} — Entry Alert"));
                } else {
                    photo = photo.caption(tf_label.as_str());
                }
                InputMedia::Photo(photo)
            })
            .collect();

        for chunk in media.chunks(10) {
            if let Err(e) = bot.send_media_group(chat_id, chunk.to_vec()).await {
                warn!("Failed to send alert media group: {e}");
            }
        }
    }

    // Format alert text
    let direction_emoji = match direction.to_uppercase().as_str() {
        "LONG" => "🟢 LONG",
        "SHORT" => "🔴 SHORT",
        _ => "⚪ NONE",
    };

    let bold_symbol = to_bold_sans(symbol);
    let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC");
    let alert_text = format!(
        "━━━ 🎯 ENTRY ALERT ━━━\n\
         📊 {bold_symbol}\n\
         {direction_emoji} | Confidence: {confidence:.0}%\n\
         🕐 {ts}\n\
         \n\
         {summary}"
    );

    for chunk in split_message(&alert_text, 4000) {
        if let Err(e) = bot.send_message(chat_id, &chunk).await {
            warn!("Failed to send alert message: {e}");
        }
    }

    Ok(())
}

/// Format a message listing all configured tickers for the /list command.
pub fn available_tickers_message(conf: &EnvConf) -> String {
    let mut lines = vec!["📋 Configured tickers:\n".to_string()];

    for tc in &conf.tickers {
        lines.push(format!(
            "• {} — {} TFs, default: {}",
            to_bold_sans(&tc.symbol),
            tc.tfs.len(),
            tc.default_tf,
        ));
    }

    lines.push(format!(
        "\n⚙️ Scan interval: {}s | Confidence threshold: {:.0}%",
        conf.scan_interval_secs, conf.confidence_threshold
    ));

    lines.join("\n")
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Convert ASCII to Unicode Mathematical Bold Sans-Serif for a visually distinct ticker.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EnvConf, TickerConf};

    #[test]
    fn test_split_message_short() {
        let chunks = split_message("Hello world", 100);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Hello world");
    }

    #[test]
    fn test_split_message_splits_at_line_boundary() {
        let text = "Line one\nLine two\nLine three";
        let chunks = split_message(text, 18);
        assert!(chunks.len() >= 2);
        // Each chunk should be <= 18 chars
        for chunk in &chunks {
            assert!(chunk.len() <= 18, "Chunk too long: {}", chunk.len());
        }
    }

    #[test]
    fn test_to_bold_sans() {
        let result = to_bold_sans("BTC-USDT");
        // Should not contain any plain ASCII letters
        assert!(!result.contains('B'));
        assert!(!result.contains('T'));
        // Hyphen should be preserved
        assert!(result.contains('-'));
    }

    #[test]
    fn test_to_bold_sans_preserves_special_chars() {
        let result = to_bold_sans("A-1");
        assert!(result.contains('-'));
        // 'A' should be converted to a bold sans character
        assert_ne!(result.chars().next().unwrap(), 'A');
    }

    fn test_conf() -> EnvConf {
        let tickers = vec![
            TickerConf {
                symbol: "BTC-USDT".into(),
                sl_percent: 0.1,
                tol_percent: 0.618,
                tfs: vec![],
                default_tf: algotrap::prelude::Timeframe::M15,
            },
            TickerConf {
                symbol: "ETH-USDT".into(),
                sl_percent: 0.08,
                tol_percent: 0.5,
                tfs: vec![
                    algotrap::prelude::Timeframe::M15,
                    algotrap::prelude::Timeframe::H1,
                ],
                default_tf: algotrap::prelude::Timeframe::H4,
            },
        ];

        EnvConf {
            tickers,
            telegram_bot_token: "test".into(),
            telegram_chat_id: -100,
            llm_api_base: "http://localhost".into(),
            llm_api_key: "key".into(),
            llm_model: "model".into(),
            browserless_url: "http://localhost".into(),
            prompts_dir: "config/prompts".into(),
            scan_interval_secs: 900,
            confidence_threshold: 70.0,
            timeout_secs: 30,
        }
    }

    #[test]
    fn test_available_tickers_message() {
        let conf = test_conf();
        let msg = available_tickers_message(&conf);

        assert!(msg.contains("Configured tickers"));
        assert!(msg.contains("2 TFs"));
        assert!(msg.contains("900s"));
        assert!(msg.contains("70%"));
    }

    #[test]
    fn test_available_tickers_message_empty() {
        let mut conf = test_conf();
        conf.tickers.clear();
        let msg = available_tickers_message(&conf);
        assert!(msg.contains("Configured tickers"));
        // Should still have the settings line
        assert!(msg.contains("Scan interval"));
    }
}

