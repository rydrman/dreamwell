use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitDimension {
    Length,
    Mass,
    Time,
    Temperature,
    Ratio,
    Dimensionless,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum UnitClass {
    None,
    Ucum {
        canonical: String,
        dimension: UnitDimension,
        display_name: String,
    },
    Custom(String),
}

/// Colloquial spellings mapped to UCUM before validation. Bare `ft`/`in`/`lb` are not valid
/// UCUM for foot/inch/pound (and `ft` alone parses as femtotonne), so we normalize first.
const COMMON_UNIT_ALIASES: &[(&str, &str)] = &[
    ("ft", "[ft_i]"),
    ("ft_i", "[ft_i]"),
    ("foot", "[ft_i]"),
    ("feet", "[ft_i]"),
    ("in", "[in_i]"),
    ("in_i", "[in_i]"),
    ("inch", "[in_i]"),
    ("inches", "[in_i]"),
    ("yd", "[yd_i]"),
    ("yd_i", "[yd_i]"),
    ("yard", "[yd_i]"),
    ("yards", "[yd_i]"),
    ("mi", "[mi_us]"),
    ("mi_us", "[mi_us]"),
    ("mile", "[mi_us]"),
    ("miles", "[mi_us]"),
    ("lb", "[lb_av]"),
    ("lb_av", "[lb_av]"),
    ("lbs", "[lb_av]"),
    ("pound", "[lb_av]"),
    ("pounds", "[lb_av]"),
    ("oz", "[oz_av]"),
    ("oz_av", "[oz_av]"),
    ("ounce", "[oz_av]"),
    ("ounces", "[oz_av]"),
    ("f", "[degF]"),
    ("degf", "[degF]"),
    ("deg_f", "[degF]"),
    ("fahrenheit", "[degF]"),
    ("deg f", "[degF]"),
    ("c", "Cel"),
    ("celsius", "Cel"),
    ("degc", "Cel"),
    ("deg c", "Cel"),
    ("deg_c", "Cel"),
    ("m", "m"),
    ("meter", "m"),
    ("metre", "m"),
    ("meters", "m"),
    ("metres", "m"),
    ("cm", "cm"),
    ("centimeter", "cm"),
    ("centimetre", "cm"),
    ("centimeters", "cm"),
    ("centimetres", "cm"),
    ("km", "km"),
    ("kilometer", "km"),
    ("kilometre", "km"),
    ("kilometers", "km"),
    ("kilometres", "km"),
    ("kg", "kg"),
    ("kilogram", "kg"),
    ("kilograms", "kg"),
    ("kilogramme", "kg"),
    ("kilogrammes", "kg"),
    ("g", "g"),
    ("gram", "g"),
    ("grams", "g"),
    ("gramme", "g"),
    ("grammes", "g"),
    ("l", "L"),
    ("liter", "L"),
    ("litre", "L"),
    ("liters", "L"),
    ("litres", "L"),
    ("ml", "mL"),
    ("milliliter", "mL"),
    ("millilitre", "mL"),
    ("milliliters", "mL"),
    ("millilitres", "mL"),
    ("s", "s"),
    ("sec", "s"),
    ("secs", "s"),
    ("second", "s"),
    ("seconds", "s"),
    ("min", "min"),
    ("mins", "min"),
    ("minute", "min"),
    ("minutes", "min"),
    ("h", "h"),
    ("hr", "h"),
    ("hrs", "h"),
    ("hour", "h"),
    ("hours", "h"),
    ("percent", "%"),
    ("pct", "%"),
];

/// Preferred editor label for a stored canonical UCUM code.
const FRIENDLY_CANONICAL_LABELS: &[(&str, &str)] = &[
    ("[ft_i]", "ft"),
    ("[in_i]", "in"),
    ("[yd_i]", "yd"),
    ("[mi_us]", "mi"),
    ("[lb_av]", "lb"),
    ("[oz_av]", "oz"),
    ("[degF]", "°F"),
    ("Cel", "°C"),
];

fn normalize_alias_key(raw: &str) -> String {
    raw.trim().replace('°', "").trim().to_ascii_lowercase()
}

fn resolve_common_unit(raw: &str) -> Option<&'static str> {
    let key = normalize_alias_key(raw);
    if key.is_empty() {
        return None;
    }
    COMMON_UNIT_ALIASES
        .iter()
        .find(|(alias, _)| *alias == key)
        .map(|(_, ucum)| *ucum)
}

/// Editor-friendly label for a stored unit string (canonical UCUM or custom).
pub fn friendly_unit_label(unit: &str) -> &str {
    FRIENDLY_CANONICAL_LABELS
        .iter()
        .find(|(canonical, _)| *canonical == unit)
        .map(|(_, friendly)| *friendly)
        .unwrap_or(unit)
}

mod display;

pub use display::{
    format_feet_inches, format_measurement_change_display, format_measurement_display,
    format_measurement_value, MeasurementDisplay,
};

/// Parse an optional unit string into a normalized class for storage and UI.
pub fn classify_unit(unit: Option<&str>) -> UnitClass {
    let Some(raw) = unit.map(str::trim).filter(|s| !s.is_empty()) else {
        return UnitClass::None;
    };
    let candidate = resolve_common_unit(raw).unwrap_or(raw);
    if ucum::validate(candidate).is_ok() {
        let canonical = ucum::canonical(candidate).unwrap_or_else(|_| candidate.to_string());
        let display_name = ucum::display_name(candidate).unwrap_or_else(|_| candidate.to_string());
        let dimension = ucum::analyze(candidate)
            .map(ucum_dimension)
            .unwrap_or(UnitDimension::Dimensionless);
        return UnitClass::Ucum {
            canonical,
            dimension,
            display_name,
        };
    }
    UnitClass::Custom(raw.to_string())
}

