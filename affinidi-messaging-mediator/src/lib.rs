#![feature(async_closure)]

use std::fmt::Debug;

use affinidi_did_resolver_cache_sdk::DIDCacheClient;
use axum::{
    async_trait,
    extract::{FromRef, FromRequestParts},
};
use chrono::{DateTime, Utc};
use common::{
    config::{read_config_file, Config, ConfigRaw},
    errors::MediatorError,
    jwt_auth::AuthError,
};
use database::DatabaseHandler;
use http::request::Parts;
use tasks::websocket_streaming::StreamingTask;
use tracing::{event, Level};
use tracing_subscriber::{reload::Handle, EnvFilter, Registry};

pub mod common;
pub mod database;
pub mod handlers;
pub mod messages;
pub mod resolvers;
pub mod server;
pub mod tasks;

#[derive(Clone)]
pub struct SharedData {
    pub config: Config,
    pub service_start_timestamp: DateTime<Utc>,
    pub did_resolver: DIDCacheClient,
    pub database: DatabaseHandler,
    pub streaming_task: Option<StreamingTask>,
}

impl Debug for SharedData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedData")
            .field("config", &self.config)
            .field("service_start_timestamp", &self.service_start_timestamp)
            .finish()
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for SharedData
where
    Self: FromRef<S>,
    S: Send + Sync + Debug,
{
    type Rejection = AuthError;

    async fn from_request_parts(_parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self::from_ref(state)) // <---- added this line
    }
}

pub async fn init(
    reload_handle: Option<Handle<EnvFilter, Registry>>,
) -> Result<Config, MediatorError> {
    // Read configuration file parameters
    let config = read_config_file("conf/mediator.toml")?;

    // Setup logging
    if reload_handle.is_some() {
        let level: EnvFilter = EnvFilter::new(config.log_level.as_str());
        reload_handle
            .unwrap()
            .modify(|filter| *filter = level)
            .map_err(|e| MediatorError::InternalError("NA".into(), e.to_string()))?;
        event!(Level::INFO, "Log level set to ({})", config.log_level);
        event!(Level::DEBUG, "Log level set to ({})", config.log_level);
    }

    match <common::config::Config as async_convert::TryFrom<ConfigRaw>>::try_from(config).await {
        Ok(config) => {
            event!(
                Level::INFO,
                "Configuration settings parsed successfully.\n{:#?}",
                config
            );
            Ok(config)
        }
        Err(err) => Err(err),
    }
}
