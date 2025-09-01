use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::access_controller::{decision::Decision, predicates::Action};

/// The AccessReport is a struct that contains all information about the decision made by the access controller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessReport {
    pub access_decision: Decision,
    pub decision_reason: Option<String>,
    pub transaction_data: Value,
    pub rules: Vec<RuleReport>,
}

/// The RuleReport is a struct that contains all information about the rule that was evaluated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleReport {
    pub predicate_reports: Vec<PredicateReport>,
    pub is_matched: bool,
    pub applied_action: Option<Action>,
}

/// The PredicateReport is a struct that contains all information about the predicate that was evaluated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredicateReport {
    pub predicate_name: String,
    pub is_matched: bool,
    pub result_reason: String,
}

impl std::fmt::Display for PredicateReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:30} : {} : {}",
            self.predicate_name,
            if self.is_matched {
                "[MATCHED]"
            } else {
                "[NOT MATCHED]"
            },
            self.result_reason
        )
    }
}

impl PredicateReport {
    pub fn new(
        predicate_name: impl AsRef<str>,
        is_matched: bool,
        result_reason: impl AsRef<str>,
    ) -> Self {
        Self {
            predicate_name: predicate_name.as_ref().to_string(),
            is_matched,
            result_reason: result_reason.as_ref().to_string(),
        }
    }
}

impl RuleReport {
    pub fn new() -> Self {
        Self {
            predicate_reports: vec![],
            is_matched: false,
            applied_action: None,
        }
    }

    pub fn add_predicate_report(&mut self, predicate: PredicateReport) {
        self.predicate_reports.push(predicate);
        self.is_matched = self.predicate_reports.iter().all(|p| p.is_matched);
    }

    pub fn add_predicate_reports(&mut self, predicates: impl IntoIterator<Item = PredicateReport>) {
        self.predicate_reports.extend(predicates);
        self.is_matched = self.predicate_reports.iter().all(|p| p.is_matched);
    }

    pub fn set_final_action(&mut self, action: Option<Action>) {
        self.applied_action = action;
    }
}

impl std::fmt::Display for RuleReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let action_fmt = match &self.applied_action {
            Some(action) => match action {
                Action::Allow => "✅ Allow".to_string(),
                Action::Deny => "❌ Deny".to_string(),
                Action::HookAction(hook_action) => format!("🔗 {}", hook_action.url().to_string()),
            },
            None => "[No action applied]".to_string(),
        };

        write!(
            f,
            "\n\t{} \n\t---> Action applied: {}",
            self.predicate_reports
                .iter()
                .map(|p| p.to_string())
                .join("\n\t"),
            action_fmt,
        )
    }
}

impl AccessReport {
    pub fn new() -> Self {
        Self {
            access_decision: Decision::Allow,
            decision_reason: None,
            rules: vec![],
            transaction_data: Value::Null,
        }
    }

    pub fn add_rule(&mut self, rule: RuleReport) {
        self.rules.push(rule);
    }

    pub fn set_decision(&mut self, decision: Decision) {
        self.access_decision = decision;
    }
    pub fn set_decision_with_reason(&mut self, decision: Decision, reason: impl AsRef<str>) {
        self.access_decision = decision;
        self.decision_reason = Some(reason.as_ref().to_string());
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn set_transaction_data(&mut self, transaction_data: Value) {
        self.transaction_data = transaction_data;
    }
}

impl std::fmt::Display for AccessReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // rules should be enumerated as well
        let rules_fmt = self
            .rules
            .iter()
            .enumerate()
            .map(|(i, r)| format!("Rule {}: {}", i + 1, r.to_string()))
            .join("\n\n");

        let decision_fmt = match self.access_decision {
            Decision::Allow => {
                if let Some(reason) = &self.decision_reason {
                    format!("🟢 Allow ({reason})")
                } else {
                    "🟢 Allow".to_string()
                }
            }
            Decision::Deny => {
                if let Some(reason) = &self.decision_reason {
                    format!("🔴 Deny ({reason})")
                } else {
                    "🔴 Deny".to_string()
                }
            }
        };

        write!(
            f,
            "\n=================================\nAccess Report for transaction: \n{} \n{} \n{}",
            serde_json::to_string_pretty(&self.transaction_data).unwrap(),
            rules_fmt,
            decision_fmt,
        )
    }
}
