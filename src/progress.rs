//! Thread-safe progress and approximate wall-clock ETA. No invented completion percentages.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

#[derive(Clone)]
pub struct Progress(Arc<Inner>);
struct Inner {
    started: Instant,
    key: String,
    previous_s: Option<f64>,
    state: Mutex<State>,
}
struct State {
    phase: String,
    started: Instant,
    done: f64,
    total: f64,
    finished: bool,
    failed: bool,
}
pub struct Status {
    pub phase: String,
    pub elapsed_s: f64,
    pub remaining_s: Option<f64>,
    pub fraction: Option<f32>,
    pub historical: bool,
    pub overrun: bool,
}
type History = BTreeMap<String, Vec<f64>>;
fn history() -> &'static Mutex<History> {
    static HISTORY: OnceLock<Mutex<History>> = OnceLock::new();
    HISTORY.get_or_init(|| {
        Mutex::new(
            std::fs::read("output/runtime-estimates.json")
                .ok()
                .and_then(|v| serde_json::from_slice(&v).ok())
                .unwrap_or_default(),
        )
    })
}
fn estimate(
    elapsed: f64,
    done: f64,
    total: f64,
    previous: Option<f64>,
    finished: bool,
) -> (Option<f64>, bool, bool) {
    if finished {
        return (Some(0.0), false, false);
    }
    if total > 0.0 && done > 0.0 && done < total && elapsed >= 0.15 {
        return (
            Some((elapsed * (total - done) / done).max(0.1)),
            false,
            false,
        );
    }
    if let Some(previous) = previous.filter(|v| v.is_finite() && *v > 0.0) {
        if elapsed < previous {
            return (Some(previous - elapsed), true, false);
        }
        return (None, true, true);
    }
    (None, false, false)
}
impl Progress {
    pub fn new(phase: impl Into<String>, key: impl Into<String>) -> Self {
        let key = key.into();
        let previous_s = history().lock().unwrap().get(&key).and_then(|samples| {
            let mut samples: Vec<_> = samples
                .iter()
                .copied()
                .filter(|x| x.is_finite() && *x > 0.0)
                .collect();
            samples.sort_by(f64::total_cmp);
            samples.get(samples.len() / 2).copied()
        });
        let now = Instant::now();
        Self(Arc::new(Inner {
            started: now,
            key,
            previous_s,
            state: Mutex::new(State {
                phase: phase.into(),
                started: now,
                done: 0.0,
                total: 0.0,
                finished: false,
                failed: false,
            }),
        }))
    }
    pub fn stage(&self, phase: impl Into<String>, total: f64) {
        let mut state = self.0.state.lock().unwrap();
        state.phase = phase.into();
        state.started = Instant::now();
        state.done = 0.0;
        state.total = total.max(0.0);
    }
    pub fn update(&self, done: f64) {
        if !done.is_finite() {
            return;
        }
        let mut s = self.0.state.lock().unwrap();
        s.done = done.clamp(s.done, s.total.max(s.done));
    }
    pub fn extend_total(&self, phase: impl Into<String>, total: f64) {
        let mut s = self.0.state.lock().unwrap();
        s.phase = phase.into();
        s.total = total.max(s.total);
    }
    pub fn finish(&self, success: bool) {
        let mut state = self.0.state.lock().unwrap();
        if state.finished {
            return;
        }
        state.finished = true;
        state.failed = !success;
        if !success {
            return;
        }
        let elapsed = self.0.started.elapsed().as_secs_f64();
        let mut h = history().lock().unwrap();
        let samples = h.entry(self.0.key.clone()).or_default();
        samples.push(elapsed);
        if samples.len() > 7 {
            samples.remove(0);
        }
        let _ = std::fs::create_dir_all("output");
        if let Ok(data) = serde_json::to_vec_pretty(&*h) {
            let _ = std::fs::write("output/runtime-estimates.json", data);
        }
    }
    pub fn status(&self) -> Status {
        let s = self.0.state.lock().unwrap();
        let elapsed_s = self.0.started.elapsed().as_secs_f64();
        let stage_elapsed = s.started.elapsed().as_secs_f64();
        let (remaining_s, historical, overrun) = if s.failed {
            (None, false, false)
        } else if s.done > 0.0 && s.done < s.total {
            estimate(stage_elapsed, s.done, s.total, None, s.finished)
        } else {
            estimate(elapsed_s, 0.0, 0.0, self.0.previous_s, s.finished)
        };
        Status {
            phase: s.phase.clone(),
            elapsed_s,
            remaining_s,
            historical,
            overrun,
            fraction: if s.failed {
                None
            } else if s.finished {
                Some(1.0)
            } else if s.total > 0.0 {
                Some((s.done / s.total).clamp(0.0, 0.99) as f32)
            } else {
                None
            },
        }
    }
}
pub fn duration(seconds: f64) -> String {
    let seconds = seconds.max(0.0).ceil() as u64;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {:02}m", seconds / 3600, seconds / 60 % 60)
    }
}
impl Status {
    pub fn label(&self) -> String {
        let eta = self
            .remaining_s
            .map(|s| {
                format!(
                    "about {} left{}",
                    duration(s),
                    if self.historical {
                        " (previous runs)"
                    } else {
                        " in this phase"
                    }
                )
            })
            .unwrap_or_else(|| {
                if self.overrun {
                    "Taking longer than previous runs; updating estimate".into()
                } else {
                    "Estimating remaining time…".into()
                }
            });
        format!(
            "{} · {} elapsed · {eta}",
            self.phase,
            duration(self.elapsed_s)
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measured_work_eta_scales_with_actual_progress() {
        assert_eq!(
            estimate(2., 25., 100., None, false),
            (Some(6.), false, false)
        );
        assert_eq!(
            estimate(4., 50., 100., None, false),
            (Some(4.), false, false)
        );
        assert_eq!(estimate(0.01, 1., 100., None, false), (None, false, false));
    }
    #[test]
    fn historical_overruns_never_pretend_complete() {
        assert_eq!(
            estimate(3., 0., 0., Some(10.), false),
            (Some(7.), true, false)
        );
        assert_eq!(estimate(12., 0., 0., Some(10.), false), (None, true, true));
        assert_eq!(
            estimate(12., 100., 100., Some(10.), false),
            (None, true, true)
        );
        assert_eq!(
            estimate(12., 0., 0., Some(10.), true),
            (Some(0.), false, false)
        );
    }
    #[test]
    fn shared_progress_is_monotonic_and_never_100_percent_before_completion() {
        let p = Progress::new("test", "unit-progress");
        p.stage("GPU steps", 100.);
        let worker = p.clone();
        worker.update(50.);
        p.update(25.);
        assert_eq!(p.status().fraction, Some(0.5));
        p.update(100.);
        assert_eq!(p.status().fraction, Some(0.99));
        assert_eq!(duration(60.1), "1m 01s");
        assert_eq!(duration(3600.), "1h 00m");
    }
    #[test]
    fn failed_work_is_not_reported_as_complete() {
        let p = Progress::new("test", "unit-failure");
        p.stage("work", 100.);
        p.update(50.);
        p.extend_total("longer work", 200.);
        assert_eq!(p.status().fraction, Some(0.25));
        p.finish(false);
        assert_eq!(p.status().fraction, None);
        assert_eq!(p.status().remaining_s, None);
    }
}
