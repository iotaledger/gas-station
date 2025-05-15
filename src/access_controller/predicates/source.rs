// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use anyhow::{bail, Context};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use tracing::trace;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceWithData {
    pub location: Location,
    #[serde(skip, default)]
    pub data: Option<Vec<u8>>,
}

impl SourceWithData {
    // Create a new location with the given path.
    pub fn new(location: Location) -> Self {
        SourceWithData {
            location,
            data: None,
        }
    }
    // Fetch the data from the location.
    pub async fn fetch(&mut self) -> Result<(), anyhow::Error> {
        self.data = Some(self.location.fetch_bytes().await?);
        trace!("Fetched data from location: {:?}", self.data);
        Ok(())
    }

    pub fn get_data(&self) -> Option<&[u8]> {
        self.data.as_deref()
    }

    pub fn get_data_string(&self) -> Option<String> {
        self.data
            .as_ref()
            .map(|data| String::from_utf8_lossy(data).to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "location-type", rename_all = "kebab-case")]
pub enum Location {
    #[serde(rename = "file")]
    LocationPathFile(LocationPathFile),
    #[serde(rename = "redis")]
    LocationPathRedis(LocationPathRedis),
    #[serde(rename = "http")]
    LocationPathHttp(LocationPathHttp),
    #[cfg(test)]
    #[serde(rename = "memory")]
    LocationPathMemory(LocationPathMemory),
}

#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LocationPathMemory {
    pub data: String,
    pub rego_rule_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LocationPathFile {
    url: String,
    rego_rule_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LocationPathRedis {
    url: String,
    redis_key: String,
    rego_rule_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LocationPathHttp {
    url: String,
    rego_rule_path: String,
}

impl ToString for Location {
    fn to_string(&self) -> String {
        match self {
            Location::LocationPathFile(path) => {
                format!("url: {} rule_path: {}", path.url, path.rego_rule_path)
            }
            Location::LocationPathRedis(path) => {
                format!(
                    "url: {}, rule_path: {}, redis_key: {}",
                    path.url, path.rego_rule_path, path.redis_key
                )
            }
            Location::LocationPathHttp(path) => {
                format!("url: {}, rule_path: {}", path.url, path.rego_rule_path)
            }
            #[cfg(test)]
            Location::LocationPathMemory(path) => {
                format!("data: {}", path.data)
            }
        }
    }
}

impl Location {
    /// Create a new location with the given file path.
    pub fn new_file(url: impl AsRef<str>, rego_rule_name: impl AsRef<str>) -> Self {
        Location::LocationPathFile(LocationPathFile {
            url: url.as_ref().to_string(),
            rego_rule_path: rego_rule_name.as_ref().to_string(),
        })
    }

    /// Create a new location with the given redis url and key.
    pub fn new_redis(
        url: impl AsRef<str>,
        redis_key: impl AsRef<str>,
        rego_rule_name: impl AsRef<str>,
    ) -> Self {
        Location::LocationPathRedis(LocationPathRedis {
            url: url.as_ref().to_string(),
            redis_key: redis_key.as_ref().to_string(),
            rego_rule_path: rego_rule_name.as_ref().to_string(),
        })
    }

    /// Create a new location with the given http url.
    pub fn new_http(url: impl AsRef<str>, rego_rule_name: impl AsRef<str>) -> Self {
        Location::LocationPathHttp(LocationPathHttp {
            url: url.as_ref().to_string(),
            rego_rule_path: rego_rule_name.as_ref().to_string(),
        })
    }

    #[cfg(test)]
    pub fn new_memory(data: impl AsRef<str>, rego_rule_name: impl AsRef<str>) -> Self {
        Location::LocationPathMemory(LocationPathMemory {
            data: data.as_ref().to_string(),
            rego_rule_path: rego_rule_name.as_ref().to_string(),
        })
    }

    /// Get the rego rule path from the location.
    pub fn get_rego_rule_path(&self) -> &str {
        match self {
            Location::LocationPathFile(location) => &location.rego_rule_path,
            Location::LocationPathRedis(location) => &location.rego_rule_path,
            Location::LocationPathHttp(location) => &location.rego_rule_path,
            #[cfg(test)]
            Location::LocationPathMemory(location) => &location.rego_rule_path,
        }
    }

    /// Fetch the data from the location.
    pub async fn fetch_string(&self) -> Result<String, anyhow::Error> {
        match self {
            Location::LocationPathFile(location) => {
                trace!("Fetching data from file path: {}", location.url);
                let data = tokio::fs::read_to_string(&location.url)
                    .await
                    .with_context(|| format!("unable to load data from path: {}", location.url))?;
                Ok(data)
            }
            Location::LocationPathHttp(location) => {
                trace!("Fetching data from http url: {}", location.url);
                let response = reqwest::get(location.url.clone())
                    .await
                    .with_context(|| format!("unable to load data from url: {}", location.url))?;
                if !response.status().is_success() {
                    bail!(
                        "Error while getting the data from url {} client error: {}",
                        location.url,
                        response.status()
                    );
                }
                let data = response.text().await?;
                Ok(data)
            }
            Location::LocationPathRedis(location) => {
                trace!("Fetching data from redis url: {}", location.url);
                let client = redis::Client::open(location.url.clone()).with_context(|| {
                    format!("unable to connect to redis server: {}", location.url)
                })?;
                let mut con = client.get_async_connection().await?;
                let data: String =
                    con.get(location.redis_key.clone()).await.with_context(|| {
                        format!("unable to get data from redis key: {}", location.redis_key)
                    })?;
                Ok(data)
            }
            #[cfg(test)]
            Location::LocationPathMemory(location) => {
                trace!("Fetching data from memory: {}", location.data);
                Ok(location.data.clone())
            }
        }
    }

    /// Fetch the data from the location as bytes.
    pub async fn fetch_bytes(&self) -> Result<Vec<u8>, anyhow::Error> {
        match self {
            Location::LocationPathFile(location) => {
                trace!("Fetching data from file path: {}", location.url);
                let data = tokio::fs::read(&location.url)
                    .await
                    .with_context(|| format!("unable to load data from path: {}", location.url))?;
                Ok(data)
            }
            Location::LocationPathHttp(location) => {
                trace!("Fetching data from http url: {}", location.url);
                let response = reqwest::get(location.url.clone())
                    .await
                    .with_context(|| format!("unable to load data from url: {}", location.url))?;
                if !response.status().is_success() {
                    bail!(
                        "Error while getting the data from url {} client error: {}",
                        location.url,
                        response.status()
                    );
                }
                let data = response.bytes().await?;
                Ok(data.to_vec())
            }
            Location::LocationPathRedis(url) => {
                let client = redis::Client::open(url.url.clone())
                    .with_context(|| format!("unable to connect to redis server: {}", url.url))?;
                let mut con = client.get_async_connection().await?;
                let data: Vec<u8> = con.get(url.redis_key.clone()).await.with_context(|| {
                    format!("unable to get data from redis key: {}", url.redis_key)
                })?;
                Ok(data)
            }
            #[cfg(test)]
            Location::LocationPathMemory(location) => {
                trace!("Fetching data from memory: {}", location.data);
                Ok(location.data.as_bytes().to_vec())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::{routing::get, Router};

    use super::*;

    const TEST_REGO_RULE_NAME: &str = "data.test.some_match";
    const TEST_REGO_FILE_PATH: &str =
        "./src/access_controller/predicates/test_files/sample_expression.rego";
    const TEST_REGO_REDIS_KEY: &str = "test_key";
    const TEST_REGO_HTTP_URL: &str = "http://localhost:8080/sample_expression.rego";
    const TEST_REGO_CONTENT: &str = include_str!("./test_files/sample_expression.rego");
    const TEST_REGO_REDIS_URL: &str = "redis://localhost:6379";

    async fn setup_http_server() -> Result<(), anyhow::Error> {
        let app = Router::new().route(
            "/sample_expression.rego",
            get(|| async { TEST_REGO_CONTENT }),
        );
        let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
        let server = axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .with_graceful_shutdown(async {
                tokio::signal::ctrl_c()
                    .await
                    .expect("Failed to install Ctrl+C signal handler");
            });
        tokio::spawn(async move {
            if let Err(e) = server.await {
                eprintln!("Server error: {}", e);
            }
        });

        Ok(())
    }

    #[tokio::test]
    async fn test_source_location() {
        let file_location = Location::new_file(TEST_REGO_FILE_PATH, TEST_REGO_RULE_NAME);
        let redis_location = Location::new_redis(
            TEST_REGO_REDIS_URL,
            TEST_REGO_REDIS_KEY,
            TEST_REGO_RULE_NAME,
        );
        let http_location = Location::new_http(TEST_REGO_HTTP_URL, TEST_REGO_RULE_NAME);

        assert_eq!(file_location.get_rego_rule_path(), TEST_REGO_RULE_NAME);
        assert_eq!(redis_location.get_rego_rule_path(), TEST_REGO_RULE_NAME);
        assert_eq!(http_location.get_rego_rule_path(), TEST_REGO_RULE_NAME);
    }

    #[tokio::test]
    async fn test_source_location_file() {
        let absolute_path = std::env::current_dir()
            .unwrap()
            .join(TEST_REGO_FILE_PATH)
            .to_str()
            .unwrap()
            .to_string();
        let file_location = Location::new_file(absolute_path, TEST_REGO_RULE_NAME);
        let data = file_location.fetch_string().await.unwrap();
        assert_eq!(data, TEST_REGO_CONTENT);
    }

    #[tokio::test]
    async fn test_source_location_http() {
        setup_http_server().await.unwrap();
        let http_location = Location::new_http(TEST_REGO_HTTP_URL, TEST_REGO_RULE_NAME);
        let data = http_location.fetch_string().await.unwrap();
        assert_eq!(data, TEST_REGO_CONTENT);
    }

    #[tokio::test]
    async fn test_source_location_redis() {
        let redis_client = redis::Client::open(TEST_REGO_REDIS_URL).unwrap();
        let mut con = redis_client.get_async_connection().await.unwrap();
        con.set::<_, _, ()>(
            TEST_REGO_REDIS_KEY.to_string(),
            TEST_REGO_CONTENT.to_string(),
        )
        .await
        .unwrap();

        let redis_location = Location::new_redis(
            TEST_REGO_REDIS_URL,
            TEST_REGO_REDIS_KEY,
            TEST_REGO_RULE_NAME,
        );
        let data = redis_location.fetch_string().await.unwrap();
        assert_eq!(data, TEST_REGO_CONTENT);
    }

    #[tokio::test]
    async fn test_source_with_data() {
        let file_location = Location::new_file(TEST_REGO_FILE_PATH, TEST_REGO_RULE_NAME);
        let mut source_with_data = SourceWithData::new(file_location);
        source_with_data.fetch().await.unwrap();
        assert_eq!(
            source_with_data.get_data_string().unwrap(),
            TEST_REGO_CONTENT
        );
    }
}
