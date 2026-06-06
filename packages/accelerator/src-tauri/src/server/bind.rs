//! TCP bind with `AddrInUse` retry.
//!
//! Waits out a prior instance's listener (e.g. an in-place updater restart releasing `:59833`)
//! before failing — so a genuine conflict (a second instance the user started) still fails fast with
//! the port-in-use signal (surfaced by main.rs) instead of hanging. Extracted from server.rs (Q2).

use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;

/// Bind `addr`, retrying briefly on `AddrInUse` so a just-exited prior instance (a restart overlap)
/// is waited out, while a genuine second-instance conflict still fails fast.
pub(crate) async fn bind_with_retry(addr: SocketAddr) -> std::io::Result<TcpListener> {
    // 100ms polling, 5s budget: the restart overlap clears in well under a
    // second, so this is responsive AND fails a genuine second-instance
    // conflict reasonably fast (it surfaces "port in use" rather than stalling).
    bind_with_retry_inner(addr, Duration::from_millis(100), Duration::from_secs(5)).await
}

/// Inner form with injectable timings so tests can exercise the wait-it-out,
/// hard-deadline, and immediate-propagation paths without real-time sleeps.
async fn bind_with_retry_inner(
    addr: SocketAddr,
    interval: Duration,
    max_wait: Duration,
) -> std::io::Result<TcpListener> {
    let deadline = std::time::Instant::now() + max_wait;
    let mut warned = false;
    loop {
        match TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                // Hard deadline: sleep only the time actually left, so we give up
                // at ~max_wait rather than overshooting by a full `interval`.
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    tracing::warn!(
                        "port {} still in use after {max_wait:?} — giving up",
                        addr.port()
                    );
                    return Err(e);
                }
                if !warned {
                    tracing::warn!(
                        "port {} in use — retrying for up to {max_wait:?} (waiting out a prior instance, e.g. an in-place updater restart)",
                        addr.port()
                    );
                    warned = true;
                }
                tokio::time::sleep(interval.min(remaining)).await;
            }
            // Any non-AddrInUse error propagates immediately — never masked by the retry.
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bind_with_retry_waits_out_a_transient_holder() {
        // The in-place-restart case: hold a freshly-chosen port, release it
        // shortly, and assert the retry binds once it frees.
        let probe = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap();
        let addr = probe.local_addr().unwrap();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            drop(probe);
        });
        let listener =
            bind_with_retry_inner(addr, Duration::from_millis(20), Duration::from_secs(2))
                .await
                .expect("should bind once the transient holder releases the port");
        assert_eq!(listener.local_addr().unwrap().port(), addr.port());
    }

    #[tokio::test]
    async fn bind_with_retry_gives_up_on_a_persistent_conflict_at_a_hard_deadline() {
        // A second instance, not a restart overlap: a port held for the whole
        // window must fail with AddrInUse (so main.rs surfaces "port in use").
        // `interval` is deliberately LARGER than `budget` so a HARD deadline caps
        // at ~budget (sleeping only the remaining time) while a SOFT one would
        // sleep a full interval and overshoot — the elapsed assertion catches
        // that regression.
        let probe = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .unwrap();
        let addr = probe.local_addr().unwrap();
        let budget = Duration::from_millis(200);
        let interval = Duration::from_millis(500);
        let started = std::time::Instant::now();
        let err = bind_with_retry_inner(addr, interval, budget)
            .await
            .expect_err("should give up while the port stays held");
        let elapsed = started.elapsed();
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
        assert!(
            elapsed < budget + Duration::from_millis(150),
            "hard deadline overshot: {elapsed:?} (budget {budget:?}, interval {interval:?}) — a soft deadline would sleep a full interval past the budget"
        );
        drop(probe);
    }

    #[tokio::test]
    async fn bind_with_retry_propagates_non_addrinuse_immediately() {
        // An unassigned TEST-NET-1 address (RFC 5737) can't be bound →
        // AddrNotAvailable, NOT AddrInUse. It must return at once, never entering
        // the retry budget (a 10s budget would expose a wrongful retry).
        let bad = SocketAddr::from(([192, 0, 2, 1], 0));
        let started = std::time::Instant::now();
        let err = bind_with_retry_inner(bad, Duration::from_millis(100), Duration::from_secs(10))
            .await
            .expect_err("binding an unassigned address must fail");
        assert_ne!(err.kind(), std::io::ErrorKind::AddrInUse);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a non-AddrInUse error must propagate immediately, not retry for the budget"
        );
    }
}
