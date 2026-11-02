//! Reaching the servers through something other than the network directly
//! (ADR-0166).
//!
//! A managed store usually lives inside a private network, and the way in
//! from a laptop is an SSH bastion. A forward to one address is not enough
//! for it, for two reasons. In cluster mode the server answers with the
//! addresses of the other nodes, and the client goes on to connect to them.
//! And with TLS the certificate names the real host, so the client has to
//! keep asking for that name even while it connects somewhere else.
//!
//! Both are solved at the one point where a host name becomes a socket
//! address. The client keeps every host name it was given or told about,
//! and so keeps the TLS server name, while an [`AddressMap`] decides which
//! socket stands for it — typically a loopback forward it opens on first
//! use. This crate knows nothing about SSH: the map is supplied by whoever
//! wires the adapter up.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use redis::io::AsyncDNSResolver;
use redis::{ErrorKind, RedisError, RedisFuture};

/// The future an [`AddressMap`] answers with.
pub type MapFuture<'a> = Pin<Box<dyn Future<Output = Result<SocketAddr, String>> + Send + 'a>>;

/// Decides which socket address stands for a server's `host:port`.
///
/// It is asked once for every connection the client opens: the address in
/// the URL, and in cluster mode every node the server names. The error is a
/// message for the person connecting, and must not carry a secret.
pub trait AddressMap: Send + Sync + 'static {
    /// The socket to connect to for `host:port`.
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> MapFuture<'a>;
}

/// An [`AddressMap`] in the shape the `redis` crate asks for.
pub(crate) struct Resolver(pub(crate) Arc<dyn AddressMap>);

impl AsyncDNSResolver for Resolver {
    fn resolve<'a, 'b: 'a>(
        &'a self,
        host: &'b str,
        port: u16,
    ) -> RedisFuture<'a, Box<dyn Iterator<Item = SocketAddr> + Send + 'a>> {
        Box::pin(async move {
            let addr = self.0.resolve(host, port).await.map_err(|detail| {
                RedisError::from((ErrorKind::Io, "could not reach the server", detail))
            })?;
            Ok(Box::new(std::iter::once(addr)) as Box<dyn Iterator<Item = SocketAddr> + Send>)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Answers every name with port 7000 + the asked port's last digit, and
    /// records what it was asked.
    #[derive(Default)]
    struct Fake {
        asked: Mutex<Vec<(String, u16)>>,
    }

    impl AddressMap for Fake {
        fn resolve<'a>(&'a self, host: &'a str, port: u16) -> MapFuture<'a> {
            self.asked.lock().unwrap().push((host.to_owned(), port));
            Box::pin(async move {
                if host == "unreachable.internal" {
                    return Err("the bastion refused the forward".to_owned());
                }
                Ok(SocketAddr::from(([127, 0, 0, 1], 7000 + port % 10)))
            })
        }
    }

    #[tokio::test]
    async fn the_map_decides_the_socket_for_the_name_the_client_asked_for() {
        let fake = Arc::new(Fake::default());
        let resolver = Resolver(fake.clone());
        let addrs: Vec<_> = resolver
            .resolve("node-1.cache.internal", 6379)
            .await
            .unwrap()
            .collect();
        assert_eq!(addrs, [SocketAddr::from(([127, 0, 0, 1], 7009))]);
        assert_eq!(
            *fake.asked.lock().unwrap(),
            [("node-1.cache.internal".to_owned(), 6379)]
        );
    }

    #[tokio::test]
    async fn a_map_that_cannot_answer_is_an_io_error_that_says_why() {
        let resolver = Resolver(Arc::new(Fake::default()));
        let Err(error) = resolver.resolve("unreachable.internal", 6379).await else {
            panic!("expected an error");
        };
        assert_eq!(error.kind(), ErrorKind::Io);
        assert!(error.to_string().contains("refused the forward"), "{error}");
    }
}
