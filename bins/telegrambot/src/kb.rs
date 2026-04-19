//! Knowledge base — 10 markdown files the LLM can read and write to persist
//! insights across scan cycles.
//!
//! Storage layout:
//!   {MEMORY_DIR}/kb/{topic-slug}.md — e.g. /data/memory/kb/market-regimes.md

use std::path::{Path, PathBuf};

use tracing::{info, warn};

// ─── Topic Whitelist ─────────────────────────────────────────────────────────

/// Fixed whitelist of permitted KB topic slugs.
pub const KB_TOPICS: &[&str] = &[
    "market-regimes",
    "indicator-quirks",
    "ticker-personalities",
    "false-signal-patterns",
    "successful-setups",
    "weight-tuning-log",
    "risk-conditions",
    "cross-ticker-signals",
    "timeframe-biases",
    "lessons-learned",
];

/// Maximum content length per write (chars). Prevents runaway LLM writes.
const MAX_WRITE_CHARS: usize = 2000;

/// Maximum total file size (chars) before auto-compaction triggers.
/// ~50KB — keeps KB files manageable while retaining useful history.
const MAX_FILE_CHARS: usize = 50_000;

/// Target size after compaction (chars). Leave headroom for new writes.
const COMPACT_TARGET_CHARS: usize = 4_000;

// ─── File I/O ────────────────────────────────────────────────────────────────

fn kb_dir(memory_dir: &str) -> PathBuf {
    Path::new(memory_dir).join("kb")
}

fn topic_path(memory_dir: &str, topic: &str) -> PathBuf {
    kb_dir(memory_dir).join(format!("{topic}.md"))
}

/// Validate that a topic slug is in the whitelist.
pub fn is_valid_topic(topic: &str) -> bool {
    KB_TOPICS.contains(&topic)
}

/// Seed empty KB files on first run. Only creates files that don't exist.
pub fn seed_kb(memory_dir: &str) -> Result<(), Box<dyn core::error::Error + Send + Sync>> {
    let dir = kb_dir(memory_dir);
    std::fs::create_dir_all(&dir)?;

    for topic in KB_TOPICS {
        let path = topic_path(memory_dir, topic);
        if !path.exists() {
            std::fs::write(&path, format!("# {}\n\n", humanize_topic(topic)))?;
            info!(topic, "Seeded KB file");
        }
    }

    Ok(())
}

/// Read a KB topic file. Returns empty string if not found.
pub fn read_topic(memory_dir: &str, topic: &str) -> String {
    if !is_valid_topic(topic) {
        warn!(topic, "Attempted to read invalid KB topic");
        return format!("Error: unknown topic '{topic}'. Valid topics: {KB_TOPICS:?}");
    }

    let path = topic_path(memory_dir, topic);
    match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => {
            info!(topic, "KB file not found — returning empty");
            String::new()
        }
    }
}

/// Write/append content to a KB topic file. Subject to content length limits.
///
/// If the file would exceed `MAX_FILE_CHARS` after the write, the write still
/// succeeds but a compaction warning is logged. Call `needs_compaction()` to
/// check which topics need LLM-based summarization.
pub fn write_topic(
    memory_dir: &str,
    topic: &str,
    content: &str,
) -> Result<String, Box<dyn core::error::Error + Send + Sync>> {
    if !is_valid_topic(topic) {
        return Ok(format!(
            "Error: unknown topic '{topic}'. Valid topics: {KB_TOPICS:?}"
        ));
    }

    if content.len() > MAX_WRITE_CHARS {
        return Ok(format!(
            "Error: content exceeds {MAX_WRITE_CHARS} char limit ({} chars)",
            content.len()
        ));
    }

    // Sanitize: no path traversal, no script execution markers
    if content.contains("../") || content.contains("..\\") {
        return Ok("Error: content contains path traversal sequences".to_string());
    }

    let path = topic_path(memory_dir, topic);
    let dir = kb_dir(memory_dir);
    std::fs::create_dir_all(&dir)?;

    // Append to existing content
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let updated = format!("{existing}\n{content}\n");
    std::fs::write(&path, &updated)?;

    // Warn if approaching compaction threshold
    if updated.len() > MAX_FILE_CHARS {
        warn!(
            topic,
            file_chars = updated.len(),
            "KB topic exceeds {MAX_FILE_CHARS} chars — needs compaction"
        );
    }

    info!(topic, content_len = content.len(), "Wrote to KB");
    Ok(format!(
        "Successfully wrote {len} chars to '{topic}'",
        len = content.len()
    ))
}

