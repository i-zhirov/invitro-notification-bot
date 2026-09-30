mod config;
mod invitro;
mod notifier;
mod state;
mod telegram;

use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{FixedOffset, Utc};
use invitro::{AvailableSlot, InvitroClient, WatchTarget};
use state::SeenSlots;
use tokio::time::sleep;
use tracing::{error, info, warn};

const YEKT: i32 = 5 * 3600;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();
    let check = args.iter().any(|a| a == "--check");
    let once = args.iter().any(|a| a == "--once");

    let client = InvitroClient::new()?;

    if let Some(pos) = args.iter().position(|a| a == "--find") {
        let query = args[pos + 1..].join(" ");
        return find_mode(&client, &query).await;
    }

    if check {
        return check_mode(&client).await;
    }

    let cfg = config::Config::from_env()?;
    let target = resolve_with_retry(&client, &cfg).await?;
    info!(
        "watching {} (bitrix {}), city {}, specialty {}, {} service(s)",
        target.doctor_name,
        target.doctor_bitrix_id,
        target.city.name,
        target.specialty_name,
        target.entries.len()
    );

    let telegram =
        telegram::Telegram::new(&cfg.telegram_bot_token, &cfg.telegram_chat_ids, cfg.dry_run)?;
    let mut seen = SeenSlots::load(&cfg.state_file)?;
    if !seen.existed_before() {
        info!("no state file yet, first run");
    }

    loop {
        if let Err(e) = run_cycle(&client, &cfg, &target, &telegram, &mut seen).await {
            error!("poll cycle failed: {e:#}");
        }
        if once {
            break;
        }
        sleep(Duration::from_secs(cfg.poll_interval_secs)).await;
    }
    Ok(())
}

