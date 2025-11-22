use mongodb::{Client, options::ClientOptions};
use std::sync::Arc;
use anyhow::Result;
use log::error; // 加这一行
use crate::config::AppConfig;

#[derive(Clone)]
pub struct MongoClients {
    pub local: Arc<Client>,
    pub remote: Arc<Client>,
}

impl MongoClients {
    pub async fn new(config: &AppConfig) -> Result<Self> {
        println!("⚠️ MongoClients::new() called");

        let local_uri = &config.mongodb.local_uri;
        let remote_uri = &config.mongodb.remote_uri;

        // 本地连接选项
        let local_options = match ClientOptions::parse(local_uri).await {
            Ok(opt) => opt,
            Err(e) => {
                error!("Failed to parse local MongoDB URI: {}", e);
                return Err(e.into());
            }
        };

        // 远程连接选项
        let remote_options = match ClientOptions::parse(remote_uri).await {
            Ok(opt) => opt,
            Err(e) => {
                error!("Failed to parse remote MongoDB URI: {}", e);
                return Err(e.into());
            }
        };

        // 创建本地 client
        let local_client = match Client::with_options(local_options) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                error!("Failed to create local MongoDB client: {}", e);
                return Err(e.into());
            }
        };

        // 创建远程 client
        let remote_client = match Client::with_options(remote_options) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                error!("Failed to create remote MongoDB client: {}", e);
                return Err(e.into());
            }
        };

        Ok(MongoClients {
            local: local_client,
            remote: remote_client,
        })
    }
}