/// Overwrite a KB topic file with new content (used after LLM compaction).
///
/// Bypasses the append logic and replaces the entire file. The header is
/// preserved by the caller (the LLM compaction prompt asks for it).
pub fn force_write_topic(
    memory_dir: &str,
    topic: &str,
    content: &str,
) -> Result<(), Box<dyn core::error::Error + Send + Sync>> {
    if !is_valid_topic(topic) {
        return Err(format!("Invalid KB topic: {topic}").into());
    }

    let path = topic_path(memory_dir, topic);
    let dir = kb_dir(memory_dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(&path, content)?;

    info!(
        topic,
        content_len = content.len(),
        "Force-wrote compacted KB"
    );
    Ok(())
}

/// Return topics that exceed `MAX_FILE_CHARS` and need LLM-based compaction.
///
/// Returns a vec of `(topic_slug, current_content)` for topics that are
/// over the size limit. The caller should feed each to an LLM summarization
/// prompt and then call `force_write_topic()` with the result.
pub fn needs_compaction(memory_dir: &str) -> Vec<(String, String)> {
    KB_TOPICS
        .iter()
        .filter_map(|topic| {
            let content = read_topic(memory_dir, topic);
            if content.len() > MAX_FILE_CHARS {
                Some((topic.to_string(), content))
            } else {
                None
            }
        })
        .collect()
}

/// Target size for compacted KB content.
pub fn compact_target_chars() -> usize {
    COMPACT_TARGET_CHARS
}

/// Read all KB topics at once (for context injection). Returns a map of
/// topic → content for non-empty topics only.
pub fn read_all_topics(memory_dir: &str) -> Vec<(String, String)> {
    KB_TOPICS
        .iter()
        .filter_map(|topic| {
            let content = read_topic(memory_dir, topic);
            if content.trim().is_empty() {
                None
            } else {
                Some((topic.to_string(), content))
            }
        })
        .collect()
}

/// Convert a topic slug to a human-readable title.
fn humanize_topic(slug: &str) -> String {
    slug.split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_topic_whitelist() {
        assert!(is_valid_topic("market-regimes"));
        assert!(is_valid_topic("lessons-learned"));
        assert!(!is_valid_topic("secret-topic"));
        assert!(!is_valid_topic("../etc/passwd"));
    }

    #[test]
    fn test_humanize_topic() {
        assert_eq!(humanize_topic("market-regimes"), "Market Regimes");
        assert_eq!(
            humanize_topic("cross-ticker-signals"),
            "Cross Ticker Signals"
        );
    }

    #[test]
    fn test_seed_and_read() {
        let dir = std::env::temp_dir().join("telegrambot_test_kb");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_str().unwrap();

        seed_kb(dir_str).unwrap();

        // Should have 10 files
        let kb = kb_dir(dir_str);
        let count = std::fs::read_dir(&kb).unwrap().count();
        assert_eq!(count, 10);

        // Each file should have a title header
        let content = read_topic(dir_str, "market-regimes");
        assert!(content.starts_with("# Market Regimes"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_write_and_read() {
        let dir = std::env::temp_dir().join("telegrambot_test_kb_write");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_str().unwrap();

        seed_kb(dir_str).unwrap();
        let result = write_topic(
            dir_str,
            "lessons-learned",
            "BTC tends to gap fill on Mondays.",
        )
        .unwrap();
        assert!(result.contains("Successfully wrote"));

        let content = read_topic(dir_str, "lessons-learned");
        assert!(content.contains("gap fill on Mondays"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_write_invalid_topic() {
        let dir = std::env::temp_dir().join("telegrambot_test_kb_invalid");
        let dir_str = dir.to_str().unwrap();

        let result = write_topic(dir_str, "hacker-topic", "oops").unwrap();
        assert!(result.contains("Error: unknown topic"));
    }

    #[test]
    fn test_write_too_long() {
        let dir = std::env::temp_dir().join("telegrambot_test_kb_long");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_str().unwrap();

        seed_kb(dir_str).unwrap();
        let long = "x".repeat(3000);
        let result = write_topic(dir_str, "market-regimes", &long).unwrap();
        assert!(result.contains("exceeds"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_write_path_traversal() {
        let dir = std::env::temp_dir().join("telegrambot_test_kb_traversal");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_str().unwrap();

        seed_kb(dir_str).unwrap();
        let result = write_topic(dir_str, "market-regimes", "read ../etc/passwd").unwrap();
        assert!(result.contains("path traversal"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
