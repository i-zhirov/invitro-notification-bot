use std::collections::BTreeMap;

use anyhow::Result;

use crate::invitro::{AvailableSlot, WatchTarget};
use crate::telegram::Telegram;

/// Telegram message length limit (4096 chars); stay safely below it.
const MAX_MESSAGE_CHARS: usize = 3500;

/// Send one Telegram message per (office, service) group of new slots.
///
/// `mark_sent` is invoked after each group is fully delivered, so the caller
/// can persist progress and only retry the unsent groups later.
pub async fn notify_new_slots<F>(
    telegram: &Telegram,
    target: &WatchTarget,
    slots: &[AvailableSlot],
    mut mark_sent: F,
) -> Result<()>
where
    F: FnMut(&[&AvailableSlot]) -> Result<()>,
{
    let mut groups: BTreeMap<(String, String, String, String, String), Vec<&AvailableSlot>> =
        BTreeMap::new();
    for s in slots {
        groups
            .entry((
                s.office_id.clone(),
                s.office_name.clone(),
                s.service_id.clone(),
                s.service_name.clone(),
                s.price.clone(),
            ))
            .or_default()
            .push(s);
    }

    for ((_, office_name, _, service_name, price), group) in &groups {
        for msg in format_messages(target, office_name, service_name, price, group) {
            telegram.send(&msg).await?;
        }
        mark_sent(group)?;
    }
    Ok(())
}

/// Build the messages for one (office, service) group, chunking date lines so
/// each message stays under Telegram's length limit.
fn format_messages(
    target: &WatchTarget,
    office_name: &str,
    service_name: &str,
    price: &str,
    group: &[&AvailableSlot],
) -> Vec<String> {
    let mut by_date: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for s in group {
        by_date.entry(&s.date).or_default().push(&s.time);
    }

    let specialty_slug = target
        .specialty_slug
        .as_deref()
        .unwrap_or("ginekolog")
        .to_lowercase();
    let link = format!(
        "https://www.invitro.ru/{}/vrachi/{}/{}/",
        target.city.slug, specialty_slug, target.doctor_bitrix_id
    );

    let header = format!(
        "🩺 <b>Новые слоты у врача: {}</b>\n🏥 {}\n💳 {} — {} ₽\n",
        target.doctor_name, office_name, service_name, price
    );

    let mut messages = Vec::new();
    let mut current = String::with_capacity(MAX_MESSAGE_CHARS + 128);
    current.push_str(&header);

    for (date, times) in &by_date {
        let mut sorted = times.clone();
        sorted.sort();
        let line = format!("📅 {}: {}\n", date, sorted.join(", "));

        // Start a new message if this line does not fit; always keep the header
        // on subsequent chunks with a "(продолжение)" marker.
        if current.len() + line.len() > MAX_MESSAGE_CHARS {
            current.push_str(&format!("🔗 {link}"));
            messages.push(current);
            current = String::with_capacity(MAX_MESSAGE_CHARS + 128);
            current.push_str(&header);
            current.push_str("(продолжение)\n");
        }
        current.push_str(&line);
    }

    current.push_str(&format!("🔗 {link}"));
    messages.push(current);
    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invitro::{City, WatchEntry};

    fn target() -> WatchTarget {
        WatchTarget {
            doctor_uuid: "uuid".into(),
            doctor_bitrix_id: 19143,
            doctor_name: "Хохлова Ольга Евгеньевна".into(),
            city: City {
                id: "city".into(),
                name: "Курган".into(),
                slug: "kurgan".into(),
            },
            specialty_name: "Гинеколог".into(),
            specialty_slug: Some("ginekolog".into()),
            entries: vec![WatchEntry {
                office_id: "office".into(),
                office_name: "МО Курган-4:Гоголя 133".into(),
                service_id: "svc".into(),
                service_name: "Прием врача в клинике".into(),
                price: "3500.00".into(),
            }],
        }
    }

    fn slot(date: &str, time: &str) -> AvailableSlot {
        AvailableSlot {
            office_id: "office".into(),
            office_name: "МО Курган-4:Гоголя 133".into(),
            service_id: "svc".into(),
            service_name: "Прием врача в клинике".into(),
            price: "3500.00".into(),
            date: date.into(),
            time: time.into(),
        }
    }

    #[test]
    fn message_contains_link_and_slots() {
        let msgs = format_messages(
            &target(),
            "МО Курган-4:Гоголя 133",
            "Прием врача в клинике",
            "3500.00",
            &[
                &slot("2026-10-05", "13:00"),
                &slot("2026-10-05", "10:00"),
                &slot("2026-10-06", "09:30"),
            ],
        );
        assert_eq!(msgs.len(), 1);
        let msg = &msgs[0];
        assert!(msg.contains("https://www.invitro.ru/kurgan/vrachi/ginekolog/19143/"));
        assert!(msg.contains("📅 2026-10-05: 10:00, 13:00"));
        assert!(msg.contains("📅 2026-10-06: 09:30"));
        assert!(msg.contains("3500.00"));
        assert!(msg.contains("\n🔗 https://www.invitro.ru/kurgan/vrachi/ginekolog/19143/"));
        assert!(msg.ends_with("19143/"));
        assert!(!msg.contains("vrachi/vrachi"));
    }

    #[test]
    fn long_group_is_chunked() {
        let slots: Vec<AvailableSlot> = (0..300)
            .flat_map(|d| {
                (0..10).map(move |t| {
                    slot(
                        &format!("2026-11-{:02}", (d % 28) + 1),
                        &format!("{:02}:00", t + 9),
                    )
                })
            })
            .collect();
        let refs: Vec<&AvailableSlot> = slots.iter().collect();
        let msgs = format_messages(
            &target(),
            "МО Курган-4:Гоголя 133",
            "Прием врача в клинике",
            "3500.00",
            &refs,
        );
        assert!(
            msgs.len() > 1,
            "expected multiple messages, got {}",
            msgs.len()
        );
        for m in &msgs {
            assert!(
                m.len() <= MAX_MESSAGE_CHARS + 128,
                "message too long: {}",
                m.len()
            );
            assert!(m.contains("https://www.invitro.ru/kurgan/vrachi/ginekolog/19143/"));
        }
    }
}
