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

use std::{fmt::Debug, io::Cursor, num::NonZeroUsize, path::PathBuf};
use std::{
    fs::File,
    process::{Child, Command, Stdio},
};
use std::{ops::Deref, path::Path};

#[cfg(unix)]
unsafe extern "C" {
    // Minimal C bindings used to create a new session (setsid) and to send
    // signals to a process group (kill with a negative pid). This avoids
    // adding an external dependency just for a couple of libc calls.
    fn setsid() -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(unix)]
const SIGTERM: i32 = 15;
#[cfg(unix)]
const SIGKILL: i32 = 9;

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
        // If the child has already exited, try_wait will reap it for us.
        match self.0.try_wait() {
            Ok(Some(status)) => {
                debug!(%status, "subprocess already exited (reaped)");
                return;
            }
            Ok(None) => {
                debug!(
                    pid = self.0.id(),
                    "subprocess still running; proceeding to terminate"
                );
            }
            Err(e) => {
                warn!(error = %e, "error while checking subprocess status; will attempt to kill");
            }
        }

        // Try graceful termination of the whole process group on Unix first.
        // This helps ensure browsers (child processes spawned by the driver)
        // are also terminated.
        #[cfg(unix)]
        {
            let pid = self.0.id() as i32;
            debug!(%pid, "sending SIGTERM to process group");
            // send SIGTERM to the process group (-pid); check return value (best-effort)
            unsafe {
                let r = kill(-pid, SIGTERM);
                if r != 0 {
                    warn!(%pid, ret = r, "failed to send SIGTERM to group (best-effort)");
                }
            }
        }

        // Also attempt to politely terminate the child itself (cross-platform).
        if let Err(e) = self.0.kill() {
            debug!(error = %e, "failed to send kill() to subprocess (might have exited)");
        } else {
            debug!(pid = self.0.id(), "sent kill() to subprocess (best-effort)");
        }

        // Poll for exit for a short while (non-blocking).
        for _ in 0..20 {
            match self.0.try_wait() {
                Ok(Some(status)) => {
                    debug!(%status, "subprocess exited after termination signal");
                    return;
                }
                Ok(None) => {
                    // still running, keep waiting
                }
                Err(e) => {
                    warn!(error = %e, "error while polling subprocess; continuing");
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        // Escalate: force-kill the child process itself (best-effort).
        debug!(pid = self.0.id(), "escalating: sending SIGKILL / kill()");
        #[cfg(unix)]
        {
            let pid = self.0.id() as i32;
            unsafe {
                let r = kill(-pid, SIGKILL);
                if r != 0 {
                    warn!(%pid, ret = r, "failed to send SIGKILL to group (best-effort)");
                }
            }
        }

        if let Err(e) = self.0.kill() {
            warn!(error = %e, "failed to force-kill child with kill()");
        }

        // Final attempt: block until we can reap the child so it doesn't remain a zombie.
        match self.0.wait() {
            Ok(status) => {
                debug!(%status, "subprocess reaped successfully in Drop");
            }
            Err(e) => {
                warn!(error = %e, "final wait to reap subprocess failed");
            }
        }
    }
}

impl Subprocess {
    pub fn new(
        cmd: &str,
        args: &Vec<String>,
        logfile: Option<PathBuf>,
    ) -> Result<Self, std::io::Error> {
        let (pipe_out, pipe_err) = match logfile {
            Some(p) => {
                let out = File::create(p.clone()).unwrap();
                let err = File::create(p).unwrap();
                (Stdio::from(out), Stdio::from(err))
            }
            None => (Stdio::inherit(), Stdio::inherit()),
        };
        let child = Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(pipe_out)
            .stderr(pipe_err)
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
            None,
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
    pub fn default_with_log(logfile: &Path) -> Result<Self, std::io::Error> {
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
            Some(logfile.to_path_buf()),
        )?;

        debug!(%port, "Starting geckodriver...");
        // Sleep for 3 seconds, waiting webdriver
        std::thread::sleep(Duration::from_secs(3));
        debug!(%port, "geckodriver is listening...");

        Ok(Self { proc, port })
    }

    pub fn new(port: usize, logfile: &Path) -> Result<Self, std::io::Error> {
        let proc = Subprocess::new(
            "geckodriver",
            &vec![
                "-p".to_string(),
                port.to_string(),
                "--log".to_string(),
                "fatal".to_string(),
            ],
            Some(logfile.to_path_buf()),
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
