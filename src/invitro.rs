use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use serde::Deserialize;

const BASE_URL: &str = "https://www.invitro.ru/golk";
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Minimal client for the public Invitro.ru JSON API (`/golk/*`).
///
/// The endpoints are public (no auth). A browser-like User-Agent is sent because
/// the site sits behind DDoS-Guard, which may challenge requests without one.
#[derive(Clone)]
pub struct InvitroClient {
    http: reqwest::Client,
}

impl InvitroClient {
    pub fn new() -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { http })
    }

    async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T> {
        let url = format!("{BASE_URL}{path}");
        let resp = self
            .http
            .get(&url)
            .query(query)
            .header("Accept", "application/json")
            .send()
            .await
            .with_context(|| format!("request failed: {url}"))?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("{url} -> HTTP {status}: {}", truncate(&body, 300));
        }
        serde_json::from_str(&body)
            .with_context(|| format!("bad JSON from {url}: {}", truncate(&body, 200)))
    }

    /// Map a numeric (bitrix) doctor id from the page URL to an internal UUID.
    pub async fn doctor_uuid(&self, bitrix_id: u64) -> Result<String> {
        let m: MapperResponse = self
            .get(
                &format!("/doctors/api/v1/mapper/doctors/bitrix/{bitrix_id}"),
                &[],
            )
            .await?;
        Ok(m.uuid)
    }

    /// Find a city by slug (e.g. `kurgan`).
    pub async fn city_by_slug(&self, slug: &str) -> Result<City> {
        let r: CitiesResponse = self.get("/addresses/api/v1/cities", &[("q", slug)]).await?;
        r.cities
            .iter()
            .find(|c| c.slug == slug)
            .or_else(|| r.cities.first())
            .cloned()
            .with_context(|| format!("city with slug '{slug}' not found"))
    }

    pub async fn doctor(&self, uuid: &str, city_id: &str) -> Result<Doctor> {
        self.get(
            &format!("/appointments/api/v1/doctors/{uuid}/"),
            &[("cityID", city_id)],
        )
        .await
    }

    /// Offices and services of a doctor for one specialty in one city.
    pub async fn doctor_specialty(
        &self,
        doctor_uuid: &str,
        specialty_id: &str,
        city_id: &str,
    ) -> Result<DoctorSpecialty> {
        self.get(
            &format!("/appointments/api/v1/doctors/{doctor_uuid}/specialties/{specialty_id}/"),
            &[("cityID", city_id)],
        )
        .await
    }

    /// Office details, used to verify which city an office belongs to.
    pub async fn office(&self, office_id: &str) -> Result<OfficeDetails> {
        self.get(
            &format!("/addresses/api/v1/medical-offices/{office_id}"),
            &[],
        )
        .await
    }

    /// Availability per date (today .. ~30 days) for an office/doctor/service.
    /// `slot_date` is the start of the window (usually today in the target city).
    pub async fn intervals(
        &self,
        office_id: &str,
        doctor_uuid: &str,
        service_id: &str,
        slot_date: &str,
    ) -> Result<BTreeMap<String, bool>> {
        let r: IntervalsResponse = self
            .get(
                "/appointments/api/v1/intervals/doctors",
                &[
                    ("officeID", office_id),
                    ("doctorID", doctor_uuid),
                    ("serviceID", service_id),
                    ("slotDate", slot_date),
                ],
            )
            .await?;
        Ok(r.intervals)
    }

    /// Concrete times of one date for an office/doctor/service.
    pub async fn slots(
        &self,
        office_id: &str,
        doctor_uuid: &str,
        service_id: &str,
        slot_date: &str,
    ) -> Result<SlotsResponse> {
        self.get(
            "/appointments/api/v1/slots/doctors",
            &[
                ("officeID", office_id),
                ("doctorID", doctor_uuid),
                ("serviceID", service_id),
                ("slotDate", slot_date),
            ],
        )
        .await
    }
}

// ---------- payload types ----------

