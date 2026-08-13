// @agent-context: Pure context-window resolution and per-turn admission budgets.
//
// Keep provider I/O out of this module. Callers supply runtime metadata and an
// optional ModelProfile; the result records which source won so conservative
// fallback behavior is visible in telemetry and tests.

mod budget;
mod spec;

pub use budget::{
    validate_settings, BudgetError, DispatchAdmission, DispatchRejection, DispatchUsage, TurnBudget,
};
pub use spec::{ContextSource, ContextSpec, CONSERVATIVE_FALLBACK_CONTEXT_TOKENS};
