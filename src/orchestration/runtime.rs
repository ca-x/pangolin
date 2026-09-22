//! Process-local admission and routing state. Every acquired permit is RAII-owned,
//! including queued cancellation and downstream body cancellation.
use std::{
    collections::VecDeque,
    num::NonZeroU32,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use dashmap::DashMap;
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use moka::sync::Cache;
use rand::Rng;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::{
    Candidate, Error, Result,
    policy::{CircuitPolicy, Limits, Strategy},
};

pub struct Runtime {
    pub generation: AtomicU64,
    resources: DashMap<String, Arc<Resource>>,
    circuits: DashMap<String, Arc<Mutex<Circuit>>>,
    rotations: Cache<String, Arc<AtomicU64>>,
    affinity: Cache<String, String>,
    pub sessions: super::session::Sessions,
    pub affinity_rules: super::affinity::Cache,
    pub websocket_pins: Cache<String, String>,
    pub upstream_clients: crate::providers::ClientPool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            generation: AtomicU64::new(0),
            resources: DashMap::new(),
            circuits: DashMap::new(),
            rotations: Cache::builder()
                .max_capacity(10_000)
                .time_to_idle(Duration::from_secs(1800))
                .build(),
            affinity: Cache::builder()
                .max_capacity(10_000)
                .time_to_idle(Duration::from_secs(1800))
                .build(),
            sessions: Default::default(),
            affinity_rules: Default::default(),
            websocket_pins: Cache::builder()
                .max_capacity(10000)
                .time_to_idle(Duration::from_secs(86400))
                .build(),
            upstream_clients: Default::default(),
        }
    }
}

pub struct Resource {
    limits: Limits,
    slots: Arc<Semaphore>,
    waiting: Arc<Semaphore>,
    rpm: Option<DefaultDirectRateLimiter>,
    tpm: Option<DefaultDirectRateLimiter>,
    // Serializes multi-quota admission so parallel requests cannot overshoot either bucket.
    admission: Mutex<()>,
    pub inflight: AtomicU64,
    pub completed: AtomicU64,
    pub failed: AtomicU64,
    latency_us: AtomicU64,
}

fn limiter(limit: Option<u32>) -> Option<DefaultDirectRateLimiter> {
    limit
        .and_then(NonZeroU32::new)
        .map(|n| RateLimiter::direct(Quota::per_minute(n)))
}

impl Resource {
    fn new(limits: Limits) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(
                limits.concurrent.unwrap_or(1_000_000) as usize
            )),
            waiting: Arc::new(Semaphore::new(limits.queue as usize)),
            rpm: limiter(limits.rpm),
            tpm: limiter(limits.tpm),
            limits,
            admission: Mutex::new(()),
            inflight: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            latency_us: AtomicU64::new(0),
        }
    }
}

impl Runtime {
    pub fn reset_derived(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.resources.clear();
        self.circuits.clear();
        self.rotations.invalidate_all();
        self.affinity.invalidate_all();
        self.websocket_pins.invalidate_all();
        self.affinity_rules.reset();
        self.sessions.clear();
    }
    #[cfg(test)]
    pub fn measured_latency(&self, resource: &str) -> u64 {
        self.resources
            .get(resource)
            .map_or(0, |resource| resource.latency_us.load(Ordering::Relaxed))
    }