/// Resolve the watch target at startup, retrying on failure (the site may be
/// temporarily unavailable or DDoS-Guard may be active).
async fn resolve_with_retry(client: &InvitroClient, cfg: &config::Config) -> Result<WatchTarget> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        let result = match &cfg.doctor_name {
            Some(name) => resolve_by_name(client, cfg, name).await,
            None => {
                client
                    .resolve_target(
                        cfg.doctor_bitrix_id,
                        &cfg.city_slug,
                        cfg.specialty_slug.as_deref(),
                        &cfg.service_ids,
                    )
                    .await
            }
        };
        match result {
            Ok(t) => return Ok(t),
            Err(e) if attempt < 5 => {
                warn!("resolve attempt {attempt} failed: {e:#}; retrying in 30s");
                sleep(Duration::from_secs(30)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Resolve the doctor by name (INVITRO_DOCTOR_NAME) instead of a bitrix id.
async fn resolve_by_name(
    client: &InvitroClient,
    cfg: &config::Config,
    name: &str,
) -> Result<WatchTarget> {
    let city = client.city_by_slug(&cfg.city_slug).await?;
    let doctors = client.search_doctors(&city, name).await?;
    let doctor = InvitroClient::pick_doctor(&doctors, name)?;
    info!(
        "resolved doctor by name: {} (bitrix {})",
        doctor.full_name, doctor.bitrix_id
    );
    client
        .resolve_target(
            doctor.bitrix_id,
            &cfg.city_slug,
            cfg.specialty_slug.as_deref(),
            &cfg.service_ids,
        )
        .await
}

/// One poll cycle: fetch all available slots, notify about new ones, persist state.
async fn run_cycle(
    client: &InvitroClient,
    cfg: &config::Config,
    target: &WatchTarget,
    telegram: &telegram::Telegram,
    seen: &mut SeenSlots,
) -> Result<()> {
    let today = today_yekt();
    let slots = client.fetch_available_slots(target, &today).await?;
    info!("cycle: {} slot(s) available", slots.len());

    // First run with no history: either notify about everything that is
    // currently open (default) or silently record it as baseline.
    if !seen.existed_before() && !cfg.notify_on_first_run {
        for s in &slots {
            seen.insert(state::SlotKey::from_slot(s));
        }
        seen.save(&cfg.state_file)?;
        info!("baseline recorded: {} slot(s)", slots.len());
        return Ok(());
    }

    let new: Vec<AvailableSlot> = slots
        .iter()
        .filter(|s| !seen.contains(&state::SlotKey::from_slot(s)))
        .cloned()
        .collect();

    if new.is_empty() {
        return Ok(());
    }

    info!("found {} new slot(s), notifying", new.len());
    notifier::notify_new_slots(telegram, target, &new, |group| {
        for s in group {
            seen.insert(state::SlotKey::from_slot(s));
        }
        seen.save(&cfg.state_file)
    })
    .await
}

/// `--find <name>`: search doctors in the city by name and print their booking
/// pages (no Telegram credentials required). Exits with code 1 if nothing found.
async fn find_mode(client: &InvitroClient, query: &str) -> Result<()> {
    if query.is_empty() {
        anyhow::bail!("usage: invitro-bot --find <doctor name>");
    }
    let city_slug = std::env::var("INVITRO_CITY_SLUG").unwrap_or_else(|_| "kurgan".into());
    let city = client.city_by_slug(&city_slug).await?;
    let doctors = client.search_doctors(&city, query).await?;

    if doctors.is_empty() {
        eprintln!("No doctors found for '{query}' in {}", city.name);
        std::process::exit(1);
    }

    println!(
        "Found {} doctor(s) for '{query}' in {}:",
        doctors.len(),
        city.name
    );
    for d in &doctors {
        println!(
            "\n{} (bitrix_id={}, uuid={}, стаж {} лет)\n  booking page: {}",
            d.full_name,
            d.bitrix_id,
            d.uuid,
            d.experience,
            InvitroClient::booking_page(&city.slug, d)
        );
    }
    Ok(())
}

/// `--check`: resolve and print the watch target (doctor, city, services) without
/// polling or requiring Telegram credentials.
async fn check_mode(client: &InvitroClient) -> Result<()> {
    let bitrix: u64 = std::env::var("INVITRO_DOCTOR_BITRIX_ID")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(19143);
    let city_slug = std::env::var("INVITRO_CITY_SLUG").unwrap_or_else(|_| "kurgan".into());
    let doctor_name = std::env::var("INVITRO_DOCTOR_NAME")
        .ok()
        .filter(|s| !s.is_empty());
    let specialty = std::env::var("INVITRO_SPECIALTY_SLUG")
        .ok()
        .filter(|s| !s.is_empty());
    let services: Vec<String> = std::env::var("INVITRO_SERVICE_IDS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let bitrix_id = if let Some(name) = &doctor_name {
        let city = client.city_by_slug(&city_slug).await?;
        let doctors = client.search_doctors(&city, name).await?;
        let doctor = InvitroClient::pick_doctor(&doctors, name)?;
        println!(
            "Doctor:     {} (bitrix_id={}, resolved by name)",
            doctor.full_name, doctor.bitrix_id
        );
        doctor.bitrix_id
    } else {
        bitrix
    };

    let target = client
        .resolve_target(bitrix_id, &city_slug, specialty.as_deref(), &services)
        .await
        .context("failed to resolve doctor; check INVITRO_DOCTOR_BITRIX_ID / INVITRO_DOCTOR_NAME / INVITRO_CITY_SLUG / INVITRO_SPECIALTY_SLUG")?;

    if doctor_name.is_none() {
        println!(
            "Doctor:     {} (bitrix_id={})",
            target.doctor_name, target.doctor_bitrix_id
        );
    }
    println!("City:       {} ({})", target.city.name, target.city.slug);
    println!("Specialty:  {}", target.specialty_name);
    println!("UUID:       {}", target.doctor_uuid);
    println!("Watching {} office/service pair(s):", target.entries.len());
    for e in &target.entries {
        println!("  - {} | {} | {} ₽", e.office_name, e.service_name, e.price);
    }

    let today = today_yekt();
    let slots = client.fetch_available_slots(&target, &today).await?;
    println!("\nAvailable slots today (window from {today}):");
    if slots.is_empty() {
        println!("  (none)");
    }
    for s in &slots {
        println!(
            "  - {} {} | {} | {}",
            s.date, s.time, s.office_name, s.service_name
        );
    }
    Ok(())
}

fn today_yekt() -> String {
    let tz = FixedOffset::east_opt(YEKT).expect("valid offset");
    Utc::now().with_timezone(&tz).format("%Y-%m-%d").to_string()
}
