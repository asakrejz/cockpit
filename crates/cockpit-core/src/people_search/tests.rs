use super::*;

fn p(name: &str) -> Person {
    Person::new(name, "XX")
}

fn names(list: &[Person]) -> Vec<String> {
    list.iter()
        .map(|p| p.name_in(cockpit_i18n::Lang::De))
        .collect()
}

#[test]
fn folds_case_and_accents() {
    assert_eq!(fold("Jörg BÖHM"), "jorg bohm");
    assert_eq!(fold("Weiß"), "weiss");
    assert_eq!(fold("Zoë Łukasz"), "zoe lukasz");
}

#[test]
fn finds_without_caring_for_accents_or_case() {
    let all = [p("Timo Böhm"), p("Mira Keller")];
    assert_eq!(names(&search(&all, &[], "BOHM")), ["Timo Böhm"]);
    assert_eq!(names(&search(&all, &[], "böh")), ["Timo Böhm"]);
}

#[test]
fn needs_two_characters() {
    let all = [p("Mira Keller")];
    assert!(search(&all, &[], "").is_empty());
    assert!(search(&all, &[], " m ").is_empty());
    assert_eq!(search(&all, &[], "mi").len(), 1);
}

#[test]
fn prefix_before_word_start_before_substring() {
    let all = [
        p("Lena Ammer"),
        p("Samuel Roth"),
        p("Amelie Kraus"),
        p("Anna Maier"),
    ];
    // "am": Amelie (name starts), Anna Maier no ("ma" ≠ "am"), Lena Ammer (word "ammer"),
    // Samuel (inside a word).
    assert_eq!(
        names(&search(&all, &[], "am")),
        ["Amelie Kraus", "Lena Ammer", "Samuel Roth"]
    );
}

#[test]
fn me_comes_first_and_nobody_twice() {
    let local = [p("Mira Keller"), Person::me(), p("Michael Ott")];
    let remote = [p("Mira Keller"), p("MICHAEL OTT"), p("Mikka Lind")];
    let found = search(&local, &remote, "mi");
    assert_eq!(names(&found), ["Mira Keller", "Michael Ott", "Mikka Lind"]);
    let with_me = search(&[p("Dunja Rau"), Person::me()], &[], "du");
    assert_eq!(names(&with_me), ["Du", "Dunja Rau"]);
}

#[test]
fn local_wins_over_remote_and_order_inside_a_rank_is_stable() {
    let local = [Person::new("Mira Keller", "MK")];
    let remote = [Person::new("mira keller", "ZZ"), p("Mirko Vogt")];
    let found = search(&local, &remote, "mir");
    assert_eq!(found[0].initials, "MK");
    assert_eq!(names(&found), ["Mira Keller", "Mirko Vogt"]);
}

#[test]
fn leaves_out_the_agent_and_unassigned_and_cuts_to_eight() {
    let mut all = vec![Person::agent(), Person::nobody(), p("Nicht Jemand")];
    for i in 0..12 {
        all.push(p(&format!("Nora {i}")));
    }
    let found = search(&all, &[], "no");
    assert_eq!(found.len(), MAX_SUGGESTIONS);
    assert!(found.iter().all(|f| !f.is_agent() && !f.nobody));
    assert!(search(&all, &[], "agent").is_empty());
}
