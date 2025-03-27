use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub location: LocationSource,
    #[serde(skip, default)]
    pub data: Option<Vec<u8>>,
}

pub type RegoRuleName = String;
pub type RedisKeyName = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "location-type", rename_all = "kebab-case")]
pub enum LocationSource {
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

impl ToString for LocationSource {
    fn to_string(&self) -> String {
        match self {
            LocationSource::LocationPathFile(path) => {
                format!("url: {} rule_name: {}", path.url, path.rego_rule_name)
            }
            LocationSource::LocationPathRedis(path) => {
                format!(
                    "url: {}, rule_name: {}, redis_key: {}",
                    path.url, path.rego_rule_name, path.redis_key
                )
            }
            LocationSource::LocationPathHttp(path) => {
                format!("url: {}, rule_name: {}", path.url, path.rego_rule_name)
            }
        }
    }
}

impl LocationSource {
    pub fn new_file(url: impl AsRef<str>, rego_rule_name: impl AsRef<str>) -> Self {
        LocationSource::LocationPathFile(LocationPathFile {
            url: url.as_ref().to_string(),
            rego_rule_name: rego_rule_name.as_ref().to_string(),
        })
    }

    pub fn new_redis(
        url: impl AsRef<str>,
        redis_key: impl AsRef<str>,
        rego_rule_name: impl AsRef<str>,
    ) -> Self {
        LocationSource::LocationPathRedis(LocationPathRedis {
            url: url.as_ref().to_string(),
            redis_key: redis_key.as_ref().to_string(),
            rego_rule_name: rego_rule_name.as_ref().to_string(),
        })
    }

    pub fn new_http(url: impl AsRef<str>, rego_rule_name: impl AsRef<str>) -> Self {
        LocationSource::LocationPathHttp(LocationPathHttp {
            url: url.as_ref().to_string(),
            rego_rule_name: rego_rule_name.as_ref().to_string(),
        })
    }

    pub fn get_rego_rule_name(&self) -> &str {
        match self {
            LocationSource::LocationPathFile(location) => &location.rego_rule_name,
            LocationSource::LocationPathRedis(location) => &location.rego_rule_name,
            LocationSource::LocationPathHttp(location) => &location.rego_rule_name,
        }
    }

    /// Fetch the data from the location.
    pub async fn fetch_string(&self) -> Result<String, anyhow::Error> {
        match self {
            LocationSource::LocationPathFile(location) => {
                trace!("Fetching data from file path: {}", location.url);
                let data = tokio::fs::read_to_string(&location.url)
                    .await
                    .with_context(|| format!("unable to load data from path: {}", location.url))?;
                Ok(data)
            }
            LocationSource::LocationPathHttp(location) => {
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
            LocationSource::LocationPathRedis(_location) => {
                unimplemented!("Redis fetch is not implemented yet")
            }
        }
    }

    /// Fetch the data from the location as bytes.
    pub async fn fetch_bytes(&self) -> Result<Vec<u8>, anyhow::Error> {
        match self {
            LocationSource::LocationPathFile(location) => {
                trace!("Fetching data from file path: {}", location.url);
                let data = tokio::fs::read(&location.url)
                    .await
                    .with_context(|| format!("unable to load data from path: {}", location.url))?;
                Ok(data)
            }
            LocationSource::LocationPathHttp(location) => {
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
            // TODO
            LocationSource::LocationPathRedis(url) => {
                unimplemented!("Redis fetch is not implemented yet")
            }
        }
    }
}

impl Source {
    // Create a new location with the given path.
    pub fn new(location: LocationSource) -> Self {
        Source {
            location,
            data: None,
        }
    }

    // Fetch the data from the location.
    pub async fn fetch(&mut self) -> Result<(), anyhow::Error> {
        self.data = Some(self.location.fetch_bytes().await?);
        debug!("Fetched data from location: {:?}", self.data);
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
