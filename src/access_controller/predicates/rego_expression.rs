use anyhow::{bail, Context};
use regorus::Value;
use serde::{Deserialize, Serialize};
use tracing::trace;

use super::location::{SourceLocation, SourceWithData};

/// RegoExpression allows to evaluate Rego policies
/// using the regorus engine.
#[derive(Debug, Clone)]
pub struct RegoExpression {
    pub source: SourceWithData,
    // probably the expression should be optional
    pub expression: Option<regorus::Engine>,
}

impl RegoExpression {
    pub fn try_from_source(source: SourceWithData) -> Result<Self, anyhow::Error> {
        let expression = if let Some(data) = source.get_data_string() {
            let mut expression = regorus::Engine::new();
            expression
                .add_policy(source.location.to_string(), data)
                .with_context(|| format!("failed to add policy {}", source.location.to_string()))?;
            Some(expression)
        } else {
            trace!("Source data is empty for {}. Please make use 'reload()' to initialize the expression", source.location.to_string());
            None
        };
        Ok(RegoExpression { source, expression })
    }

    /// Reload the policy from the source.
    pub async fn reload(&mut self) -> Result<(), anyhow::Error> {
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
        let location = SourceLocation::deserialize(deserializer)?;
        let source_with_data = SourceWithData::new(location);
        RegoExpression::try_from_source(source_with_data).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_rego_expression_smoke_test() {
        let rego_rule_name = "data.test.allow";
        let source_location = SourceLocation::new_file(
            "/home/rayven/code/iota/iota-gas-station/test.rego",
            rego_rule_name,
        );
        let mut source = SourceWithData::new(source_location);
        source.fetch().await.unwrap();

        let mut rego_expression = RegoExpression::try_from_source(source).unwrap();

        let matched_input_data = r#"{"method": "GET", "path": "data"}"#;
        assert!(rego_expression.matches(matched_input_data).unwrap());

        let unmatched_input_data = r#"{"method": "POST", "path": "data"}"#;
        assert!(!rego_expression.matches(unmatched_input_data).unwrap())
    }
}
