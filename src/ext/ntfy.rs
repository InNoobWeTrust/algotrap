use core::error::Error;
use reqwest::Client;
use validator::Validate;

#[derive(Debug, Validate)]
pub struct NtfyMessage {
    #[validate(url)]
    url: String,
    topic: Option<String>,
    message: Option<String>,
    title: Option<String>,
    message_template: Option<String>,
    #[validate(range(min = 1, max = 5))]
    priority: Option<u8>,
    tags: Option<Vec<String>>,
    actions: Option<Vec<Vec<String>>>,
}

impl Default for NtfyMessage {
    fn default() -> Self {
        Self {
            url: "https://ntfy.sh".to_string(),
            topic: None,
            message: None,
            title: None,
            message_template: None,
            priority: None,
            tags: None,
            actions: None,
        }
    }
}

impl NtfyMessage {
    pub fn new(url: &str) -> Self {
        Self::default().url(url)
    }

    pub fn url(mut self, url: &str) -> Self {
        self.url = url.to_string();
        self
    }

    pub fn topic(mut self, topic: &str) -> Self {
        self.topic = Some(topic.to_string());
        self
    }

    pub fn title(mut self, title: &str) -> Self {
        self.title = Some(title.to_string());
        self
    }

    pub fn message_template(mut self, message_template: &str) -> Self {
        self.message_template = Some(message_template.to_string());
        self
    }

    pub fn message(mut self, message: &str) -> Self {
        self.message = Some(message.to_string());
        self
    }

    pub fn priority(mut self, priority: u8) -> Self {
        self.priority = Some(priority);
        self
    }

    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    pub fn actions(mut self, actions: Vec<Vec<String>>) -> Self {
        self.actions = Some(actions);
        self
    }

    pub async fn send(self) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.validate()?;
        if self.topic.is_none() {
            return Err("Missing topic".into());
        }
        if self.title.is_none() {
            return Err("Missing title".into());
        }
        if self.message.is_none() {
            return Err("Missing message".into());
        }
        let client = Client::new();
        let uri = match self.message_template {
            Some(ref tmpl) => {
                let encoded_tmpl: String =
                    url::form_urlencoded::byte_serialize(tmpl.as_bytes()).collect();
                format!(
                    "{}/{}?tpl=1&m={}",
                    self.url,
                    self.topic.expect("Missing topic"),
                    encoded_tmpl
                )
            }
            None => format!("{}/{}", self.url, self.topic.expect("Missing topic")),
        };

        let mut request_builder = client
            .post(uri)
            .body(self.message.expect("Missing message").to_string());

        if let Some(t) = self.title {
            request_builder = request_builder.header("X-Title", t);
        }

        if let Some(p) = self.priority {
            request_builder = request_builder.header("X-Priority", p.to_string());
        }

        if let Some(t) = self.tags {
            request_builder = request_builder.header("X-Tags", t.join(","));
        }

        if let Some(actions) = self.actions {
            let action: String = actions
                .iter()
                .map(|act| act.join(", "))
                .collect::<Vec<String>>()
                .join("; ");
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
}
