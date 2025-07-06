use reqwest::Client;

pub async fn send_ntfy_notification(
    topic: &str,
    message: &str,
    title: Option<&str>,
    priority: Option<&str>,
    tags: Option<Vec<&str>>,
    action_url: Option<&str>,
    action_text: Option<&str>,
) -> Result<(), reqwest::Error> {
    let client = Client::new();
    let mut request_builder = client
        .post(format!("https://ntfy.sh/{topic}"))
        .body(message.to_string());

    if let Some(t) = title {
        request_builder = request_builder.header("Title", t);
    }

    if let Some(p) = priority {
        request_builder = request_builder.header("Priority", p);
    }

    if let Some(t) = tags {
        request_builder = request_builder.header("Tags", t.join(","));
    }

    if let (Some(url), Some(text)) = (action_url, action_text) {
        let action = format!("view, {text}, {url}");
        request_builder = request_builder.header("Actions", action);
    }

    let res = request_builder.send().await?;

    if res.status().is_success() {
        println!("Notification sent successfully!");
    } else {
        eprintln!("Failed to send notification: {:#?}", res.status());
        eprintln!("Response: {:#?}", res.text().await?);
    }

    Ok(())
}
