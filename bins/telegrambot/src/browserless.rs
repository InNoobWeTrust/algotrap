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

    let payload = serde_json::json!({
        "html": html,
        "options": {
            "fullPage": false,
            "type": "png"
        },
        "viewport": {
            "width": 1080,
            "height": 1080
        },
        "waitForTimeout": 2000
    });

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
