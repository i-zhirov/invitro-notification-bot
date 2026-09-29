use anyhow::{Context, Result};
use serde::Serialize;

/// Minimal Telegram Bot API client (sendMessage only).
#[derive(Clone)]
pub struct Telegram {
    http: reqwest::Client,
    token: String,
    chat_id: String,
    dry_run: bool,
}

impl Telegram {
    pub fn new(token: &str, chat_id: &str, dry_run: bool) -> Result<Self> {
        let http = reqwest::Client::builder()
            .build()
            .context("failed to build HTTP client")?;
        Ok(Telegram {
            http,
            token: token.to_string(),
            chat_id: chat_id.to_string(),
            dry_run,
        })
    }

    pub async fn send(&self, text: &str) -> Result<()> {
        if self.dry_run {
            println!("=== DRY RUN: would send to {} ===\n{text}\n", self.chat_id);
            return Ok(());
        }

        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.token);
        let body = SendMessage {
            chat_id: self.chat_id.clone(),
            text: text.to_string(),
            parse_mode: "HTML",
            disable_web_page_preview: true,
        };

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .with_context(|| "Telegram request failed")?;

        let status = resp.status();
        let text_resp = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "Telegram -> HTTP {status}: {}",
                &text_resp.chars().take(300).collect::<String>()
            );
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct SendMessage {
    chat_id: String,
    text: String,
    parse_mode: &'static str,
    disable_web_page_preview: bool,
}
