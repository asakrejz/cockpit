//! Temperatur-Umrechner Celsius ⇄ Fahrenheit.
//!
//! Zeigt: Trennung von reiner Logik (testbar ohne Browser) und UI,
//! sowie zwei Eingabefelder, die sich gegenseitig aktualisieren.

use leptos::prelude::*;

/// Celsius → Fahrenheit
pub fn celsius_to_fahrenheit(c: f64) -> f64 {
    c * 9.0 / 5.0 + 32.0
}

/// Fahrenheit → Celsius
pub fn fahrenheit_to_celsius(f: f64) -> f64 {
    (f - 32.0) * 5.0 / 9.0
}

/// Rundet auf eine Nachkommastelle und entfernt ein überflüssiges ".0".
fn format_temp(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    }
}

#[component]
pub fn TemperatureConverter(
    /// Startwert in °C.
    #[prop(default = 20.0)]
    initial_celsius: f64,
) -> impl IntoView {
    // Wir speichern den *Text* beider Felder. So überschreiben wir nie,
    // was der Nutzer gerade tippt (z. B. "12." oder "-").
    let (celsius, set_celsius) = signal(format_temp(initial_celsius));
    let (fahrenheit, set_fahrenheit) = signal(format_temp(celsius_to_fahrenheit(initial_celsius)));

    let on_celsius = move |ev| {
        let text = event_target_value(&ev);
        if let Ok(c) = text.trim().parse::<f64>() {
            set_fahrenheit.set(format_temp(celsius_to_fahrenheit(c)));
        }
        set_celsius.set(text);
    };

    let on_fahrenheit = move |ev| {
        let text = event_target_value(&ev);
        if let Ok(f) = text.trim().parse::<f64>() {
            set_celsius.set(format_temp(fahrenheit_to_celsius(f)));
        }
        set_fahrenheit.set(text);
    };

    view! {
        <div class="temperature">
            <label>
                <input type="number" step="any" prop:value=celsius on:input=on_celsius />
                " °C"
            </label>
            <span class="temperature__eq">"="</span>
            <label>
                <input type="number" step="any" prop:value=fahrenheit on:input=on_fahrenheit />
                " °F"
            </label>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_values() {
        assert_eq!(celsius_to_fahrenheit(0.0), 32.0);
        assert_eq!(celsius_to_fahrenheit(100.0), 212.0);
        assert_eq!(fahrenheit_to_celsius(-40.0), -40.0);
    }

    #[test]
    fn roundtrip() {
        let c = 37.5;
        let back = fahrenheit_to_celsius(celsius_to_fahrenheit(c));
        assert!((back - c).abs() < 1e-9);
    }

    #[test]
    fn formatting() {
        assert_eq!(format_temp(68.0), "68");
        assert_eq!(format_temp(99.86), "99.9");
        assert_eq!(format_temp(-17.777), "-17.8");
    }
}
