use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Weak};
use tokio::sync::{Mutex, OwnedMutexGuard};

static FLIGHTS: LazyLock<Mutex<HashMap<String, Weak<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Serializes concurrent work for the same cache identity while allowing unrelated keys.
pub async fn acquire_singleflight(key: &str) -> OwnedMutexGuard<()> {
    let lock = {
        let mut flights = FLIGHTS.lock().await;
        flights.retain(|_, weak| weak.strong_count() > 0);
        if let Some(existing) = flights.get(key).and_then(Weak::upgrade) {
            existing
        } else {
            let created = Arc::new(Mutex::new(()));
            flights.insert(key.to_string(), Arc::downgrade(&created));
            created
        }
    };

    lock.lock_owned().await
}

#[cfg(test)]
mod tests {
    use super::acquire_singleflight;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Verifies that identical keys never execute their critical sections concurrently.
    #[tokio::test]
    async fn identical_keys_are_serialized() {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();

        for _ in 0..8 {
            let active = active.clone();
            let peak = peak.clone();
            tasks.push(tokio::spawn(async move {
                let _guard = acquire_singleflight("description:test").await;
                let now = active.fetch_add(1, Ordering::AcqRel) + 1;
                peak.fetch_max(now, Ordering::AcqRel);
                tokio::task::yield_now().await;
                active.fetch_sub(1, Ordering::AcqRel);
            }));
        }

        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(peak.load(Ordering::Acquire), 1);
    }
}
