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
