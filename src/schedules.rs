use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, NaiveTime, TimeZone, Utc};
use serde::Deserialize;
use tokio::sync::mpsc::unbounded_channel;
use tracing::{info, warn};

use crate::{
    orchestrator::Orchestrator,
    types::{ScheduleCadence, ScheduleIntervalUnit, ScheduleRecord, UiEvent},
};

const SCHEDULE_POLL_INTERVAL_SECONDS: u64 = 1;
const SCHEDULE_LEASE_SECONDS: i64 = 300;

#[derive(Debug, Deserialize)]
pub struct ScheduleCreateRequest {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    pub run_type: String,
    pub cadence_kind: String,
    pub cadence_value: String,
    #[serde(default)]
    pub recipe_name: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Clone)]
pub struct ScheduleRuntime {
    orchestrator: Orchestrator,
}

impl ScheduleRuntime {
    pub fn new(orchestrator: Orchestrator) -> Self {
        Self { orchestrator }
    }

    pub fn start(self) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(
                SCHEDULE_POLL_INTERVAL_SECONDS,
            ));
            loop {
                ticker.tick().await;
                if let Err(err) = self.tick().await {
                    warn!(error = %err, "schedule tick failed");
                }
            }
        });
    }

    async fn tick(&self) -> Result<()> {
        let due = self.orchestrator.store().list_due_schedules(Utc::now())?;
        for schedule in due {
            if let Err(err) =
                run_schedule_by_id(self.orchestrator.clone(), &schedule.id, false).await
            {
                warn!(schedule_id = %schedule.id, error = %err, "scheduled run failed");
            }
        }
        Ok(())
    }
}

pub fn build_schedule_record(
    request: ScheduleCreateRequest,
    conversation_id: String,
) -> Result<ScheduleRecord> {
    let now = Utc::now();
    let name = request.name.trim();
    if name.is_empty() {
        bail!("schedule name is required");
    }
    let run_type = request.run_type.trim();
    if run_type != "recipe" && run_type != "prompt" {
        bail!("schedule run_type must be 'recipe' or 'prompt'");
    }
    let recipe_name = request
        .recipe_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let prompt = request
        .prompt
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if run_type == "recipe" && recipe_name.is_none() {
        bail!("recipe schedules require recipe_name");
    }
    if run_type == "prompt" && prompt.is_none() {
        bail!("prompt schedules require prompt");
    }
    let cadence = parse_schedule_cadence(&request.cadence_kind, &request.cadence_value)?;
    let next_run_at = next_run_after(&cadence, now)?;
    Ok(ScheduleRecord {
        id: format!(
            "schedule-{}-{:08x}",
            now.format("%Y%m%d%H%M%S"),
            rand::random::<u32>()
        ),
        name: name.to_string(),
        title: request.title.filter(|value| !value.trim().is_empty()),
        run_type: run_type.to_string(),
        recipe_name,
        prompt,
        conversation_id,
        cadence,
        enabled: true,
        next_run_at,
        last_run_at: None,
        last_status: None,
        last_error: None,
        lease_expires_at: None,
        created_at: now,
        updated_at: now,
    })
}

pub async fn run_schedule_by_id(
    orchestrator: Orchestrator,
    schedule_id: &str,
    force: bool,
) -> Result<ScheduleRecord> {
    let now = Utc::now();
    let Some(schedule) =
        orchestrator
            .store()
            .claim_schedule(schedule_id, now, SCHEDULE_LEASE_SECONDS, force)?
    else {
        return Err(anyhow!("schedule '{schedule_id}' is not due or not found"));
    };

    let prompt = schedule_prompt(&orchestrator, &schedule)?;
    let (ui_tx, _ui_rx) = unbounded_channel::<UiEvent>();
    info!(schedule_id = %schedule.id, conversation_id = %schedule.conversation_id, "running schedule");
    let result = orchestrator
        .run_scheduled_prompt(&schedule.conversation_id, prompt, ui_tx)
        .await;

    let next = next_run_after(&schedule.cadence, Utc::now())?;
    let (status, error) = match result {
        Ok(_) => ("done", None),
        Err(err) => ("failed", Some(format!("{err:#}"))),
    };
    let updated = orchestrator
        .store()
        .release_schedule(&schedule.id, status, error.clone(), next)?
        .ok_or_else(|| anyhow!("schedule '{}' disappeared after run", schedule.id))?;
    orchestrator.store().append_audit_event(
        Some(&schedule.conversation_id),
        "schedule_run",
        "scheduled run completed",
        serde_json::json!({
            "schedule_id": schedule.id,
            "status": status,
            "error": error,
        }),
    )?;
    Ok(updated)
}

