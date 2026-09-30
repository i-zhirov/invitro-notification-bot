use std::path::PathBuf;

use anyhow::{Context, Result};

/// Runtime configuration, read from environment variables.
///
/// All variables use the `INVITRO_` prefix:
/// - `INVITRO_TELEGRAM_BOT_TOKEN` (required) — Telegram bot token from @BotFather
/// - `INVITRO_TELEGRAM_CHAT_ID` (required) — comma-separated chat ids to send
///   notifications to (e.g. `123,-100456`)
/// - `INVITRO_DRY_RUN` — if `1`, print notifications to stdout instead of sending (default: 0)
/// - `INVITRO_DOCTOR_BITRIX_ID` — numeric doctor id from the page URL (default: 19143)
/// - `INVITRO_CITY_SLUG` — city slug from the page URL (default: kurgan)
/// - `INVITRO_SPECIALTY_SLUG` — specialty to watch (default: primary one)
/// - `INVITRO_SERVICE_IDS` — comma-separated service UUIDs to watch; default: the
///   doctor's main service (the one used by the site's booking flow)
/// - `INVITRO_POLL_INTERVAL_SECS` — seconds between polls (default: 120)
/// - `INVITRO_STATE_FILE` — path of the JSON state file (default: ./state.json)
/// - `INVITRO_NOTIFY_ON_FIRST_RUN` — if `1`, notify about slots available on the very
///   first poll (default: 1)
#[derive(Debug, Clone)]
pub struct Config {
    pub telegram_bot_token: String,
    pub telegram_chat_ids: Vec<String>,
    pub dry_run: bool,
    pub doctor_bitrix_id: u64,
    pub city_slug: String,
    pub specialty_slug: Option<String>,
    pub service_ids: Vec<String>,
    pub poll_interval_secs: u64,
    pub state_file: PathBuf,
    pub notify_on_first_run: bool,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let telegram_bot_token = env_req("INVITRO_TELEGRAM_BOT_TOKEN")?;
        let telegram_chat_ids = parse_chat_ids(&env_req("INVITRO_TELEGRAM_CHAT_ID")?)?;
        if telegram_chat_ids.is_empty() {
            anyhow::bail!("INVITRO_TELEGRAM_CHAT_ID must contain at least one chat id");
        }

        let doctor_bitrix_id = env_or("INVITRO_DOCTOR_BITRIX_ID", "19143")
            .parse()
            .context("INVITRO_DOCTOR_BITRIX_ID must be a number")?;
        let poll_interval_secs: u64 = env_or("INVITRO_POLL_INTERVAL_SECS", "120")
            .parse()
            .context("INVITRO_POLL_INTERVAL_SECS must be a number")?;

        let service_ids = env_or("INVITRO_SERVICE_IDS", "")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(Config {
            telegram_bot_token,
            telegram_chat_ids,
            dry_run: env_or("INVITRO_DRY_RUN", "0") == "1",
            doctor_bitrix_id,
            city_slug: env_or("INVITRO_CITY_SLUG", "kurgan"),
            specialty_slug: env_opt("INVITRO_SPECIALTY_SLUG"),
            service_ids,
            poll_interval_secs,
            state_file: PathBuf::from(env_or("INVITRO_STATE_FILE", "state.json")),
            notify_on_first_run: env_or("INVITRO_NOTIFY_ON_FIRST_RUN", "1") == "1",
        })
    }
}

fn env_req(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("{key} is not set"))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Parse a comma-separated list of Telegram chat ids (e.g. `"123,-100456"`).
///
/// Entries are trimmed; empty entries (e.g. from double commas) are skipped.
/// An entry that is not an integer is rejected with an error.
fn parse_chat_ids(raw: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.parse::<i64>().is_err() {
            anyhow::bail!("invalid Telegram chat id: '{part}'");
        }
        out.push(part.to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_ids() {
        assert_eq!(
            parse_chat_ids("733839622,450097973").unwrap(),
            vec!["733839622", "450097973"]
        );
    }

    #[test]
    fn trims_whitespace_and_skips_empty_entries() {
        assert_eq!(
            parse_chat_ids(" 733839622 , , 450097973 ").unwrap(),
            vec!["733839622", "450097973"]
        );
        assert_eq!(
            parse_chat_ids("733839622,,450097973").unwrap(),
            vec!["733839622", "450097973"]
        );
    }

    #[test]
    fn parses_single_and_group_ids() {
        assert_eq!(parse_chat_ids("733839622").unwrap(), vec!["733839622"]);
        assert_eq!(
            parse_chat_ids("-1001234567890").unwrap(),
            vec!["-1001234567890"]
        );
    }

    #[test]
    fn rejects_invalid_entries() {
        assert!(parse_chat_ids("123,abc").is_err());
        assert!(parse_chat_ids("12a").is_err());
        assert!(parse_chat_ids("123, 45.6").is_err());
        assert!(parse_chat_ids("123,456,").is_ok()); // trailing comma is just an empty entry
    }

    #[test]
    fn empty_input_yields_empty_list() {
        assert!(parse_chat_ids("").unwrap().is_empty());
        assert!(parse_chat_ids(" , , ").unwrap().is_empty());
    }
}
