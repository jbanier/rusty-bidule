use std::time::Duration;

use anyhow::Result;
use chrono::{Duration as ChronoDuration, Utc};
use tokio::sync::mpsc::unbounded_channel;
use tracing::{info, warn};

use crate::{orchestrator::Orchestrator, types::UiEvent};

#[derive(Clone)]
pub struct AutoPullRuntime {
    orchestrator: Orchestrator,
    poll_interval: Duration,
}

impl AutoPullRuntime {
    pub fn new(orchestrator: Orchestrator) -> Self {
        Self {
            orchestrator,
            poll_interval: Duration::from_secs(1),
        }
    }

    pub fn start(self) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(self.poll_interval);
            loop {
                ticker.tick().await;
                if let Err(err) = self.tick().await {
                    warn!(error = %err, "auto-pull tick failed");
                }
            }
        });
    }

    async fn tick(&self) -> Result<()> {
        let now = Utc::now();
        let due_jobs = self.orchestrator.store().list_due_jobs(now)?;
        for (conversation_id, due_job) in due_jobs {
            let mut jobs = self.orchestrator.store().load_job_state(&conversation_id)?;
            let Some(job_index) = jobs.iter().position(|job| job.alias == due_job.alias) else {
                continue;
            };
            if !jobs[job_index].is_due_for_poll(now) {
                continue;
            }
            jobs[job_index].lease_expires_at = Some(now + ChronoDuration::seconds(60));
            jobs[job_index].updated_at = now;
            let claimed_job = jobs[job_index].clone();
            self.orchestrator
                .store()
                .save_job_state(&conversation_id, &jobs)?;

            info!(%conversation_id, alias = %claimed_job.alias, "running auto-pull job");
            let (ui_tx, _ui_rx) = unbounded_channel::<UiEvent>();
            let result = self
                .orchestrator
                .run_automation_turn(&conversation_id, &claimed_job, ui_tx)
                .await;

            let mut jobs = self.orchestrator.store().load_job_state(&conversation_id)?;
            if let Some(job) = jobs.iter_mut().find(|job| job.alias == due_job.alias) {
                job.updated_at = Utc::now();
                job.lease_expires_at = None;
                match result {
                    Ok(_) => {
                        job.last_error = None;
                        if let Some(interval) = job.poll_interval_seconds {
                            job.next_poll_at =
                                Some(Utc::now() + ChronoDuration::seconds(interval as i64));
                        }
                        if job.status.is_none() {
                            job.status = Some("automation_checked".to_string());
                        }
                    }
                    Err(err) => {
                        job.last_error = Some(format!("{err:#}"));
                        if let Some(interval) = job.poll_interval_seconds {
                            job.next_poll_at =
                                Some(Utc::now() + ChronoDuration::seconds(interval as i64));
                        }
                    }
                }
            }
            self.orchestrator
                .store()
                .save_job_state(&conversation_id, &jobs)?;
        }
        Ok(())
    }
}
