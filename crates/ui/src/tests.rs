use super::*;
use std::collections::HashSet;

fn all_unique(ids: impl IntoIterator<Item = &'static str>) -> bool {
    let ids: Vec<_> = ids.into_iter().collect();
    ids.iter().collect::<HashSet<_>>().len() == ids.len()
}

#[test]
fn enum_ids_are_unique() {
    assert!(all_unique(ButtonStyle::ALL.map(ButtonStyle::id)));
    assert!(all_unique(ControlSize::ALL.map(ControlSize::id)));
    assert!(all_unique(ButtonShape::ALL.map(ButtonShape::id)));
    assert!(all_unique(Tint::ALL.map(Tint::id)));
    assert!(all_unique(TextStyle::ALL.map(TextStyle::id)));
    assert!(all_unique(MediaRatio::ALL.map(MediaRatio::id)));
}

/// Default values omit their attribute – the CSS defines no rule for them.
#[test]
fn defaults_omit_their_attribute() {
    assert_eq!(ControlSize::default().attr(), None);
    assert_eq!(ButtonShape::default().attr(), None);
    assert_eq!(BadgeVariant::default().attr(), None);
    assert_eq!(Contrast::default().attr(), None);
    assert_eq!(ControlSize::Large.attr(), Some("large"));
    assert_eq!(Contrast::High.attr(), Some("high"));
}

/// The IDs must match the selectors in `components.css`.
#[test]
fn ids_match_css_selectors() {
    let css = include_str!("../components.css");
    for style in ButtonStyle::ALL {
        // `bordered` is the default and has no rule of its own.
        if style != ButtonStyle::Bordered {
            assert!(
                css.contains(&format!("data-style=\"{}\"", style.id())),
                "no CSS for data-style={}",
                style.id()
            );
        }
    }
    for size in ControlSize::ALL {
        if let Some(id) = size.attr() {
            assert!(
                css.contains(&format!("data-control-size=\"{id}\"")),
                "no CSS for {id}"
            );
        }
    }
    for ratio in MediaRatio::ALL {
        if ratio != MediaRatio::R16x9 {
            assert!(css.contains(&format!("data-ratio=\"{}\"", ratio.id())));
        }
    }
    for tone in StatusTone::ALL {
        if let Some(id) = tone.attr() {
            assert!(
                css.contains(&format!("data-tone=\"{id}\"")),
                "no CSS for tone {id}"
            );
        }
    }
    for style in TextStyle::ALL {
        assert!(
            css.contains(&format!(".t-{} ", style.id())),
            "no CSS for .t-{}",
            style.id()
        );
    }
}
