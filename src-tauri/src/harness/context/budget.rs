use crate::harness::config::HarnessSettings;
use crate::harness::context::ContextSpec;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BudgetError {
    #[error("invalid context-budget setting `{field}`: {reason}")]
    InvalidSetting {
        field: &'static str,
        reason: &'static str,
    },
    #[error(
        "model context is too small: {context_tokens} tokens cannot hold safety reserve {safety_tokens} plus minimum final answer {minimum_completion_tokens}"
    )]
    InsufficientContext {
        context_tokens: u32,
        safety_tokens: u32,
        minimum_completion_tokens: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnBudget {
    pub context: ContextSpec,
    pub reserved_completion_tokens: u32,
    pub safety_reserve_tokens: u32,
    pub prompt_budget_tokens: u32,
    pub tool_result_budget_tokens: u32,
    pub per_tool_result_max_tokens: u32,
    pub summary_max_tokens: u32,
    pub max_tool_steps: u32,
    pub max_same_tool_calls: u32,
}

impl TurnBudget {
    /// Build a complete turn budget before dispatching the model. Requested
    /// completion is clamped to provider output metadata and cannot reduce the
    /// configured final-answer reserve.
    pub fn from_settings(
        context: ContextSpec,
        settings: &HarnessSettings,
        requested_completion_tokens: Option<u32>,
    ) -> Result<Self, BudgetError> {
        validate_settings(settings)?;

        let context_tokens = context.context_window_tokens;
        let fractional_safety =
            (f64::from(context_tokens) * settings.context_safety_fraction).ceil() as u32;
        let safety_reserve_tokens = fractional_safety.max(settings.context_safety_min_tokens);
        let available_after_safety = context_tokens.checked_sub(safety_reserve_tokens).ok_or(
            BudgetError::InsufficientContext {
                context_tokens,
                safety_tokens: safety_reserve_tokens,
                minimum_completion_tokens: settings.minimum_final_answer_tokens,
            },
        )?;

        if available_after_safety <= settings.minimum_final_answer_tokens {
            return Err(BudgetError::InsufficientContext {
                context_tokens,
                safety_tokens: safety_reserve_tokens,
                minimum_completion_tokens: settings.minimum_final_answer_tokens,
            });
        }

        let provider_completion_limit = context
            .output_limit_tokens
            .unwrap_or(available_after_safety)
            .min(available_after_safety);
        if provider_completion_limit < settings.minimum_final_answer_tokens {
            return Err(BudgetError::InsufficientContext {
                context_tokens,
                safety_tokens: safety_reserve_tokens,
                minimum_completion_tokens: settings.minimum_final_answer_tokens,
            });
        }

        let requested_completion_tokens =
            requested_completion_tokens.unwrap_or(settings.max_tokens);
        let reserved_completion_tokens = requested_completion_tokens
            .max(settings.minimum_final_answer_tokens)
            .min(provider_completion_limit);
        let prompt_budget_tokens = available_after_safety - reserved_completion_tokens;
        let tool_result_budget_tokens =
            (f64::from(prompt_budget_tokens) * settings.tool_context_fraction).floor() as u32;

        Ok(Self {
            context,
            reserved_completion_tokens,
            safety_reserve_tokens,
            prompt_budget_tokens,
            tool_result_budget_tokens,
            per_tool_result_max_tokens: settings
                .tool_result_max_tokens
                .min(tool_result_budget_tokens),
            summary_max_tokens: settings.summary_max_tokens.min(prompt_budget_tokens),
            max_tool_steps: settings.max_tool_steps,
            max_same_tool_calls: settings.max_same_tool_calls,
        })
    }

    /// Admit one prospective tool dispatch against both the whole prompt and
    /// the tool-only sub-budget. The caller supplies the result ceiling it will
    /// enforce; oversized or arithmetically invalid requests are rejected.
    pub fn admit_dispatch(
        &self,
        usage: DispatchUsage,
        prospective_result_tokens: u32,
    ) -> Result<DispatchAdmission, DispatchRejection> {
        if prospective_result_tokens == 0 {
            return Err(DispatchRejection::EmptyResultAllowance);
        }
        if prospective_result_tokens > self.per_tool_result_max_tokens {
            return Err(DispatchRejection::PerResultLimitExceeded {
                requested_tokens: prospective_result_tokens,
                limit_tokens: self.per_tool_result_max_tokens,
            });
        }

        let projected_prompt_tokens = usage
            .prompt_tokens
            .checked_add(prospective_result_tokens)
            .ok_or(DispatchRejection::ArithmeticOverflow {
                field: "promptTokens",
            })?;
        let projected_tool_result_tokens = usage
            .tool_result_tokens
            .checked_add(prospective_result_tokens)
            .ok_or(DispatchRejection::ArithmeticOverflow {
                field: "toolResultTokens",
            })?;
        let projected_tool_steps = usage
            .tool_steps
            .checked_add(1)
            .ok_or(DispatchRejection::ArithmeticOverflow { field: "toolSteps" })?;
        let projected_same_tool_calls =
            usage
                .same_tool_calls
                .checked_add(1)
                .ok_or(DispatchRejection::ArithmeticOverflow {
                    field: "sameToolCalls",
                })?;

        if projected_prompt_tokens > self.prompt_budget_tokens {
            return Err(DispatchRejection::PromptBudgetExceeded {
                projected_tokens: projected_prompt_tokens,
                limit_tokens: self.prompt_budget_tokens,
            });
        }
        if projected_tool_result_tokens > self.tool_result_budget_tokens {
            return Err(DispatchRejection::ToolResultBudgetExceeded {
                projected_tokens: projected_tool_result_tokens,
                limit_tokens: self.tool_result_budget_tokens,
            });
        }
        if projected_tool_steps > self.max_tool_steps {
            return Err(DispatchRejection::ToolStepLimitReached {
                projected_steps: projected_tool_steps,
                limit_steps: self.max_tool_steps,
            });
        }
        if projected_same_tool_calls > self.max_same_tool_calls {
            return Err(DispatchRejection::SameToolLimitReached {
                projected_calls: projected_same_tool_calls,
                limit_calls: self.max_same_tool_calls,
            });
        }

        Ok(DispatchAdmission {
            projected_prompt_tokens,
            projected_tool_result_tokens,
            projected_tool_steps,
            projected_same_tool_calls,
            remaining_prompt_tokens: self.prompt_budget_tokens - projected_prompt_tokens,
            remaining_tool_result_tokens: self.tool_result_budget_tokens
                - projected_tool_result_tokens,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DispatchUsage {
    /// Total prompt tokens already retained, including prior tool results.
    pub prompt_tokens: u32,
    /// Tool-result subset of `prompt_tokens`.
    pub tool_result_tokens: u32,
    pub tool_steps: u32,
    pub same_tool_calls: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DispatchAdmission {
    pub projected_prompt_tokens: u32,
    pub projected_tool_result_tokens: u32,
    pub projected_tool_steps: u32,
    pub projected_same_tool_calls: u32,
    pub remaining_prompt_tokens: u32,
    pub remaining_tool_result_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DispatchRejection {
    #[error("tool result allowance must be greater than zero")]
    EmptyResultAllowance,
    #[error(
        "prospective tool result requests {requested_tokens} tokens, exceeding per-result limit {limit_tokens}"
    )]
    PerResultLimitExceeded {
        requested_tokens: u32,
        limit_tokens: u32,
    },
    #[error(
        "tool dispatch would use {projected_tokens} prompt tokens, exceeding budget {limit_tokens}"
    )]
    PromptBudgetExceeded {
        projected_tokens: u32,
        limit_tokens: u32,
    },
    #[error(
        "tool dispatch would retain {projected_tokens} tool-result tokens, exceeding budget {limit_tokens}"
    )]
    ToolResultBudgetExceeded {
        projected_tokens: u32,
        limit_tokens: u32,
    },
    #[error("tool dispatch would be step {projected_steps}, exceeding limit {limit_steps}")]
    ToolStepLimitReached {
        projected_steps: u32,
        limit_steps: u32,
    },
    #[error(
        "tool dispatch would be repeated call {projected_calls}, exceeding same-tool limit {limit_calls}"
    )]
    SameToolLimitReached {
        projected_calls: u32,
        limit_calls: u32,
    },
    #[error("context accounting overflowed `{field}`")]
    ArithmeticOverflow { field: &'static str },
}