fn ucum_dimension(analysis: ucum::Analysis) -> UnitDimension {
    if analysis.is_dimensionless {
        return UnitDimension::Ratio;
    }
    let d = analysis.dimension.0;
    if d[0] == 1 && d[1] == 0 && d[2] == 0 && d[4] == 0 {
        return UnitDimension::Length;
    }
    if d[2] == 1 && d[0] == 0 && d[1] == 0 && d[4] == 0 {
        return UnitDimension::Mass;
    }
    if d[1] == 1 && d[0] == 0 && d[2] == 0 && d[4] == 0 {
        return UnitDimension::Time;
    }
    if d[4] == 1 {
        return UnitDimension::Temperature;
    }
    if d == ucum::Dimension::DIMENSIONLESS.0 {
        return UnitDimension::Dimensionless;
    }
    UnitDimension::Custom
}

/// UCUM unit codes commonly used in scenario measurements, with plain-English hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitSuggestion {
    pub code: &'static str,
    pub label: &'static str,
}

/// Scenario-friendly unit labels for autocomplete. Common imperial abbreviations normalize
/// to UCUM on save. Values are decimal floats in that unit — e.g. 182 cm, 71 in, 5.5 ft.
pub const SCENARIO_UNIT_SUGGESTIONS: &[UnitSuggestion] = &[
    UnitSuggestion {
        code: "1",
        label: "dimensionless count",
    },
    UnitSuggestion {
        code: "%",
        label: "percent (0–100)",
    },
    UnitSuggestion {
        code: "stress",
        label: "custom game label",
    },
    UnitSuggestion {
        code: "cm",
        label: "centimeters (height)",
    },
    UnitSuggestion {
        code: "kg",
        label: "kilograms (weight)",
    },
    UnitSuggestion {
        code: "m",
        label: "meters",
    },
    UnitSuggestion {
        code: "km",
        label: "kilometers",
    },
    UnitSuggestion {
        code: "ft",
        label: "feet (decimal: 5.5 = five and a half feet)",
    },
    UnitSuggestion {
        code: "in",
        label: "inches (71 = five foot eleven)",
    },
    UnitSuggestion {
        code: "lb",
        label: "pounds (weight)",
    },
    UnitSuggestion {
        code: "g",
        label: "grams",
    },
    UnitSuggestion {
        code: "s",
        label: "seconds",
    },
    UnitSuggestion {
        code: "min",
        label: "minutes",
    },
    UnitSuggestion {
        code: "h",
        label: "hours",
    },
    UnitSuggestion {
        code: "L",
        label: "liters",
    },
    UnitSuggestion {
        code: "mL",
        label: "milliliters",
    },
];

/// Canonical unit code to persist (empty string when absent).
pub fn normalize_unit(unit: Option<&str>) -> Option<String> {
    match classify_unit(unit) {
        UnitClass::None => None,
        UnitClass::Ucum { canonical, .. } => Some(canonical),
        UnitClass::Custom(label) => Some(label),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_ucum_length() {
        let class = classify_unit(Some("cm"));
        assert!(matches!(
            class,
            UnitClass::Ucum {
                dimension: UnitDimension::Length,
                ..
            }
        ));
    }

    #[test]
    fn custom_unit_for_game_labels() {
        let class = classify_unit(Some("stress"));
        assert!(matches!(class, UnitClass::Custom(ref s) if s == "stress"));
    }

    #[test]
    fn common_imperial_aliases_resolve_to_ucum() {
        for (input, expected) in [
            ("ft", "[ft_i]"),
            ("feet", "[ft_i]"),
            ("in", "[in_i]"),
            ("inches", "[in_i]"),
            ("lb", "[lb_av]"),
            ("lbs", "[lb_av]"),
            ("pounds", "[lb_av]"),
        ] {
            let class = classify_unit(Some(input));
            assert!(
                matches!(class, UnitClass::Ucum { ref canonical, .. } if canonical == expected),
                "{input} should canonicalize to {expected}, got {class:?}"
            );
        }
    }

    #[test]
    fn bare_ft_is_not_femtotonne() {
        let class = classify_unit(Some("ft"));
        assert!(matches!(
            class,
            UnitClass::Ucum {
                dimension: UnitDimension::Length,
                ..
            }
        ));
    }

    #[test]
    fn normalize_unit_maps_aliases() {
        assert_eq!(normalize_unit(Some("ft")), Some("[ft_i]".to_string()));
        assert_eq!(normalize_unit(Some("lb")), Some("[lb_av]".to_string()));
        assert_eq!(normalize_unit(Some("stress")), Some("stress".to_string()));
    }

    #[test]
    fn friendly_unit_label_reverses_canonical() {
        assert_eq!(friendly_unit_label("[ft_i]"), "ft");
        assert_eq!(friendly_unit_label("[lb_av]"), "lb");
        assert_eq!(friendly_unit_label("stress"), "stress");
    }

    #[test]
    fn bracketless_ucum_aliases_resolve() {
        for (input, expected) in [("in_i", "[in_i]"), ("ft_i", "[ft_i]"), ("lb_av", "[lb_av]")] {
            let class = classify_unit(Some(input));
            assert!(
                matches!(class, UnitClass::Ucum { ref canonical, .. } if canonical == expected),
                "{input} should canonicalize to {expected}, got {class:?}"
            );
        }
        let display = format_measurement_value(71.0, Some("in_i"));
        assert_eq!(display.primary, "5′11″");
    }

    #[test]
    fn scenario_unit_suggestions_are_nonempty() {
        for suggestion in SCENARIO_UNIT_SUGGESTIONS {
            assert!(!suggestion.code.is_empty());
            assert!(!suggestion.label.is_empty());
        }
    }
}
