//! A Redis-compatible store reached through an SSH bastion (ADR-0166).
//!
//! The tunnel every other kind uses forwards one loopback port to one
//! address and rewrites the URL to it. That is not enough here: a cluster
//! names its other nodes only once the client is connected, and the managed
//! services require TLS for a certificate that names the real host, which a
//! rewritten URL would no longer ask for.
//!
//! So the URL is left alone, and the adapter is handed an
//! [`AddressMap`](dbboard_redis::AddressMap) instead. Every time the client
//! is about to connect to a `host:port` — the one in the URL, or a node the
//! cluster named — [`Forwards`] opens a forward to it over one shared SSH
//! session, once, and answers with the forward's loopback address.

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Mutex;

use dbboard_redis::{AddressMap, MapFuture};
use dbboard_tunnel::SshSession;

/// Whatever keeps a forward open while it is held.
type Guard = Box<dyn Send + Sync>;

type OpenFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(SocketAddr, Guard), String>> + Send + 'a>>;

/// Opens one forward to `host:port`. The SSH session in production; a fake
/// in the tests, which need no bastion.
pub(crate) trait Opener: Send + Sync + 'static {
    fn open<'a>(&'a self, host: &'a str, port: u16) -> OpenFuture<'a>;
}

impl Opener for SshSession {
    fn open<'a>(&'a self, host: &'a str, port: u16) -> OpenFuture<'a> {
        Box::pin(async move {
            let tunnel = self
                .forward(host.to_owned(), port)
                .await
                .map_err(|e| format!("ssh tunnel to {host}:{port}: {e}"))?;
            Ok((tunnel.local_addr(), Box::new(tunnel) as Guard))
        })
    }
}

/// One forward per far-side `host:port`, opened the first time it is asked
/// for and kept for as long as the adapter holds this map.
pub(crate) struct Forwards<O> {
    opener: O,
    open: Mutex<HashMap<(String, u16), (SocketAddr, Guard)>>,
}

impl<O: Opener> Forwards<O> {
    pub(crate) fn new(opener: O) -> Self {
        Self {
            opener,
            open: Mutex::new(HashMap::new()),
        }
    }

    fn known(&self, host: &str, port: u16) -> Option<SocketAddr> {
        let open = self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        open.get(&(host.to_owned(), port)).map(|(addr, _)| *addr)
    }
}

impl<O: Opener> AddressMap for Forwards<O> {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> MapFuture<'a> {
        Box::pin(async move {
            if let Some(addr) = self.known(host, port) {
                return Ok(addr);
            }
            // Not held across the await: opening a forward is only a bind and
            // a spawn, so when two connections race for the same node the
            // loser's forward is simply dropped and both use the winner's.
            let opened = self.opener.open(host, port).await?;
            let mut open = self
                .open
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Ok(open.entry((host.to_owned(), port)).or_insert(opened).0)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::sync::Arc;

    /// Hands out loopback ports 40001, 40002, … and counts the forwards that
    /// are still open, by the guards that have not been dropped.
    #[derive(Default)]
    struct Fake {
        opened: AtomicU16,
        alive: Arc<AtomicU16>,
    }

    struct Alive(Arc<AtomicU16>);

    impl Drop for Alive {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl Opener for Arc<Fake> {
        fn open<'a>(&'a self, host: &'a str, _port: u16) -> OpenFuture<'a> {
            Box::pin(async move {
                if host == "refused.internal" {
                    return Err("ssh tunnel to refused.internal:6379: denied".to_owned());
                }
                let n = self.opened.fetch_add(1, Ordering::SeqCst) + 1;
                self.alive.fetch_add(1, Ordering::SeqCst);
                Ok((
                    SocketAddr::from(([127, 0, 0, 1], 40_000 + n)),
                    Box::new(Alive(Arc::clone(&self.alive))) as Guard,
                ))
            })
        }
    }

    #[tokio::test]
    async fn a_node_asked_for_twice_is_forwarded_once() {
        let fake = Arc::new(Fake::default());
        let forwards = Forwards::new(Arc::clone(&fake));
        let first = forwards.resolve("node-1.internal", 6379).await.unwrap();
        let again = forwards.resolve("node-1.internal", 6379).await.unwrap();
        assert_eq!(first, again);
        assert_eq!(fake.opened.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn each_node_gets_its_own_forward() {
        let fake = Arc::new(Fake::default());
        let forwards = Forwards::new(Arc::clone(&fake));
        let a = forwards.resolve("node-1.internal", 6379).await.unwrap();
        let b = forwards.resolve("node-2.internal", 6379).await.unwrap();
        let c = forwards.resolve("node-1.internal", 6380).await.unwrap();
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(fake.opened.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn the_forwards_close_with_the_map() {
        let fake = Arc::new(Fake::default());
        let forwards = Forwards::new(Arc::clone(&fake));
        forwards.resolve("node-1.internal", 6379).await.unwrap();
        forwards.resolve("node-2.internal", 6379).await.unwrap();
        assert_eq!(fake.alive.load(Ordering::SeqCst), 2);
        drop(forwards);
        assert_eq!(fake.alive.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_forward_that_cannot_open_is_not_remembered() {
        let fake = Arc::new(Fake::default());
        let forwards = Forwards::new(Arc::clone(&fake));
        let error = forwards
            .resolve("refused.internal", 6379)
            .await
            .unwrap_err();
        assert!(error.contains("denied"), "{error}");
        assert!(forwards.known("refused.internal", 6379).is_none());
    }
}