/// Validate context-budget settings without requiring a model resolution.
/// Settings persistence can call this before accepting user-provided values.
pub fn validate_settings(settings: &HarnessSettings) -> Result<(), BudgetError> {
    require_fraction(
        "memoryCompactionFloor",
        settings.memory_compaction_floor,
        true,
    )?;
    require_positive("maxTurns", settings.max_turns)?;
    require_positive(
        "minimumFinalAnswerTokens",
        settings.minimum_final_answer_tokens,
    )?;
    require_fraction(
        "contextSafetyFraction",
        settings.context_safety_fraction,
        false,
    )?;
    require_positive("contextSafetyMinTokens", settings.context_safety_min_tokens)?;
    require_fraction("toolContextFraction", settings.tool_context_fraction, true)?;
    require_positive("toolResultMaxTokens", settings.tool_result_max_tokens)?;
    require_positive("summaryMaxTokens", settings.summary_max_tokens)?;
    require_positive("maxToolSteps", settings.max_tool_steps)?;
    require_positive("maxSameToolCalls", settings.max_same_tool_calls)?;
    require_positive("maxTokens", settings.max_tokens)?;
    if settings.max_tool_steps > 6 {
        return Err(BudgetError::InvalidSetting {
            field: "maxToolSteps",
            reason: "must not exceed the app-owned limit of 6",
        });
    }
    if settings.max_same_tool_calls > 2 {
        return Err(BudgetError::InvalidSetting {
            field: "maxSameToolCalls",
            reason: "must not exceed the app-owned limit of 2",
        });
    }
    if settings.max_same_tool_calls > settings.max_tool_steps {
        return Err(BudgetError::InvalidSetting {
            field: "maxSameToolCalls",
            reason: "must not exceed maxToolSteps",
        });
    }
    Ok(())
}

