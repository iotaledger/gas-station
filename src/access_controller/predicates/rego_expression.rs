use anyhow::{bail, Context};
use regorus::Value;

use crate::access_controller::location::{LocationSource, Source};

/// RegoExpression allows to evaluate Rego policies
/// using the regorus engine.
pub struct RegoExpression {
    pub source: Source,
    pub engine: regorus::Engine,
}

impl RegoExpression {
    /// Create a new RegoExpression from the Source
    pub fn try_from_source(source: Source) -> Result<Self, anyhow::Error> {
        let mut engine = regorus::Engine::new();
        let source_data = source
            .get_data_string()
            .with_context(|| format!("Source data is empty for {}", source.location.to_string()))?;
        engine
            .add_policy(source.location.to_string(), source_data)
            .with_context(|| {
                format!("error while loading policy {}", source.location.to_string())
            })?;

        Ok(RegoExpression { source, engine })
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
        let mut engine = regorus::Engine::new();
        engine
            .add_policy(self.source.location.to_string(), source_data)
            .with_context(|| {
                format!(
                    "error while loading policy {}",
                    self.source.location.to_string()
                )
            })?;
        self.engine = engine;
        Ok(())
    }

    /// Evaluate the policy with the given input data.
    pub fn matches(&mut self, input_data: &str) -> Result<bool, anyhow::Error> {
        let rego_rule_name = self.source.location.get_rego_rule_name().to_string();
        let mut engine = self.engine.clone();
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

#[cfg(test)]
mod test {
    use std::result;

    use super::*;

    #[tokio::test]
    async fn test_rego_expression_smoke_test() {
        let rego_rule_name = "data.test.allow";
        let source_location = LocationSource::new_file(
            "/home/rayven/code/iota/iota-gas-station/test.rego",
            rego_rule_name,
        );
        let mut source = Source::new(source_location);
        source.fetch().await.unwrap();

        let mut rego_expression = RegoExpression::try_from_source(source).unwrap();

        let matched_input_data = r#"{"method": "GET", "path": "data"}"#;
        assert!(rego_expression.matches(matched_input_data).unwrap());

        let unmatched_input_data = r#"{"method": "POST", "path": "data"}"#;
        assert!(!rego_expression.matches(unmatched_input_data).unwrap())
    }
}
