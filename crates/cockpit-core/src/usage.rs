//! Consumption and limits of the agents: tokens, costs, runtime, budgets.
//!
//! * [`Usage`] is the consumption of a step (and, summed, of a run). Money is always a
//!   whole number in **euro cents**, never a floating-point number.
//! * [`Pricing`] are the prices per million tokens – a setting. In the demo every
//!   step produces deterministic values (see [`crate::demo::step_tokens`]); the real agent later
//!   enters the reported values via [`crate::AgentRun::record_usage`].
//! * [`AgentLimits`] are the adjustable limits; [`StopReason`] says which one stopped a run.
//! * [`Charge`] is a booking in the workspace's ledger. „Today“ and „this
//!   month“ are derived from it. Day and month count in UTC – the logic knows no time zone.
//!
//! Everything here is pure: the time comes from outside.

use cockpit_i18n::{Msg, msg};
use serde::{Deserialize, Serialize};

use crate::agent::AgentRun;

/// Milliseconds per day.
const DAY_MS: u64 = 86_400_000;
/// Bookings stay in the ledger this long (longer than any month).
pub const KEEP_DAYS: u64 = 62;
/// The ledger keeps at most this many bookings.
pub const MAX_CHARGES: usize = 2_000;

/// Consumption of a step or of a whole run.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Costs in euro cents.
    pub cost_cents: u64,
    /// Runtime in milliseconds.
    pub millis: u64,
}

impl Usage {
    /// Consumption with costs according to the prices from the setting.
    pub fn priced(input_tokens: u64, output_tokens: u64, millis: u64, pricing: &Pricing) -> Self {
        Self {
            input_tokens,
            output_tokens,
            cost_cents: pricing.cost_cents(input_tokens, output_tokens),
            millis,
        }
    }

    pub fn tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    pub fn is_empty(&self) -> bool {
        *self == Usage::default()
    }

    pub fn add(&mut self, other: &Usage) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cost_cents = self.cost_cents.saturating_add(other.cost_cents);
        self.millis = self.millis.saturating_add(other.millis);
    }

    /// `12.400 Token ein · 1.200 aus · 25 ct · 25 s`
    pub fn line_msg(&self) -> Msg {
        msg!(
            CoreUsageLine,
            input = format_number(self.input_tokens),
            output = format_number(self.output_tokens),
            cost = format_euro(self.cost_cents),
            time = format_duration(self.millis)
        )
    }

    /// [`Usage::line_msg`] in German.
    pub fn line(&self) -> String {
        self.line_msg().de()
    }
}

/// Prices in euro cents per million tokens.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct Pricing {
    pub input_cents_per_mtok: u64,
    pub output_cents_per_mtok: u64,
}

impl Default for Pricing {
    fn default() -> Self {
        Self {
            input_cents_per_mtok: 300,
            output_cents_per_mtok: 1_500,
        }
    }
}

impl Pricing {
    /// Costs in whole cents, rounded up: consumption is never free if the price is above 0.
    pub fn cost_cents(&self, input_tokens: u64, output_tokens: u64) -> u64 {
        let micro = (u128::from(input_tokens) * u128::from(self.input_cents_per_mtok))
            .saturating_add(u128::from(output_tokens) * u128::from(self.output_cents_per_mtok));
        u64::try_from(micro.div_ceil(1_000_000)).unwrap_or(u64::MAX)
    }
}

/// Which limit stopped a run.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The budget per run is reached.
    RunBudget,
    /// The budget for the month is reached.
    MonthBudget,
    /// The time limit per run is reached.
    Time,
}

impl StopReason {
    pub fn label_msg(self) -> Msg {
        match self {
            StopReason::RunBudget => msg!(CoreUsageStopRunBudget),
            StopReason::MonthBudget => msg!(CoreUsageStopMonthBudget),
            StopReason::Time => msg!(CoreUsageStopTime),
        }
    }

    /// [`StopReason::label_msg`] in German.
    pub fn label(self) -> String {
        self.label_msg().de()
    }