    pub fn metrics(&self) -> String {
        // The existing metrics endpoint is public. Aggregate by resource kind so
        // it never reveals project, channel or API-key identifiers.
        let mut totals = std::collections::BTreeMap::<&str, [u64; 4]>::new();
        for entry in &self.resources {
            let kind = if entry.key().starts_with("channel:") {
                "channel"
            } else if entry.key().starts_with("key:") {
                "key"
            } else {
                "other"
            };
            let resource = entry.value();
            let counters = [
                resource.inflight.load(Ordering::Relaxed),
                resource.completed.load(Ordering::Relaxed),
                resource.failed.load(Ordering::Relaxed),
                (resource.limits.queue as usize - resource.waiting.available_permits()) as u64,
            ];
            let total = totals.entry(kind).or_default();
            for (sum, value) in total.iter_mut().zip(counters) {
                *sum = sum.saturating_add(value);
            }
        }
        totals.into_iter().map(|(kind, [inflight, completed, failed, queued])| format!(
            "pangolin_orchestration_inflight{{resource=\"{kind}\"}} {inflight}\npangolin_orchestration_completed_total{{resource=\"{kind}\"}} {completed}\npangolin_orchestration_failed_total{{resource=\"{kind}\"}} {failed}\npangolin_orchestration_queued{{resource=\"{kind}\"}} {queued}\n"
        )).collect()
    }

    fn resource(&self, id: &str, limits: &Limits) -> Arc<Resource> {
        // A policy change is installed only while idle. Live permits keep their original
        // semaphore, preventing a settings update from admitting a second concurrent pool.
        let mut entry = self
            .resources
            .entry(id.to_owned())
            .or_insert_with(|| Arc::new(Resource::new(limits.clone())));
        if entry.limits != *limits && Arc::strong_count(&entry) == 1 {
            *entry = Arc::new(Resource::new(limits.clone()));
        }
        entry.clone()
    }

