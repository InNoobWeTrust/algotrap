use core::time::Duration;

/// Capture a chart screenshot via the Browserless API.
///
/// Sends the rendered HTML to the Browserless `/screenshot` endpoint
/// and returns the raw PNG bytes.
pub async fn capture_chart_screenshot(
    html: &str,
    browserless_url: &str,
) -> Result<Vec<u8>, Box<dyn core::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let screenshot_url = format!("{}/screenshot", browserless_url.trim_end_matches('/'));

    let payload = browserless_screenshot_payload(html);

    let response = client
        .post(&screenshot_url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .timeout(Duration::from_secs(30))
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Browserless screenshot failed: {status} - {body}").into());
    }

    Ok(response.bytes().await?.to_vec())
}

pub(crate) fn browserless_screenshot_payload(html: &str) -> serde_json::Value {
    serde_json::json!({
        "html": html,
        "options": {
            "fullPage": false,
            "type": "png"
        },
        "viewport": {
            "width": 1080,
            "height": 1080
        },
        "waitForFunction": browserless_ready_wait()
    })
}

/// Wait predicate for the Browserless `/screenshot` readiness contract.
///
/// Browserless validates `waitForFunction` as an **object** (`{ fn, timeout }`),
/// not a bare string. The closure resolves once the shared chartlib bridge flips
/// `document.body[data-chart-state]` from `pending` to `ready` (or throws on
/// `failed`), so Browserless returns only after the chart is actually drawn.
pub(crate) fn browserless_ready_wait() -> serde_json::Value {
    serde_json::json!({
        "fn": browserless_ready_wait_function(),
        "timeout": 25_000
    })
}

pub(crate) fn browserless_ready_wait_function() -> &'static str {
    r#"() => {
        const chart = document.querySelector('[data-chart-state]');
        if (!chart) return false;
        if (chart.dataset.chartState === 'failed') {
            throw new Error('Chart rendering failed');
        }
        return chart.dataset.chartState === 'ready';
    }"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn payload_renders_valid_object_wait_function() {
        // Regression guard: Browserless v2.33 rejects string-form
        // waitForFunction (400 "must be of type object").
        let payload = browserless_screenshot_payload("<html></html>");
        let wait = payload["waitForFunction"].clone();

        assert!(
            wait.is_object(),
            "waitForFunction must be an object, got {wait}"
        );
        assert_eq!(wait["fn"].as_str(), Some(browserless_ready_wait_function()));
        assert!(wait["timeout"].as_u64().unwrap_or(0) >= 1000);

        // The wait predicate observes the readiness contract, not a fixed sleep.
        let fn_src = wait["fn"].as_str().expect("fn must be a string");
        assert!(fn_src.contains("data-chart-state"));
        assert!(fn_src.contains("'ready'"));
        assert!(fn_src.contains("failed"));
    }

    #[test]
    fn payload_carries_fixed_document_shape() {
        let payload = browserless_screenshot_payload("<html></html>");
        assert_eq!(payload["options"]["fullPage"], json!(false));
        assert_eq!(payload["options"]["type"], json!("png"));
        assert_eq!(payload["viewport"], json!({"width": 1080, "height": 1080}));
    }
}