    /// What is raised on resume.
    pub fn raises_msg(self) -> Msg {
        match self {
            StopReason::RunBudget => msg!(CoreUsageRaisesRunBudget),
            StopReason::MonthBudget => msg!(CoreUsageRaisesMonthBudget),
            StopReason::Time => msg!(CoreUsageRaisesTime),
        }
    }

    /// [`StopReason::raises_msg`] in German.
    pub fn raises(self) -> String {
        self.raises_msg().de()
    }

    /// Increases the UI offers: cents for budgets, minutes for the time limit.
    pub fn raise_options(self) -> [u64; 2] {
        match self {
            StopReason::RunBudget | StopReason::MonthBudget => [100, 500],
            StopReason::Time => [10, 30],
        }
    }

    /// `1,00 €` bzw. `10 Min`
    pub fn raise_label_msg(self, extra: u64) -> Msg {
        match self {
            StopReason::Time => msg!(CoreUsageMinutes, n = extra),
            _ => Msg::raw(format_euro(extra)),
        }
    }

    /// [`StopReason::raise_label_msg`] in German.
    pub fn raise_label(self, extra: u64) -> String {
        self.raise_label_msg(extra).de()
    }
}

/// The adjustable limits. `None` means: no limit.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct AgentLimits {
    pub pricing: Pricing,
    /// Budget per run in cents.
    pub run_budget_cents: Option<u64>,
    /// Budget per month in cents, across all runs.
    pub month_budget_cents: Option<u64>,
    /// Time limit per run in minutes (runtime of the agent, not waiting time for you).
    pub run_minutes: Option<u32>,
    /// This many runs may work at the same time (at least 1).
    pub max_parallel: u32,
}

impl Default for AgentLimits {
    fn default() -> Self {
        Self {
            pricing: Pricing::default(),
            run_budget_cents: Some(500),
            month_budget_cents: Some(5_000),
            run_minutes: Some(30),
            max_parallel: 3,
        }
    }
}

impl AgentLimits {
    /// Without any limit (for tests of a run without a workspace).
    #[cfg(test)]
    pub fn unlimited() -> Self {
        Self {
            pricing: Pricing::default(),
            run_budget_cents: None,
            month_budget_cents: None,
            run_minutes: None,
            max_parallel: u32::MAX,
        }
    }

    /// A limit of 0 would stop every run immediately, without parallelism nothing would run.
    pub fn is_valid(&self) -> bool {
        self.max_parallel >= 1
            && self.run_budget_cents != Some(0)
            && self.month_budget_cents != Some(0)
            && self.run_minutes != Some(0)
    }

    /// Which limit is reached for this run? `month_spent_cents` is what was spent in the current
    /// month across all runs. The run's increases count too.
    pub fn reached(&self, run: &AgentRun, month_spent_cents: u64) -> Option<StopReason> {
        let usage = run.usage();
        if let Some(budget) = self.run_budget_cents
            && usage.cost_cents >= budget.saturating_add(run.extra_budget_cents)
        {
            return Some(StopReason::RunBudget);
        }
        if let Some(budget) = self.month_budget_cents
            && month_spent_cents >= budget
        {
            return Some(StopReason::MonthBudget);
        }
        if let Some(minutes) = self.run_minutes {
            let allowed = u64::from(minutes).saturating_add(run.extra_minutes);
            if usage.millis >= allowed.saturating_mul(60_000) {
                return Some(StopReason::Time);
            }
        }
        None
    }

    /// A sentence that says how far consumption and limit are. For the hint at the
    /// stopped run.
    pub fn describe_msg(&self, reason: StopReason, run: &AgentRun, month_spent_cents: u64) -> Msg {
        let usage = run.usage();
        match reason {
            StopReason::RunBudget => msg!(
                CoreUsageDescribeRunBudget,
                used = format_euro(usage.cost_cents),
                allowed = format_euro(
                    self.run_budget_cents
                        .unwrap_or_default()
                        .saturating_add(run.extra_budget_cents)
                )
            ),
            StopReason::MonthBudget => msg!(
                CoreUsageDescribeMonthBudget,
                used = format_euro(month_spent_cents),
                allowed = format_euro(self.month_budget_cents.unwrap_or_default())
            ),
            StopReason::Time => msg!(
                CoreUsageDescribeTime,
                used = format_duration(usage.millis),
                allowed = u64::from(self.run_minutes.unwrap_or_default())
                    .saturating_add(run.extra_minutes)
            ),
        }
    }

