use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Running,
    Paused,
    Finished,
}

#[derive(Debug)]
pub struct TimerState {
    pub duration_secs: u64,
    pub phase: Phase,
    pub end_at: Option<Instant>,
    pub paused_remaining: Option<Duration>,
}

impl Default for TimerState {
    fn default() -> Self {
        Self {
            duration_secs: 25 * 60,
            phase: Phase::Idle,
            end_at: None,
            paused_remaining: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TimerSnapshot {
    pub duration_secs: u64,
    pub remaining_secs: u64,
    pub phase: Phase,
}

impl TimerState {
    pub fn snapshot(&self) -> TimerSnapshot {
        TimerSnapshot {
            duration_secs: self.duration_secs,
            remaining_secs: self.remaining().as_secs(),
            phase: self.phase,
        }
    }

    pub fn remaining(&self) -> Duration {
        match self.phase {
            Phase::Running => self
                .end_at
                .map(|end| end.saturating_duration_since(Instant::now()))
                .unwrap_or_default(),
            Phase::Paused => self.paused_remaining.unwrap_or_default(),
            Phase::Idle => Duration::from_secs(self.duration_secs),
            Phase::Finished => Duration::ZERO,
        }
    }

    pub fn set_duration(&mut self, secs: u64) {
        self.duration_secs = secs.min(3600);
        match self.phase {
            Phase::Idle | Phase::Finished => {
                self.phase = Phase::Idle;
                self.end_at = None;
                self.paused_remaining = None;
            }
            Phase::Paused => {
                self.paused_remaining = Some(Duration::from_secs(self.duration_secs));
            }
            Phase::Running => {
                self.end_at = Some(Instant::now() + Duration::from_secs(self.duration_secs));
            }
        }
    }

    pub fn start(&mut self) {
        let remaining = match self.phase {
            Phase::Paused => self
                .paused_remaining
                .unwrap_or_else(|| Duration::from_secs(self.duration_secs)),
            _ => Duration::from_secs(self.duration_secs),
        };
        if remaining.is_zero() {
            return;
        }
        self.end_at = Some(Instant::now() + remaining);
        self.paused_remaining = None;
        self.phase = Phase::Running;
    }

    pub fn pause(&mut self) {
        if matches!(self.phase, Phase::Running) {
            let remaining = self.remaining();
            self.paused_remaining = Some(remaining);
            self.end_at = None;
            self.phase = Phase::Paused;
        }
    }

    pub fn reset(&mut self) {
        self.end_at = None;
        self.paused_remaining = None;
        self.phase = Phase::Idle;
    }

    /// Advance state — returns true on the tick that just transitioned to Finished.
    pub fn tick(&mut self) -> bool {
        if self.phase == Phase::Running && self.remaining().is_zero() {
            self.phase = Phase::Finished;
            self.end_at = None;
            return true;
        }
        false
    }
}
