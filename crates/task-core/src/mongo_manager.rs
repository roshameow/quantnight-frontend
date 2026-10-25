use crate::config::AppConfig;
use anyhow::{anyhow, Result};
use mongodb::{options::ClientOptions, Client};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::OnceCell;

/// Desktop callers retain their original full-pool fields and constructor.
#[derive(Clone)]
pub struct MongoClients<T = Client> {
    pub local: Arc<T>,
    pub remote: Arc<T>,
}

impl MongoClients {
    pub async fn new(config: &AppConfig) -> Result<Self> {
        Self::new_with(config, |role, uri| async move {
            Self::build_client(&role, &uri).await.map_err(|error| error.to_string())
        }).await.map_err(anyhow::Error::msg)
    }

    /// The sole Rust connection factory, shared by desktop and lazy task pools.
    pub(crate) async fn build_client(role: &str, uri: &str) -> Result<Arc<Client>> {
        #[cfg(feature = "test-support")]
        assert!(
            !local_config_test_support::construction_forbidden(),
            "offline bootstrap attempted Mongo client construction"
        );
        tokio::time::timeout(Duration::from_secs(8), async {
            let options = ClientOptions::parse(uri)
                .await
                .map_err(|_| anyhow!("invalid {role} MongoDB configuration (details redacted)"))?;
            Client::with_options(options)
                .map(Arc::new)
                .map_err(|_| anyhow!("failed to create {role} MongoDB client (details redacted)"))
        })
        .await
        .map_err(|_| anyhow!("{role} MongoDB client initialization timed out; not retried"))?
    }

    pub(crate) fn scoped(config: &AppConfig) -> ScopedMongoClients {
        ClientSlots::from_config(config)
    }
}

impl<T> MongoClients<T> {
    /// Same full-pool role policy and construction sequence for production and
    /// offline lowest-factory tests. No alternate success bootstrap path.
    pub(crate) async fn new_with<F, Fut>(config: &AppConfig, mut build: F) -> Result<Self, String>
    where F: FnMut(String, String) -> Fut,
          Fut: Future<Output = Result<Arc<T>, String>> {
        let local = required_uri(&config.mongodb.local_uri, "local").map_err(|e| e.to_string())?;
        let remote = required_uri(&config.mongodb.remote_uri, "remote").map_err(|e| e.to_string())?;
        Ok(Self {
            local: build("local".into(), local.into()).await?,
            remote: build("remote".into(), remote.into()).await?,
        })
    }
}

fn required_uri<'a>(uri: &'a str, role: &str) -> Result<&'a str> {
    if uri.trim().is_empty() {
        return Err(anyhow!(
            "{role} MongoDB role is unconfigured; operation unavailable (no fallback)"
        ));
    }
    Ok(uri)
}

/// Only the requested configured role initializes. The result is retained by
/// this owner, including a failed attempt; unrelated proof refreshes cannot
/// replace clients or start an implicit retry loop.
struct ClientSlot<T> {
    uri: String,
    client: OnceCell<Result<Arc<T>, String>>,
}
impl<T> ClientSlot<T> {
    fn configured(uri: String) -> Self {
        Self {
            uri,
            client: OnceCell::new(),
        }
    }
    fn existing(client: Arc<T>) -> Self {
        Self {
            uri: String::new(),
            client: OnceCell::new_with(Some(Ok(client))),
        }
    }
    async fn get_with<F, Fut>(&self, role: &str, build: F) -> Result<&Arc<T>, String>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<Arc<T>, String>>,
    {
        if let Some(result) = self.client.get() {
            return result.as_ref().map_err(Clone::clone);
        }
        required_uri(&self.uri, role).map_err(|error| error.to_string())?;
        self.client
            .get_or_init(|| build(self.uri.clone()))
            .await
            .as_ref()
            .map_err(Clone::clone)
    }
}

/// Pool ownership stays here rather than introducing a TaskStore factory.
/// Generic slots permit offline bootstrap tests without constructing a Client.
pub(crate) struct ClientSlots<T> {
    local: ClientSlot<T>,
    remote: ClientSlot<T>,
}
pub(crate) type ScopedMongoClients = ClientSlots<Client>;
impl<T> ClientSlots<T> {
    fn from_config(config: &AppConfig) -> Self {
        Self {
            local: ClientSlot::configured(config.mongodb.local_uri.clone()),
            remote: ClientSlot::configured(config.mongodb.remote_uri.clone()),
        }
    }
    fn from_existing(local: Arc<T>, remote: Arc<T>) -> Self {
        Self {
            local: ClientSlot::existing(local),
            remote: ClientSlot::existing(remote),
        }
    }
    pub(crate) async fn client_with<F, Fut>(&self, remote: bool, build: F) -> Result<&Arc<T>, String>
    where
        F: FnOnce(String) -> Fut,
        Fut: Future<Output = Result<Arc<T>, String>>,
    {
        if remote {
            self.remote.get_with("remote", build).await
        } else {
            self.local.get_with("local", build).await
        }
    }
}
impl<T> ClientSlots<T> {
    pub(crate) fn existing(clients: &MongoClients<T>) -> Self {
        Self::from_existing(clients.local.clone(), clients.remote.clone())
    }
}
impl ScopedMongoClients {
    pub(crate) async fn client(&self, remote: bool) -> Result<&Arc<Client>, String> {
        self.client_with(remote, |uri| async move {
            MongoClients::build_client(if remote { "remote" } else { "local" }, &uri)
                .await
                .map_err(|error| error.to_string())
        })
        .await
    }
}

#[cfg(feature = "test-support")]
pub mod local_config_test_support {
    use super::*;
    tokio::task_local! { static FORBID_CONSTRUCTION: (); }
    pub(super) fn construction_forbidden() -> bool {
        FORBID_CONSTRUCTION.try_with(|_| ()).is_ok()
    }
    /// A scoped sentinel at the sole factory, before URI parsing or client IO.
    pub async fn without_client_construction<F: Future>(future: F) -> F::Output {
        FORBID_CONSTRUCTION.scope((), future).await
    }
    /// Lowest-layer fake construction seam; executes the production slot
    /// selection, absence guard, OnceCell and injected-pool reuse unchanged.
    pub struct FakeClients<T>(ClientSlots<T>);
    impl<T> FakeClients<T> {
        pub fn from_config(config: &AppConfig) -> Self {
            Self(ClientSlots::from_config(config))
        }
        pub fn from_existing(local: Arc<T>, remote: Arc<T>) -> Self {
            Self(ClientSlots::from_existing(local, remote))
        }
        pub async fn client_with<F, Fut>(&self, remote: bool, build: F) -> Result<&Arc<T>, String>
        where
            F: FnOnce(String) -> Fut,
            Fut: Future<Output = Result<Arc<T>, String>>,
        {
            self.0.client_with(remote, build).await
        }
    }
}