    pub async fn admit(&self, id: &str, limits: &Limits, tokens: u32) -> Result<Permit> {
        let resource = self.resource(id, limits);
        if resource.limits.concurrent == Some(0)
            || resource.limits.rpm == Some(0)
            || resource.limits.tpm == Some(0)
        {
            return Err(Error::Admission("disabled"));
        }
        let slot = match resource.slots.clone().try_acquire_owned() {
            Ok(slot) => slot,
            Err(_) => {
                let _queue = resource
                    .waiting
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| Error::Admission("queue_full"))?;
                tokio::time::timeout(
                    Duration::from_millis(resource.limits.queue_timeout_ms),
                    resource.slots.clone().acquire_owned(),
                )
                .await
                .map_err(|_| Error::Admission("queue_timeout"))?
                .map_err(|_| Error::Admission("closed"))?
            }
        };
        {
            let _lock = resource.admission.lock().unwrap_or_else(|e| e.into_inner());
            // Token capacity is checked before spending RPM; a rejected token reservation
            // can conservatively spend capacity, but can never bypass either limit.
            if let Some(tpm) = &resource.tpm
                && let Some(tokens) = NonZeroU32::new(tokens)
                && !matches!(tpm.check_n(tokens), Ok(Ok(())))
            {
                return Err(Error::Admission("tpm"));
            }
            if let Some(rpm) = &resource.rpm
                && rpm.check().is_err()
            {
                return Err(Error::Admission("rpm"));
            }
            resource.inflight.fetch_add(1, Ordering::Relaxed);
        }
        Ok(Permit {
            resource,
            _slot: slot,
            started: Instant::now(),
            outcome: None,
        })
    }

    pub fn order(
        &self,
        scope: &str,
        candidates: &mut [Candidate],
        strategy: Strategy,
        sticky: Option<&str>,
    ) {
        if candidates.is_empty() {
            return;
        }
        // Priority tiers remain strict for every strategy. Inside a tier the
        // credential's own priority decides, and IDs break what is still tied.
        candidates.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then(a.credential_priority.cmp(&b.credential_priority))
                .then(a.id().cmp(&b.id()))
        });
        let tick = if strategy == Strategy::RoundRobin && candidates.len() > 1 {
            // A fixed-size digest also bounds memory for unusually long model IDs.
            self.rotations
                .get_with(blake3::hash(scope.as_bytes()).to_hex().to_string(), || {
                    Arc::new(AtomicU64::new(0))
                })
                .fetch_add(1, Ordering::Relaxed) as usize
        } else {
            0
        };
        let mut offset = 0;
        while offset < candidates.len() {
            let len = candidates[offset..]
                .iter()
                .take_while(|c| c.priority == candidates[offset].priority)
                .count();
            let tier = &mut candidates[offset..offset + len];
            match strategy {
                Strategy::Failover => (),
                Strategy::RoundRobin => tier.rotate_left(tick % len),
                Strategy::Weighted => {
                    // Weighted sampling without replacement; a seeded simulation is tested
                    // separately through the same selector.
                    weighted_order(tier, &mut rand::rng());
                }
                Strategy::LeastInflight | Strategy::Latency | Strategy::Adaptive => tier
                    .sort_by_key(|candidate| {
                        let score = self
                            .resources
                            .get(&candidate.resource_id())
                            .map(|resource| {
                                let inflight = resource.inflight.load(Ordering::Relaxed);
                                let latency = resource.latency_us.load(Ordering::Relaxed).max(1);
                                match strategy {
                                    Strategy::LeastInflight => inflight,
                                    Strategy::Latency => latency,
                                    _ => latency.saturating_mul(inflight.saturating_add(1)),
                                }
                            })
                            .unwrap_or(0);
                        (score, candidate.id())
                    }),
            }
            offset += len;
        }
        if let Some(key) = sticky
            && let Some(id) = self.affinity.get(key)
            && let Some(index) = candidates.iter().position(|c| c.id() == id)
        {
            candidates[..=index].rotate_right(1);
        }
    }

    pub fn bind(&self, sticky: Option<&str>, candidate: &Candidate) {
        if let Some(key) = sticky {
            self.affinity.insert(key.to_owned(), candidate.id());
        }
    }

    pub fn circuit_available(&self, key: &str, policy: &CircuitPolicy) -> bool {
        if !policy.enabled {
            return true;
        }
        self.circuits.get(key).is_none_or(|state| {
            let state = state.lock().unwrap_or_else(|e| e.into_inner());
            state.opened.is_none_or(|opened| {
                opened.elapsed() >= Duration::from_millis(policy.recovery_ms) && !state.probing
            })
        })
    }

    pub fn enter_circuit(&self, key: &str, policy: &CircuitPolicy) -> Result<CircuitPermit> {
        let state = self
            .circuits
            .entry(key.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(Circuit::default())))
            .clone();
        let mut circuit = state.lock().unwrap_or_else(|e| e.into_inner());
        let mut probe = false;
        if policy.enabled
            && let Some(opened) = circuit.opened
        {
            if opened.elapsed() < Duration::from_millis(policy.recovery_ms) || circuit.probing {
                return Err(Error::Admission("circuit_open"));
            }
            circuit.probing = true;
            probe = true;
        }
        drop(circuit);
        Ok(CircuitPermit {
            state,
            policy: policy.clone(),
            probe,
            settled: false,
        })
    }
}

pub fn weighted_order(candidates: &mut [Candidate], rng: &mut impl Rng) {
    for index in 0..candidates.len() {
        let total: u64 = candidates[index..]
            .iter()
            .map(|c| u64::from(c.weight))
            .sum();
        let mut draw = rng.random_range(0..total);
        let chosen = candidates[index..]
            .iter()
            .position(|c| {
                if draw < u64::from(c.weight) {
                    true
                } else {
                    draw -= u64::from(c.weight);
                    false
                }
            })
            .expect("positive weights cover the sample");
        candidates.swap(index, index + chosen);
    }
}

pub struct Permit {
    resource: Arc<Resource>,
    _slot: OwnedSemaphorePermit,
    started: Instant,
    outcome: Option<bool>,
}

impl Permit {
    pub fn reserve_retry_tokens(&self, tokens: u32) -> Result<()> {
        let _lock = self
            .resource
            .admission
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(tpm) = &self.resource.tpm
            && let Some(tokens) = NonZeroU32::new(tokens)
            && !matches!(tpm.check_n(tokens), Ok(Ok(())))
        {
            return Err(Error::Admission("tpm"));
        }
        Ok(())
    }