#[derive(Debug, Deserialize)]
struct MapperResponse {
    uuid: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct City {
    pub id: String,
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Deserialize)]
struct CitiesResponse {
    cities: Vec<City>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Specialty {
    pub id: String,
    pub slug: String,
    #[serde(default)]
    pub custom_name: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub is_primary: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Doctor {
    pub first_name: String,
    pub middle_name: String,
    pub last_name: String,
    pub specialties: Vec<Specialty>,
}

impl Doctor {
    pub fn full_name(&self) -> String {
        format!(
            "{} {} {}",
            self.last_name, self.first_name, self.middle_name
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DoctorSpecialty {
    #[serde(default)]
    pub offices: Vec<OfficeEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OfficeEntry {
    pub office: OfficeInfo,
    pub main_service_id: String,
    #[serde(default)]
    pub services: Vec<Service>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OfficeInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Service {
    pub service_id: String,
    pub name: String,
    #[serde(default)]
    pub price: String,
    #[serde(default)]
    pub is_consultation: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OfficeDetails {
    pub city_id: String,
    #[serde(default)]
    pub city_name: String,
}

#[derive(Debug, Deserialize)]
struct IntervalsResponse {
    intervals: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SlotsResponse {
    #[serde(default)]
    pub slots: Vec<String>,
}

// ---------- high-level watch model ----------

/// A service of the doctor at a concrete office that we poll.
#[derive(Debug, Clone)]
pub struct WatchEntry {
    pub office_id: String,
    pub office_name: String,
    pub service_id: String,
    pub service_name: String,
    pub price: String,
}

/// Everything needed to poll the doctor, resolved once per cycle.
#[derive(Debug, Clone)]
pub struct WatchTarget {
    pub doctor_uuid: String,
    pub doctor_bitrix_id: u64,
    pub doctor_name: String,
    pub city: City,
    pub specialty_name: String,
    pub specialty_slug: Option<String>,
    pub entries: Vec<WatchEntry>,
}

/// One concrete available date+time at an office for a service.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct AvailableSlot {
    pub office_id: String,
    pub office_name: String,
    pub service_id: String,
    pub service_name: String,
    pub price: String,
    pub date: String,
    pub time: String,
}

impl InvitroClient {
    /// Resolve doctor + city + specialty + offices/services into poll targets.
    ///
    /// Only offices belonging to `city_slug` are kept. `service_ids` (if non-empty)
    /// selects exact services; otherwise the doctor's main service at each office
    /// is watched (the one used by the site's booking flow).
    pub async fn resolve_target(
        &self,
        bitrix_id: u64,
        city_slug: &str,
        specialty_slug: Option<&str>,
        service_ids: &[String],
    ) -> Result<WatchTarget> {
        let city = self.city_by_slug(city_slug).await?;
        let uuid = self.doctor_uuid(bitrix_id).await?;
        let doctor = self.doctor(&uuid, &city.id).await?;

        let specialty = match specialty_slug {
            Some(slug) => doctor
                .specialties
                .iter()
                .find(|s| s.slug == slug)
                .with_context(|| format!("doctor {bitrix_id} has no specialty '{slug}'"))?
                .clone(),
            None => doctor
                .specialties
                .iter()
                .find(|s| s.is_primary)
                .or_else(|| doctor.specialties.first())
                .context("doctor has no specialties")?
                .clone(),
        };

        let ds = self
            .doctor_specialty(&uuid, &specialty.id, &city.id)
            .await?;

        let mut entries = Vec::new();
        for off in &ds.offices {
            // Verify the office really is in the target city (the API may return
            // offices of other cities for the same doctor).
            let details = self.office(&off.office.id).await?;
            if details.city_id != city.id {
                tracing::debug!(
                    "skipping office {} ({}) — not in {}",
                    off.office.name,
                    details.city_name,
                    city.name
                );
                continue;
            }

            // By default watch only the main service — the one the site's
            // "Записаться" flow uses (and what the doctor page shows), so the
            // user gets one message per office instead of near-duplicates for
            // every consultation service.
            let selected: Vec<&Service> = if service_ids.is_empty() {
                let main: Vec<&Service> = off
                    .services
                    .iter()
                    .filter(|s| s.service_id == off.main_service_id)
                    .collect();
                if !main.is_empty() {
                    main
                } else {
                    let consult: Vec<&Service> =
                        off.services.iter().filter(|s| s.is_consultation).collect();
                    if consult.is_empty() {
                        off.services.iter().collect()
                    } else {
                        consult
                    }
                }
            } else {
                off.services
                    .iter()
                    .filter(|s| service_ids.iter().any(|id| id == &s.service_id))
                    .collect()
            };

            for svc in selected {
                entries.push(WatchEntry {
                    office_id: off.office.id.clone(),
                    office_name: off.office.name.clone(),
                    service_id: svc.service_id.clone(),
                    service_name: svc.name.clone(),
                    price: svc.price.clone(),
                });
            }
        }

        if entries.is_empty() {
            anyhow::bail!(
                "doctor {bitrix_id} has no watched services in {city_slug} (office/service filter too narrow?)"
            );
        }

        Ok(WatchTarget {
            doctor_uuid: uuid,
            doctor_bitrix_id: bitrix_id,
            doctor_name: doctor.full_name(),
            city,
            specialty_name: specialty
                .custom_name
                .clone()
                .or(specialty.name)
                .unwrap_or_else(|| specialty.slug.clone()),
            specialty_slug: Some(specialty.slug.clone()),
            entries,
        })
    }

    /// Fetch all currently available slots for all watched entries.
    pub async fn fetch_available_slots(
        &self,
        target: &WatchTarget,
        today: &str,
    ) -> Result<Vec<AvailableSlot>> {
        let mut out = BTreeSet::new();

        for entry in &target.entries {
            let intervals = match self
                .intervals(
                    &entry.office_id,
                    &target.doctor_uuid,
                    &entry.service_id,
                    today,
                )
                .await
            {
                Ok(i) => i,
                Err(e) => {
                    tracing::warn!("intervals failed for {}: {e}", entry.service_name);
                    continue;
                }
            };

            let available_dates: Vec<&String> = intervals
                .iter()
                .filter(|(_, available)| **available)
                .map(|(d, _)| d)
                .collect();

            for date in available_dates {
                let slots = match self
                    .slots(
                        &entry.office_id,
                        &target.doctor_uuid,
                        &entry.service_id,
                        date,
                    )
                    .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("slots failed for {} at {date}: {e}", entry.service_name);
                        continue;
                    }
                };
                for time in slots.slots {
                    out.insert(AvailableSlot {
                        office_id: entry.office_id.clone(),
                        office_name: entry.office_name.clone(),
                        service_id: entry.service_id.clone(),
                        service_name: entry.service_name.clone(),
                        price: entry.price.clone(),
                        date: date.clone(),
                        time,
                    });
                }
            }
        }

        Ok(out.into_iter().collect())
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}