fn require_positive(field: &'static str, value: u32) -> Result<(), BudgetError> {
    if value == 0 {
        return Err(BudgetError::InvalidSetting {
            field,
            reason: "must be greater than zero",
        });
    }
    Ok(())
}

fn require_fraction(
    field: &'static str,
    value: f64,
    require_non_zero: bool,
) -> Result<(), BudgetError> {
    let lower_bound_valid = if require_non_zero {
        value > 0.0
    } else {
        value >= 0.0
    };
    if !value.is_finite() || !lower_bound_valid || value >= 1.0 {
        return Err(BudgetError::InvalidSetting {
            field,
            reason: "must be a finite fraction between zero and one",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::builtin::BUILTIN_MODEL_ID;
    use crate::harness::context::ContextSource;
    use crate::harness::model_directory::ModelProfile;
    use crate::harness::tokens::TokenizerKind;

    fn profile(context_window: u32, output_limit: Option<u32>) -> ModelProfile {
        ModelProfile {
            id: "test::model".into(),
            model_id: "model".into(),
            provider: "test".into(),
            name: "Model".into(),
            family: None,
            context_window: Some(context_window),
            output_limit,
            supports_tools: true,
            structured_output: false,
            reasoning: false,
            attachment: false,
            modalities_in: vec!["text".into()],
            modalities_out: vec!["text".into()],
            pricing_input_per_million: None,
            pricing_output_per_million: None,
            tokenizer_hint: TokenizerKind::CharsApproximation,
            source: vec!["test".into()],
            release_date: None,
            last_updated: None,
        }
    }

    #[test]
    fn jan_8192_budget_reserves_answer_safety_and_tool_space() {
        let context = ContextSpec::resolve(BUILTIN_MODEL_ID, None, None);
        let budget = TurnBudget::from_settings(context, &HarnessSettings::default(), None).unwrap();

        assert_eq!(budget.context.context_window_tokens, 8_192);
        assert_eq!(budget.context.source, ContextSource::BuiltIn);
        assert_eq!(budget.reserved_completion_tokens, 2_048);
        assert_eq!(budget.safety_reserve_tokens, 410);
        assert_eq!(budget.prompt_budget_tokens, 5_734);
        assert_eq!(budget.tool_result_budget_tokens, 2_580);
        assert_eq!(budget.per_tool_result_max_tokens, 512);
        assert_eq!(budget.summary_max_tokens, 384);
        assert_eq!(budget.max_tool_steps, 6);
        assert_eq!(budget.max_same_tool_calls, 2);
    }

    #[test]
    fn provider_and_context_limits_clamp_requested_budgets() {
        let profile = profile(4_096, Some(1_024));
        let context = ContextSpec::resolve("model", None, Some(&profile));
        let settings = HarnessSettings {
            max_tokens: 8_192,
            tool_result_max_tokens: 4_096,
            summary_max_tokens: 8_192,
            ..HarnessSettings::default()
        };

        let budget = TurnBudget::from_settings(context, &settings, Some(16_384)).unwrap();

        assert_eq!(budget.reserved_completion_tokens, 1_024);
        assert_eq!(budget.safety_reserve_tokens, 256);
        assert_eq!(budget.prompt_budget_tokens, 2_816);
        assert_eq!(budget.tool_result_budget_tokens, 1_267);
        assert_eq!(budget.per_tool_result_max_tokens, 1_267);
        assert_eq!(budget.summary_max_tokens, 2_816);
    }

    #[test]
    fn impossible_context_is_rejected_before_dispatch() {
        let profile = profile(700, Some(512));
        let context = ContextSpec::resolve("model", None, Some(&profile));

        let error =
            TurnBudget::from_settings(context, &HarnessSettings::default(), None).unwrap_err();

        assert_eq!(
            error,
            BudgetError::InsufficientContext {
                context_tokens: 700,
                safety_tokens: 256,
                minimum_completion_tokens: 512,
            }
        );
    }

    #[test]
    fn admission_rejects_integer_overflow_instead_of_saturating() {
        let context = ContextSpec::resolve(BUILTIN_MODEL_ID, None, None);
        let budget = TurnBudget::from_settings(context, &HarnessSettings::default(), None).unwrap();
        let usage = DispatchUsage {
            prompt_tokens: u32::MAX,
            tool_result_tokens: 0,
            tool_steps: 0,
            same_tool_calls: 0,
        };

        let error = budget.admit_dispatch(usage, 1).unwrap_err();

        assert_eq!(
            error,
            DispatchRejection::ArithmeticOverflow {
                field: "promptTokens"
            }
        );
    }

    #[test]
    fn admission_tracks_both_prompt_and_tool_sub_budget() {
        let context = ContextSpec::resolve(BUILTIN_MODEL_ID, None, None);
        let budget = TurnBudget::from_settings(context, &HarnessSettings::default(), None).unwrap();
        let usage = DispatchUsage {
            prompt_tokens: 2_000,
            tool_result_tokens: 500,
            tool_steps: 1,
            same_tool_calls: 0,
        };

        let admission = budget.admit_dispatch(usage, 512).unwrap();

        assert_eq!(admission.projected_prompt_tokens, 2_512);
        assert_eq!(admission.projected_tool_result_tokens, 1_012);
        assert_eq!(admission.projected_tool_steps, 2);
        assert_eq!(admission.projected_same_tool_calls, 1);
        assert_eq!(admission.remaining_prompt_tokens, 3_222);
        assert_eq!(admission.remaining_tool_result_tokens, 1_568);
    }

    #[test]
    fn invalid_fraction_and_step_relationship_are_rejected() {
        let context = ContextSpec::resolve(BUILTIN_MODEL_ID, None, None);
        let bad_fraction = HarnessSettings {
            tool_context_fraction: f64::NAN,
            ..HarnessSettings::default()
        };
        assert!(matches!(
            TurnBudget::from_settings(context.clone(), &bad_fraction, None),
            Err(BudgetError::InvalidSetting {
                field: "toolContextFraction",
                ..
            })
        ));

        let bad_steps = HarnessSettings {
            max_tool_steps: 1,
            max_same_tool_calls: 2,
            ..HarnessSettings::default()
        };
        assert!(matches!(
            TurnBudget::from_settings(context, &bad_steps, None),
            Err(BudgetError::InvalidSetting {
                field: "maxSameToolCalls",
                ..
            })
        ));
    }
}