    /// [`AgentLimits::describe_msg`] in German.
    pub fn describe(&self, reason: StopReason, run: &AgentRun, month_spent_cents: u64) -> String {
        self.describe_msg(reason, run, month_spent_cents).de()
    }

    /// Short sentence for the log.
    pub fn summary_msg(&self) -> Msg {
        let money = |v: Option<u64>| {
            v.map_or_else(|| msg!(CoreUsageUnlimited), |c| Msg::raw(format_euro(c)))
        };
        msg!(
            CoreUsageSummary,
            run = money(self.run_budget_cents),
            month = money(self.month_budget_cents),
            time = self.run_minutes.map_or_else(
                || msg!(CoreUsageUnlimited),
                |m| msg!(CoreUsageMinutes, n = m)
            ),
            parallel = self.max_parallel
        )
    }

    /// [`AgentLimits::summary_msg`] in German.
    pub fn summary(&self) -> String {
        self.summary_msg().de()
    }
}

/// A booking in the ledger: what a step cost.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Charge {
    pub at_ms: u64,
    pub ticket: String,
    pub usage: Usage,
}

/// Sum of the bookings in the period `[from_ms, to_ms)`.
pub fn sum_between(charges: &[Charge], from_ms: u64, to_ms: u64) -> Usage {
    let mut sum = Usage::default();
    for c in charges
        .iter()
        .filter(|c| c.at_ms >= from_ms && c.at_ms < to_ms)
    {
        sum.add(&c.usage);
    }
    sum
}

/// Start of the UTC day in which `ms` lies.
pub fn day_start(ms: u64) -> u64 {
    ms - ms % DAY_MS
}

/// Start of the UTC month in which `ms` lies.
pub fn month_start(ms: u64) -> u64 {
    let (year, month, _) = civil_from_days((ms / DAY_MS) as i64);
    days_from_civil(year, month, 1) as u64 * DAY_MS
}

/// Start of the month after the one in which `ms` lies.
pub fn next_month_start(ms: u64) -> u64 {
    let (year, month, _) = civil_from_days((ms / DAY_MS) as i64);
    let (year, month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    days_from_civil(year, month, 1) as u64 * DAY_MS
}

/// Days since 1970 → (year, month, day). Algorithm by Howard Hinnant.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// (year, month, day) → days since 1970.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if month > 2 { month - 3 } else { month + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// ---------- Presentation ----------

/// `12400` → `12.400`
pub fn format_number(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

/// `550` → `5,50 €`
pub fn format_euro(cents: u64) -> String {
    format!("{},{:02} €", format_number(cents / 100), cents % 100)
}

/// `5,50`, `5.5`, `5 €` → cents. Negative amounts, more than two decimal places and nonsense → `None`.
pub fn parse_euro(text: &str) -> Option<u64> {
    let text = text.trim().trim_end_matches('€').trim();
    let (whole, frac) = match text.split_once([',', '.']) {
        Some((w, f)) => (w, f),
        None => (text, ""),
    };
    let all_digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    if whole.is_empty() || !all_digits(whole) || !all_digits(frac) || frac.len() > 2 {
        return None;
    }
    let cents: u64 = format!("{frac:0<2}").parse().ok()?;
    whole
        .parse::<u64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(cents)
}

/// `25000` → `25 s`, `90000` → `1 Min 30 s`
pub fn format_duration(millis: u64) -> String {
    let secs = millis / 1000;
    match (secs / 60, secs % 60) {
        (0, s) => format!("{s} s"),
        (m, 0) => format!("{m} Min"),
        (m, s) => format!("{m} Min {s} s"),
    }
}

#[cfg(test)]
mod tests;
