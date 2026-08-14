use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const ACCOUNT_RATE_WINDOW: Duration = Duration::from_secs(60);
const ACCOUNT_RATE_STATE_SOFT_CAPACITY: usize = 4096;

static ACCOUNT_REQUEST_STARTS: OnceLock<Mutex<HashMap<String, VecDeque<Instant>>>> =
    OnceLock::new();

fn state() -> &'static Mutex<HashMap<String, VecDeque<Instant>>> {
    ACCOUNT_REQUEST_STARTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn retain_current_window(starts: &mut VecDeque<Instant>, now: Instant) {
    while starts
        .front()
        .is_some_and(|started_at| now.saturating_duration_since(*started_at) >= ACCOUNT_RATE_WINDOW)
    {
        starts.pop_front();
    }
}

fn try_acquire_at(account_id: &str, limit: usize, now: Instant) -> bool {
    if limit == 0 {
        return true;
    }
    let lock = state();
    let mut requests = crate::lock_utils::lock_recover(lock, "account_request_rate_gate");
    if requests.len() > ACCOUNT_RATE_STATE_SOFT_CAPACITY {
        requests.retain(|_, starts| {
            retain_current_window(starts, now);
            !starts.is_empty()
        });
    }
    let starts = requests.entry(account_id.to_string()).or_default();
    retain_current_window(starts, now);
    if starts.len() >= limit {
        return false;
    }
    starts.push_back(now);
    true
}

pub(super) fn try_acquire_account_request_slot(account_id: &str, limit: usize) -> bool {
    try_acquire_at(account_id, limit, Instant::now())
}

pub(super) fn clear_runtime_state() {
    if let Some(lock) = ACCOUNT_REQUEST_STARTS.get() {
        crate::lock_utils::lock_recover(lock, "account_request_rate_gate").clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_window_releases_capacity_after_sixty_seconds() {
        let account_id = "rate-window-account";
        let now = Instant::now();
        assert!(try_acquire_at(account_id, 2, now));
        assert!(try_acquire_at(account_id, 2, now + Duration::from_secs(10)));
        assert!(!try_acquire_at(
            account_id,
            2,
            now + Duration::from_secs(59)
        ));
        assert!(try_acquire_at(account_id, 2, now + Duration::from_secs(60)));
    }

    #[test]
    fn zero_limit_disables_rate_gate() {
        let now = Instant::now();
        for _ in 0..100 {
            assert!(try_acquire_at("rate-disabled-account", 0, now));
        }
    }

    #[test]
    fn concurrent_acquisition_never_exceeds_limit() {
        let account_id = "rate-concurrent-account";
        let accepted = std::thread::scope(|scope| {
            let handles = (0..24)
                .map(|_| scope.spawn(|| try_acquire_account_request_slot(account_id, 5)))
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("rate gate worker"))
                .filter(|accepted| *accepted)
                .count()
        });
        assert_eq!(accepted, 5);
    }
}