    pub fn finish(&mut self, success: bool) {
        self.outcome = Some(success);
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.resource.inflight.fetch_sub(1, Ordering::Relaxed);
        self.resource.completed.fetch_add(1, Ordering::Relaxed);
        if self.outcome != Some(true) {
            self.resource.failed.fetch_add(1, Ordering::Relaxed);
        }
        if self.outcome == Some(true) {
            let elapsed = self.started.elapsed().as_micros().min(u64::MAX as u128) as u64;
            let _ = self.resource.latency_us.fetch_update(
                Ordering::Relaxed,
                Ordering::Relaxed,
                |previous| {
                    Some(if previous == 0 {
                        elapsed
                    } else {
                        previous.saturating_mul(7).saturating_add(elapsed) / 8
                    })
                },
            );
        }
    }
}

#[derive(Default)]
struct Circuit {
    failures: VecDeque<Instant>,
    opened: Option<Instant>,
    probing: bool,
}

pub struct CircuitPermit {
    state: Arc<Mutex<Circuit>>,
    policy: CircuitPolicy,
    probe: bool,
    settled: bool,
}

impl CircuitPermit {
    pub fn finish(&mut self, success: bool) {
        if self.settled || !self.policy.enabled {
            return;
        }
        self.settled = true;
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if self.probe {
            state.probing = false;
        }
        if success {
            if self.probe {
                state.opened = None;
                state.failures.clear();
            }
            return;
        }
        let now = Instant::now();
        while state
            .failures
            .front()
            .is_some_and(|f| f.elapsed() >= Duration::from_millis(self.policy.window_ms))
        {
            state.failures.pop_front();
        }
        state.failures.push_back(now);
        if self.probe || state.failures.len() >= self.policy.failures {
            state.opened = Some(now);
        }
    }
}

impl Drop for CircuitPermit {
    fn drop(&mut self) {
        // Local cancellation is not an upstream failure, but must release a half-open slot.
        if self.probe && !self.settled {
            self.state.lock().unwrap_or_else(|e| e.into_inner()).probing = false;
        }
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn review_unknown_models_do_not_allocate_rotation_state() {
        let runtime = Runtime::default();
        for i in 0..20_000 {
            runtime.order(&format!("unknown-{i}"), &mut [], Strategy::RoundRobin, None);
        }
        runtime.rotations.run_pending_tasks();
        assert_eq!(runtime.rotations.entry_count(), 0);
    }

    #[tokio::test]
    async fn review_rotation_cache_is_bounded_expiring_and_only_used_for_round_robin() {
        let runtime = Runtime {
            rotations: Cache::builder()
                .max_capacity(16)
                .time_to_idle(Duration::from_millis(20))
                .build(),
            ..Runtime::default()
        };
        let mut candidates = vec![
            super::super::tests::candidate("a", 1),
            super::super::tests::candidate("b", 1),
        ];
        for strategy in [
            Strategy::Failover,
            Strategy::Weighted,
            Strategy::Latency,
            Strategy::LeastInflight,
            Strategy::Adaptive,
        ] {
            for i in 0..100 {
                runtime.order(&format!("unused-{i}"), &mut candidates, strategy, None);
            }
        }
        runtime.rotations.run_pending_tasks();
        assert_eq!(runtime.rotations.entry_count(), 0);
        for i in 0..100 {
            runtime.order(
                &format!("used-{i}"),
                &mut candidates,
                Strategy::RoundRobin,
                None,
            );
        }
        runtime.rotations.run_pending_tasks();
        assert!(runtime.rotations.entry_count() <= 16);
        tokio::time::sleep(Duration::from_millis(30)).await;
        runtime.rotations.run_pending_tasks();
        assert_eq!(runtime.rotations.entry_count(), 0);
    }
}