fn schedule_prompt(orchestrator: &Orchestrator, schedule: &ScheduleRecord) -> Result<String> {
    match schedule.run_type.as_str() {
        "prompt" => schedule
            .prompt
            .clone()
            .filter(|prompt| !prompt.trim().is_empty())
            .ok_or_else(|| anyhow!("schedule '{}' has no prompt", schedule.id)),
        "recipe" => {
            let recipe_name = schedule
                .recipe_name
                .as_deref()
                .ok_or_else(|| anyhow!("schedule '{}' has no recipe_name", schedule.id))?;
            let recipe = orchestrator
                .recipes()
                .find(recipe_name)
                .ok_or_else(|| anyhow!("recipe '{recipe_name}' not found"))?;
            let mut conversation = orchestrator.store().load(&schedule.conversation_id)?;
            conversation.pending_recipe = Some(recipe.name.clone());
            orchestrator.store().save(&conversation)?;
            Ok(recipe
                .initial_prompt
                .clone()
                .unwrap_or_else(|| format!("Run scheduled recipe '{}'.", recipe.name)))
        }
        other => Err(anyhow!("unsupported schedule run_type '{other}'")),
    }
}

pub fn parse_schedule_cadence(kind: &str, value: &str) -> Result<ScheduleCadence> {
    let value = value.trim();
    match kind {
        "every" | "interval" => parse_interval_cadence(value),
        "daily" => Ok(ScheduleCadence::Daily {
            time: parse_wall_clock(value)?.format("%H:%M").to_string(),
        }),
        "weekdays" => Ok(ScheduleCadence::Weekdays {
            time: parse_wall_clock(value)?.format("%H:%M").to_string(),
        }),
        other => bail!("unsupported cadence kind '{other}'"),
    }
}

fn parse_interval_cadence(value: &str) -> Result<ScheduleCadence> {
    let suffix = value
        .chars()
        .last()
        .ok_or_else(|| anyhow!("interval cadence is required"))?;
    let (number, unit) = match suffix {
        'm' | 'M' => (&value[..value.len() - 1], ScheduleIntervalUnit::Minutes),
        'h' | 'H' => (&value[..value.len() - 1], ScheduleIntervalUnit::Hours),
        _ => (value, ScheduleIntervalUnit::Minutes),
    };
    let every = number
        .trim()
        .parse::<u64>()
        .with_context(|| format!("invalid interval cadence '{value}'"))?;
    if every == 0 {
        bail!("interval cadence must be greater than zero");
    }
    Ok(ScheduleCadence::Interval { every, unit })
}

fn parse_wall_clock(value: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M")
        .with_context(|| format!("invalid wall-clock time '{value}', expected HH:MM"))
}

pub fn next_run_after(cadence: &ScheduleCadence, after: DateTime<Utc>) -> Result<DateTime<Utc>> {
    match cadence {
        ScheduleCadence::Interval { every, unit } => {
            let duration = match unit {
                ScheduleIntervalUnit::Minutes => ChronoDuration::minutes(*every as i64),
                ScheduleIntervalUnit::Hours => ChronoDuration::hours(*every as i64),
            };
            Ok(after + duration)
        }
        ScheduleCadence::Daily { time } => next_wall_clock_after(time, after, false),
        ScheduleCadence::Weekdays { time } => next_wall_clock_after(time, after, true),
    }
}

fn next_wall_clock_after(
    time: &str,
    after: DateTime<Utc>,
    weekdays_only: bool,
) -> Result<DateTime<Utc>> {
    let time = parse_wall_clock(time)?;
    let local_after = after.with_timezone(&Local);
    for day_offset in 0..=8 {
        let date = local_after.date_naive() + ChronoDuration::days(day_offset);
        if weekdays_only && date.weekday().number_from_monday() > 5 {
            continue;
        }
        let naive = date.and_time(time);
        let Some(local_candidate) = Local
            .from_local_datetime(&naive)
            .single()
            .or_else(|| Local.from_local_datetime(&naive).earliest())
        else {
            continue;
        };
        if local_candidate > local_after {
            return Ok(local_candidate.with_timezone(&Utc));
        }
    }
    Err(anyhow!("failed to compute next schedule run"))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use crate::types::{ScheduleCadence, ScheduleIntervalUnit};

    use super::{next_run_after, parse_schedule_cadence};

    #[test]
    fn parses_interval_cadence() {
        assert_eq!(
            parse_schedule_cadence("every", "15m").unwrap(),
            ScheduleCadence::Interval {
                every: 15,
                unit: ScheduleIntervalUnit::Minutes
            }
        );
        assert_eq!(
            parse_schedule_cadence("every", "2h").unwrap(),
            ScheduleCadence::Interval {
                every: 2,
                unit: ScheduleIntervalUnit::Hours
            }
        );
    }

    #[test]
    fn interval_next_run_adds_duration() {
        let after = Utc.with_ymd_and_hms(2026, 5, 21, 10, 0, 0).unwrap();
        let next = next_run_after(
            &ScheduleCadence::Interval {
                every: 30,
                unit: ScheduleIntervalUnit::Minutes,
            },
            after,
        )
        .unwrap();

        assert_eq!(next, Utc.with_ymd_and_hms(2026, 5, 21, 10, 30, 0).unwrap());
    }
}
