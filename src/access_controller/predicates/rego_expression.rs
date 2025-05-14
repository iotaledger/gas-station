// Copyright (c) 2024 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use anyhow::{bail, Context};
use regorus::Value;
use serde::{Deserialize, Serialize};
use tracing::trace;

use super::source::{Location, SourceWithData};

/// RegoExpression allows to evaluate Rego policies
/// using the regorus engine.
#[derive(Debug, Clone)]
pub struct RegoExpression {
    pub source: SourceWithData,
    pub expression: Option<regorus::Engine>,
}

impl RegoExpression {
    /// Create a new RegoExpression from the given source. Please make sure the source
    /// is already fetched and contains the data. If the source is not fetched, the `reload_source()` method
    /// should be called to fetch the data.
    pub fn from_source(source: SourceWithData) -> Result<Self, anyhow::Error> {
        let expression = if let Some(data) = source.get_data_string() {
            let mut expression = regorus::Engine::new();
            expression
                .add_policy(source.location.to_string(), data)
                .with_context(|| format!("failed to add policy {}", source.location.to_string()))?;
            Some(expression)
        } else {
            trace!(
                "Source data is empty for {}. Use 'reload_source()' to initialize the expression",
                source.location.to_string()
            );
            None
        };
        Ok(RegoExpression { source, expression })
    }

    /// Reload the policy from the source.
    pub async fn reload_source(&mut self) -> Result<(), anyhow::Error> {
        self.source.fetch().await?;
        let source_data = self.source.get_data_string().with_context(|| {
            format!(
                "Source data is empty for {}",
                self.source.location.to_string()
            )
        })?;
        let mut expression = regorus::Engine::new();
        expression
            .add_policy(self.source.location.to_string(), source_data)
            .with_context(|| {
                format!(
                    "error while loading policy {}",
                    self.source.location.to_string()
                )
            })?;
        self.expression = Some(expression);
        Ok(())
    }

    /// Evaluate the policy with the given input data.
    pub fn matches(&self, input_data: &str) -> Result<bool, anyhow::Error> {
        let rego_rule_name = self.source.location.get_rego_rule_name().to_string();
        if self.expression.is_none() {
            bail!("Rego expression is not initialized");
        }
        let mut engine = self.expression.clone().unwrap();
        let value = Value::from_json_str(input_data)
            .with_context(|| format!("error while converting input data to json {}", input_data))?;
        engine.set_input(value);

        let result = engine
            .eval_rule(rego_rule_name)
            .with_context(|| format!("error while evaluating rule"))?;

        if let Value::Bool(result) = result {
            Ok(result)
        } else {
            bail!("error while evaluating rule, result is not a bool")
        }
    }
}

impl Serialize for RegoExpression {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.source
            .location
            .serialize(serializer)
            .map_err(serde::ser::Error::custom)
    }
}

impl<'de> Deserialize<'de> for RegoExpression {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let location = Location::deserialize(deserializer)?;
        let source_with_data = SourceWithData::new(location);
        RegoExpression::from_source(source_with_data).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    const TEST_REGO_FILE_CONTENT: &str = include_str!("./test_files/sample_expression.rego");
    const TEST_REGO_RULE_NAME: &str = "data.test.some_match";

    #[tokio::test]
    async fn test_rego_expression_matching() {
        let location = Location::new_memory(TEST_REGO_FILE_CONTENT, TEST_REGO_RULE_NAME);
        let mut source = SourceWithData::new(location);
        source.fetch().await.unwrap();
        let rego_expression = RegoExpression::from_source(source).unwrap();

        let matched_input = r#"{"method": "GET"}"#;
        let result = rego_expression.matches(matched_input).unwrap();
        assert_eq!(result, true);

        let unmatched_input = r#"{"method": "POST"}"#;
        let result = rego_expression.matches(unmatched_input).unwrap();
        assert_eq!(result, false);
    }

    #[tokio::test]
    async fn test_rego_expression_source_reload() {
        let location = Location::new_memory(TEST_REGO_FILE_CONTENT, TEST_REGO_RULE_NAME);
        let source = SourceWithData::new(location);
        let mut rego_expression = RegoExpression::from_source(source).unwrap();

        let input_data = r#"{"method": "GET"}"#;
        let result = rego_expression.matches(input_data);
        assert!(result.is_err());

        rego_expression
            .reload_source()
            .await
            .expect("Failed to reload source");
        let result = rego_expression.matches(input_data).unwrap();
        assert_eq!(result, true);
    }

    #[tokio::test]
    async fn test_rego_expression_invalid_data_rego_file() {
        let invalid_rego_file = r#"######'####}"#;
        let location = Location::new_memory(invalid_rego_file, TEST_REGO_RULE_NAME);
        let mut source = SourceWithData::new(location);
        source.fetch().await.unwrap();

        let result = RegoExpression::from_source(source);
        assert!(result.is_err());
    }
}
