use chrono::Utc;
use core::error::Error;
use core::time::Duration;
use fantoccini::{
    Client, ClientBuilder,
    actions::{InputSource, MouseActions, PointerAction},
    elements::Element,
    error::CmdError,
};
use polars::prelude::*;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use tracing::{debug, instrument, warn};

use std::ops::Deref;
use std::process::{Child, Command, Stdio};
use std::{fmt::Debug, io::Cursor, num::NonZeroUsize};

#[derive(Debug)]
pub(crate) struct Subprocess(Child);

impl Deref for Subprocess {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for Subprocess {
    fn drop(&mut self) {
        for _ in 0..3 {
            if self.0.kill().is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_secs(5));
        }
    }
}

impl Subprocess {
    pub fn new(cmd: &str, args: &Vec<String>) -> Result<Self, Box<dyn Error>> {
        let child = Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(Subprocess(child))
    }
}

pub trait WebDriver {
    fn get_port(&self) -> usize;
    fn create_client(
        &self,
        headful: bool,
    ) -> impl Future<Output = Result<Client, Box<dyn Error + Sync + Send>>>;
}

#[derive(Debug)]
pub struct GeckoDriver {
    #[allow(dead_code)]
    proc: Subprocess,
    port: usize,
}

impl Default for GeckoDriver {
    fn default() -> Self {
        let mut rng = SmallRng::seed_from_u64(Utc::now().timestamp_micros() as u64);
        let port = rng.random_range(4445..=7999);
        let proc = Subprocess::new(
            "geckodriver",
            &vec![
                "-p".to_string(),
                port.to_string(),
                "--log".to_string(),
                "fatal".to_string(),
            ],
        )
        .expect("Failed to start geckodriver");
        debug!(%port, "Starting geckodriver...");
        // Sleep for 3 seconds, waiting webdriver
        std::thread::sleep(Duration::from_secs(3));
        debug!(%port, "geckodriver is listening...");
        Self { proc, port }
    }
}

impl GeckoDriver {
    pub fn new(port: usize) -> Result<Self, Box<dyn Error>> {
        let proc = Subprocess::new(
            "geckodriver",
            &vec![
                "-p".to_string(),
                port.to_string(),
                "--log".to_string(),
                "fatal".to_string(),
            ],
        )?;

        debug!(%port, "Starting geckodriver...");
        // Sleep for 3 seconds, waiting webdriver
        std::thread::sleep(Duration::from_secs(3));
        debug!(%port, "geckodriver is listening...");

        Ok(Self { proc, port })
    }
}

impl WebDriver for GeckoDriver {
    fn get_port(&self) -> usize {
        self.port
    }

    #[instrument]
    async fn create_client(&self, headful: bool) -> Result<Client, Box<dyn Error + Sync + Send>> {
        let browser_args = [
            "--enable-automation".into(),
            "False".into(),
            "--disable-blink-features".into(),
            "AutomationControlled".into(),
            if headful {
                "".into()
            } else {
                "--headless".into()
            },
        ]
        .into_iter()
        .filter(|s: &String| !s.is_empty())
        .collect::<Vec<String>>();

        let capabilities = serde_json::json!({
            "browserName": "firefox",
            "setWindowRect": true,
            "moz:firefoxOptions": {
            "prefs": {
            "intl.accept_languages": "en-GB"
        },
            "args": browser_args,
        },
            "timeouts": {
            "pageLoad": 10_000,
            "implicit": 5_000,
            "script": 120_000,
        }
        });

        let port = self.get_port();

        let capabilities = capabilities.as_object().unwrap().to_owned();
        let client = ClientBuilder::native()
            .capabilities(capabilities)
            .connect(&format!("http://localhost:{port}"))
            .await?;
        client.set_window_size(1280, 1024).await?;
        Ok(client)
    }
}

fn rand_delay_duration() -> Duration {
    let mut rng = SmallRng::seed_from_u64(Utc::now().timestamp_micros() as u64);
    let millis = (rng.random::<u64>() % 3 + 2) * 1000 + (rng.random::<u64>() % 1000 + 1); // Max 5s
    Duration::from_millis(millis)
}

#[instrument]
fn delay(duration: Option<Duration>) {
    let duration = duration.unwrap_or_else(rand_delay_duration);
    let duration_str = humantime::format_duration(duration);
    debug!("Sleeping for {duration_str}...");
    std::thread::sleep(duration);
}

pub trait ClientActionExt {
    fn mouse_move_to_element(&self, el: &Element) -> impl Future<Output = Result<(), CmdError>>;
    fn perform_click(&self, el: &Element) -> impl Future<Output = Result<(), CmdError>>;
    fn mouse_scroll(
        &self,
        x_offset: isize,
        y_offset: isize,
    ) -> impl Future<Output = Result<(), CmdError>>;
    fn extract_table(
        &self,
        elem: &Element,
        custom_script: Option<String>,
    ) -> impl Future<Output = Result<DataFrame, Box<dyn Error + Send + Sync>>>;
}

impl ClientActionExt for Client {
    #[instrument]
    async fn mouse_move_to_element(&self, el: &Element) -> Result<(), CmdError> {
        let mouse_move_to_element =
            MouseActions::new("mouse".into()).then(PointerAction::MoveToElement {
                element: el.clone(),
                duration: Some(rand_delay_duration()),
                x: 0.,
                y: 0.,
            });
        if let Err(e) = self.perform_actions(mouse_move_to_element).await {
            warn!(element = %e, "failed to move to element");
        }

        delay(Some(Duration::from_millis(250)));

        Ok(())
    }

    #[instrument]
    async fn perform_click(&self, el: &Element) -> Result<(), CmdError> {
        //el.click().await?;
        self.execute("arguments[0].click()", vec![serde_json::to_value(el)?])
            .await?;

        Ok(())
    }

    #[instrument]
    async fn mouse_scroll(&self, x_offset: isize, y_offset: isize) -> Result<(), CmdError> {
        self.execute(
            "scrollBy(arguments[0], arguments[1]);",
            vec![
                serde_json::to_value(x_offset)?,
                serde_json::to_value(y_offset)?,
            ],
        )
        .await?;
        delay(Some(Duration::from_secs(2)));

        Ok(())
    }

    /// Extract table as json from html
    async fn extract_table(
        &self,
        elem: &Element,
        custom_script: Option<String>,
    ) -> Result<DataFrame, Box<dyn Error + Send + Sync>> {
        const DEFAULT_SCRIPT: &str = r#"
const table = arguments[0];
const rows = table.rows;
const headers = [];
const jsonData = [];

// Extract headers
for (let i = 0; i < rows[0].cells.length; i++) {
    headers.push(rows[0].cells[i].innerText);
}

// Extract data
for (let i = 1; i < rows.length; i++) {
    const rowObject = {};
    const cells = rows[i].cells;
    for (let j = 0; j < cells.length; j++) {
        rowObject[headers[j]] = cells[j].innerText;
    }
    jsonData.push(rowObject);
}

return jsonData
"#;
        let tb_json = self
            .execute(
                &custom_script.unwrap_or(DEFAULT_SCRIPT.to_string()),
                vec![serde_json::to_value(elem)?],
            )
            .await?
            .to_string();
        let file = Cursor::new(tb_json.to_string());
        let df = JsonReader::new(file)
            .with_json_format(JsonFormat::Json)
            .infer_schema_len(None)
            .with_batch_size(NonZeroUsize::new(3).unwrap())
            .finish()
            .unwrap();
        Ok(df)
    }
}
