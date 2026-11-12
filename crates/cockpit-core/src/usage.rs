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
    pub fn line(&self) -> String {
        format!(
            "{} Token ein · {} aus · {} · {}",
            format_number(self.input_tokens),
            format_number(self.output_tokens),
            format_euro(self.cost_cents),
            format_duration(self.millis)
        )
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
    pub fn label(self) -> &'static str {
        match self {
            StopReason::RunBudget => "Budget pro Lauf erreicht",
            StopReason::MonthBudget => "Monatsbudget erreicht",
            StopReason::Time => "Zeitlimit erreicht",
        }
    }

    /// What is raised on resume.
    pub fn raises(self) -> &'static str {
        match self {
            StopReason::RunBudget => "das Budget dieses Laufs",
            StopReason::MonthBudget => "das Monatsbudget",
            StopReason::Time => "das Zeitlimit dieses Laufs",
        }
    }

    /// Increases the UI offers: cents for budgets, minutes for the time limit.
    pub fn raise_options(self) -> [u64; 2] {
        match self {
            StopReason::RunBudget | StopReason::MonthBudget => [100, 500],
            StopReason::Time => [10, 30],
        }
    }

    /// `1,00 €` bzw. `10 Min`
    pub fn raise_label(self, extra: u64) -> String {
        match self {
            StopReason::Time => format!("{extra} Min"),
            _ => format_euro(extra),
        }
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
    /// Without any limit (for runs that are clocked without a workspace).
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
    pub fn describe(&self, reason: StopReason, run: &AgentRun, month_spent_cents: u64) -> String {
        let usage = run.usage();
        match reason {
            StopReason::RunBudget => format!(
                "Der Lauf hat {} verbraucht, erlaubt sind {}.",
                format_euro(usage.cost_cents),
                format_euro(
                    self.run_budget_cents
                        .unwrap_or_default()
                        .saturating_add(run.extra_budget_cents)
                )
            ),
            StopReason::MonthBudget => format!(
                "In diesem Monat sind {} verbraucht, erlaubt sind {}.",
                format_euro(month_spent_cents),
                format_euro(self.month_budget_cents.unwrap_or_default())
            ),
            StopReason::Time => format!(
                "Der Lauf arbeitet seit {}, erlaubt sind {} Min.",
                format_duration(usage.millis),
                u64::from(self.run_minutes.unwrap_or_default()).saturating_add(run.extra_minutes)
            ),
        }
    }

    /// Short sentence for the log.
    pub fn summary(&self) -> String {
        let money = |v: Option<u64>| v.map_or("unbegrenzt".to_string(), format_euro);
        format!(
            "Budget pro Lauf {}, pro Monat {}, Zeitlimit {}, höchstens {} parallel",
            money(self.run_budget_cents),
            money(self.month_budget_cents),
            self.run_minutes
                .map_or("unbegrenzt".to_string(), |m| format!("{m} Min")),
            self.max_parallel
        )
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
mod tests {
    use super::*;

    #[test]
    fn costs_are_whole_cents_rounded_up() {
        let p = Pricing::default(); // 3 € in, 15 € out per million
        assert_eq!(p.cost_cents(0, 0), 0);
        assert_eq!(p.cost_cents(1_000_000, 0), 300);
        assert_eq!(p.cost_cents(0, 1_000_000), 1_500);
        // 12.400 × 300 / 1.000.000 = 3,72 ct and 1.200 × 1.500 / 1.000.000 = 1,8 ct: together 5,52 → 6 ct.
        assert_eq!(p.cost_cents(12_400, 1_200), 6);
        // A single token costs at least one cent, as long as there is a price.
        assert_eq!(p.cost_cents(1, 0), 1);
        // Huge values do not overflow but stay at the cap.
        let huge = Pricing {
            input_cents_per_mtok: u64::MAX,
            output_cents_per_mtok: u64::MAX,
        };
        assert_eq!(huge.cost_cents(u64::MAX, u64::MAX), u64::MAX);
    }

    #[test]
    fn free_pricing_costs_nothing() {
        let free = Pricing {
            input_cents_per_mtok: 0,
            output_cents_per_mtok: 0,
        };
        assert_eq!(free.cost_cents(5_000_000, 5_000_000), 0);
    }

    #[test]
    fn usage_adds_up() {
        let mut sum = Usage::default();
        sum.add(&Usage {
            input_tokens: 10,
            output_tokens: 5,
            cost_cents: 2,
            millis: 1000,
        });
        sum.add(&Usage {
            input_tokens: 1,
            output_tokens: 1,
            cost_cents: 1,
            millis: 500,
        });
        assert_eq!(
            sum,
            Usage {
                input_tokens: 11,
                output_tokens: 6,
                cost_cents: 3,
                millis: 1500
            }
        );
        assert_eq!(sum.tokens(), 17);
        assert!(!sum.is_empty());
        assert!(Usage::default().is_empty());
    }

    #[test]
    fn money_is_formatted_and_parsed_in_cents() {
        assert_eq!(format_euro(0), "0,00 €");
        assert_eq!(format_euro(5), "0,05 €");
        assert_eq!(format_euro(550), "5,50 €");
        assert_eq!(format_euro(123_456), "1.234,56 €");
        for (text, cents) in [
            ("5", 500),
            ("5,5", 550),
            ("5,50", 550),
            ("5.50", 550),
            (" 12,05 € ", 1205),
            ("0,01", 1),
        ] {
            assert_eq!(parse_euro(text), Some(cents), "{text}");
        }
        // Thousands separators are not guessed: „1.234,56“ is not a valid input.
        for bad in [
            "", " ", "abc", "-1", "1,234", "1,2,3", ",5", "€", "1.234,56",
        ] {
            assert_eq!(parse_euro(bad), None, "{bad:?}");
        }
        assert_eq!(parse_euro(&format_euro(550)), Some(550));
    }

    #[test]
    fn numbers_and_durations_read_well() {
        assert_eq!(format_number(7), "7");
        assert_eq!(format_number(1_000), "1.000");
        assert_eq!(format_number(1_234_567), "1.234.567");
        assert_eq!(format_duration(25_000), "25 s");
        assert_eq!(format_duration(60_000), "1 Min");
        assert_eq!(format_duration(90_000), "1 Min 30 s");
        assert_eq!(format_duration(0), "0 s");
    }

    #[test]
    fn days_and_months_count_in_utc() {
        // 2023-11-14 22:13:20 UTC
        let t = 1_700_000_000_000;
        assert_eq!(day_start(t), 1_699_920_000_000); // 2023-11-14 00:00
        assert_eq!(month_start(t), 1_698_796_800_000); // 2023-11-01 00:00
        assert_eq!(next_month_start(t), 1_701_388_800_000); // 2023-12-01 00:00
        // Year change and leap year
        let dec = 1_702_000_000_000; // 2023-12-08
        assert_eq!(next_month_start(dec), 1_704_067_200_000); // 2024-01-01
        let feb = 1_709_000_000_000; // 2024-02-27
        assert_eq!(next_month_start(feb), 1_709_251_200_000); // 2024-03-01 (leap year: 29 days)
        assert_eq!(month_start(0), 0);
    }

    #[test]
    fn charges_sum_within_the_period_only() {
        let t = 1_700_000_000_000;
        let charge = |at_ms, cents| Charge {
            at_ms,
            ticket: "BAU-1".into(),
            usage: Usage {
                cost_cents: cents,
                ..Usage::default()
            },
        };
        let charges = vec![
            charge(month_start(t) - 1, 100), // Previous month
            charge(month_start(t), 10),
            charge(day_start(t) - 1, 20), // yesterday
            charge(day_start(t), 30),
            charge(t, 40),
        ];
        let today = sum_between(&charges, day_start(t), day_start(t) + DAY_MS);
        let month = sum_between(&charges, month_start(t), next_month_start(t));
        assert_eq!(today.cost_cents, 70);
        assert_eq!(month.cost_cents, 100);
    }

    #[test]
    fn limits_validate() {
        let with = |f: fn(&mut AgentLimits)| {
            let mut l = AgentLimits::default();
            f(&mut l);
            l
        };
        assert!(AgentLimits::default().is_valid());
        assert!(!with(|l| l.max_parallel = 0).is_valid());
        assert!(!with(|l| l.run_budget_cents = Some(0)).is_valid());
        assert!(!with(|l| l.run_minutes = Some(0)).is_valid());
        let none = with(|l| {
            l.month_budget_cents = None;
            l.run_budget_cents = None;
            l.run_minutes = None;
        });
        assert!(none.is_valid(), "„no limit“ is allowed");
    }

    #[test]
    fn limits_roundtrip_and_old_data_gets_defaults() {
        let l = AgentLimits::default();
        let back: AgentLimits = serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
        assert_eq!(back, l);
        let partial: AgentLimits = serde_json::from_str(r#"{"max_parallel": 2}"#).unwrap();
        assert_eq!(partial.max_parallel, 2);
        assert_eq!(partial.run_budget_cents, Some(500));
    }
}
