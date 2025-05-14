use anyhow::{bail, Context};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use tracing::trace;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceWithData {
    pub location: SourceLocation,
    #[serde(skip, default)]
    pub data: Option<Vec<u8>>,
}

impl SourceWithData {
    // Create a new location with the given path.
    pub fn new(location: SourceLocation) -> Self {
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
pub enum SourceLocation {
    #[serde(rename = "file")]
    LocationPathFile(LocationPathFile),
    #[serde(rename = "redis")]
    LocationPathRedis(LocationPathRedis),
    #[serde(rename = "http")]
    LocationPathHttp(LocationPathHttp),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LocationPathFile {
    url: String,
    rego_rule_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LocationPathRedis {
    url: String,
    redis_key: String,
    rego_rule_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct LocationPathHttp {
    url: String,
    rego_rule_name: String,
}

impl ToString for SourceLocation {
    fn to_string(&self) -> String {
        match self {
            SourceLocation::LocationPathFile(path) => {
                format!("url: {} rule_name: {}", path.url, path.rego_rule_name)
            }
            SourceLocation::LocationPathRedis(path) => {
                format!(
                    "url: {}, rule_name: {}, redis_key: {}",
                    path.url, path.rego_rule_name, path.redis_key
                )
            }
            SourceLocation::LocationPathHttp(path) => {
                format!("url: {}, rule_name: {}", path.url, path.rego_rule_name)
            }
        }
    }
}

impl SourceLocation {
    pub fn new_file(url: impl AsRef<str>, rego_rule_name: impl AsRef<str>) -> Self {
        SourceLocation::LocationPathFile(LocationPathFile {
            url: url.as_ref().to_string(),
            rego_rule_name: rego_rule_name.as_ref().to_string(),
        })
    }

    pub fn new_redis(
        url: impl AsRef<str>,
        redis_key: impl AsRef<str>,
        rego_rule_name: impl AsRef<str>,
    ) -> Self {
        SourceLocation::LocationPathRedis(LocationPathRedis {
            url: url.as_ref().to_string(),
            redis_key: redis_key.as_ref().to_string(),
            rego_rule_name: rego_rule_name.as_ref().to_string(),
        })
    }

    pub fn new_http(url: impl AsRef<str>, rego_rule_name: impl AsRef<str>) -> Self {
        SourceLocation::LocationPathHttp(LocationPathHttp {
            url: url.as_ref().to_string(),
            rego_rule_name: rego_rule_name.as_ref().to_string(),
        })
    }

    pub fn get_rego_rule_name(&self) -> &str {
        match self {
            SourceLocation::LocationPathFile(location) => &location.rego_rule_name,
            SourceLocation::LocationPathRedis(location) => &location.rego_rule_name,
            SourceLocation::LocationPathHttp(location) => &location.rego_rule_name,
        }
    }

    /// Fetch the data from the location.
    pub async fn fetch_string(&self) -> Result<String, anyhow::Error> {
        match self {
            SourceLocation::LocationPathFile(location) => {
                trace!("Fetching data from file path: {}", location.url);
                let data = tokio::fs::read_to_string(&location.url)
                    .await
                    .with_context(|| format!("unable to load data from path: {}", location.url))?;
                Ok(data)
            }
            SourceLocation::LocationPathHttp(location) => {
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
            SourceLocation::LocationPathRedis(location) => {
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
        }
    }

    /// Fetch the data from the location as bytes.
    pub async fn fetch_bytes(&self) -> Result<Vec<u8>, anyhow::Error> {
        match self {
            SourceLocation::LocationPathFile(location) => {
                trace!("Fetching data from file path: {}", location.url);
                let data = tokio::fs::read(&location.url)
                    .await
                    .with_context(|| format!("unable to load data from path: {}", location.url))?;
                Ok(data)
            }
            SourceLocation::LocationPathHttp(location) => {
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
            SourceLocation::LocationPathRedis(url) => {
                let client = redis::Client::open(url.url.clone())
                    .with_context(|| format!("unable to connect to redis server: {}", url.url))?;
                let mut con = client.get_async_connection().await?;
                let data: Vec<u8> = con.get(url.redis_key.clone()).await.with_context(|| {
                    format!("unable to get data from redis key: {}", url.redis_key)
                })?;
                Ok(data)
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
        let file_location = SourceLocation::new_file(TEST_REGO_FILE_PATH, TEST_REGO_RULE_NAME);
        let redis_location = SourceLocation::new_redis(
            TEST_REGO_REDIS_URL,
            TEST_REGO_REDIS_KEY,
            TEST_REGO_RULE_NAME,
        );
        let http_location = SourceLocation::new_http(TEST_REGO_HTTP_URL, TEST_REGO_RULE_NAME);

        assert_eq!(file_location.get_rego_rule_name(), TEST_REGO_RULE_NAME);
        assert_eq!(redis_location.get_rego_rule_name(), TEST_REGO_RULE_NAME);
        assert_eq!(http_location.get_rego_rule_name(), TEST_REGO_RULE_NAME);
    }

    #[tokio::test]
    async fn test_source_location_file() {
        let absolute_path = std::env::current_dir()
            .unwrap()
            .join(TEST_REGO_FILE_PATH)
            .to_str()
            .unwrap()
            .to_string();
        let file_location = SourceLocation::new_file(absolute_path, TEST_REGO_RULE_NAME);
        let data = file_location.fetch_string().await.unwrap();
        assert_eq!(data, TEST_REGO_CONTENT);
    }

    #[tokio::test]
    async fn test_source_location_http() {
        setup_http_server().await.unwrap();
        let http_location = SourceLocation::new_http(TEST_REGO_HTTP_URL, TEST_REGO_RULE_NAME);
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

        let redis_location = SourceLocation::new_redis(
            TEST_REGO_REDIS_URL,
            TEST_REGO_REDIS_KEY,
            TEST_REGO_RULE_NAME,
        );
        let data = redis_location.fetch_string().await.unwrap();
        assert_eq!(data, TEST_REGO_CONTENT);
    }
}
