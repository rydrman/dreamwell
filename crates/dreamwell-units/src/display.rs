use crate::{classify_unit, friendly_unit_label, UnitClass, UnitDimension};

/// Human-readable measurement text for UI (primary + optional converted alternate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasurementDisplay {
    pub primary: String,
    pub secondary: Option<String>,
}

impl MeasurementDisplay {
    pub fn combined(&self) -> String {
        match &self.secondary {
            Some(alt) => format!("{} · {}", self.primary, alt),
            None => self.primary.clone(),
        }
    }
}

const LB_PER_KG: f64 = 2.204_622_621_8;
const CM_PER_INCH: f64 = 2.54;
/// Use feet′inches″ only at human-scale lengths; smaller values stay decimal in/cm.
const COMPOUND_IMPERIAL_INCHES: f64 = 36.0;

fn prefer_compound_imperial(total_inches: f64) -> bool {
    total_inches >= COMPOUND_IMPERIAL_INCHES
}

fn sensible_decimals(value: f64) -> u32 {
    if (value - value.round()).abs() < 1e-9 {
        0
    } else if (value * 10.0 - (value * 10.0).round()).abs() < 1e-9 {
        1
    } else {
        2
    }
}

/// Format a total inch count as feet and inches (e.g. 71 → 5′11″).
pub fn format_feet_inches(total_inches: f64) -> String {
    let inches_total = total_inches.round();
    let mut feet = (inches_total / 12.0).floor() as i64;
    let mut inches = (inches_total - feet as f64 * 12.0).round() as i64;
    if inches == 12 {
        feet += 1;
        inches = 0;
    }
    if inches == 0 {
        format!("{}′", feet)
    } else {
        format!("{}′{}″", feet, inches)
    }
}

fn trim_float(value: f64, max_decimals: u32) -> String {
    let rounded = round_for_display(value, max_decimals);
    let formatted = match max_decimals {
        0 => format!("{:.0}", rounded),
        1 => format!("{:.1}", rounded),
        2 => format!("{:.2}", rounded),
        _ => format!("{rounded}"),
    };
    if formatted.contains('.') {
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        formatted
    }
}

fn round_for_display(value: f64, max_decimals: u32) -> f64 {
    let factor = 10_f64.powi(max_decimals as i32);
    (value * factor).round() / factor
}

fn format_cm_from_inches(inches: f64) -> String {
    let cm = inches * CM_PER_INCH;
    let decimals = if cm >= 100.0 {
        0
    } else {
        sensible_decimals(inches).max(1)
    };
    with_unit(round_for_display(cm, decimals), "cm", decimals)
}

fn with_unit(value: f64, unit_label: &str, max_decimals: u32) -> String {
    if unit_label == "%" {
        format!("{}%", trim_float(value, max_decimals))
    } else if unit_label.starts_with('°') {
        format!("{}{}", trim_float(value, max_decimals), unit_label)
    } else {
        format!("{} {}", trim_float(value, max_decimals), unit_label)
    }
}

fn total_inches(value: f64, canonical: &str) -> Option<f64> {
    Some(match canonical {
        "[in_i]" => value,
        "[ft_i]" => value * 12.0,
        "cm" => value / CM_PER_INCH,
        "m" => value * 100.0 / CM_PER_INCH,
        "[yd_i]" => value * 36.0,
        "[mi_us]" => value * 63_360.0,
        _ => return None,
    })
}

fn length_pair(value: f64, canonical: &str) -> (String, Option<String>) {
    let Some(inches) = total_inches(value, canonical) else {
        let label = friendly_unit_label(canonical);
        return (with_unit(value, label, sensible_decimals(value)), None);
    };
    match canonical {
        "cm" | "m" => {
            let label = friendly_unit_label(canonical);
            let primary = with_unit(value, label, sensible_decimals(value).max(1));
            let secondary = if prefer_compound_imperial(inches) {
                Some(format_feet_inches(inches))
            } else {
                Some(with_unit(inches, "in", sensible_decimals(inches)))
            };
            (primary, secondary)
        }
        "[in_i]" => {
            let primary = if prefer_compound_imperial(inches) {
                format_feet_inches(inches)
            } else {
                with_unit(value, "in", sensible_decimals(value))
            };
            (primary, Some(format_cm_from_inches(inches)))
        }
        "[ft_i]" | "[yd_i]" | "[mi_us]" => {
            let primary = if prefer_compound_imperial(inches) {
                format_feet_inches(inches)
            } else {
                with_unit(
                    value,
                    friendly_unit_label(canonical),
                    sensible_decimals(value),
                )
            };
            (primary, Some(format_cm_from_inches(inches)))
        }
        _ => (
            with_unit(
                value,
                friendly_unit_label(canonical),
                sensible_decimals(value),
            ),
            None,
        ),
    }
}

fn mass_pair(value: f64, canonical: &str) -> (String, Option<String>) {
    match canonical {
        "kg" => (
            with_unit(value, "kg", 1),
            Some(with_unit(value * LB_PER_KG, "lb", 0)),
        ),
        "[lb_av]" => (
            with_unit(value, "lb", 0),
            Some(with_unit(value / LB_PER_KG, "kg", 1)),
        ),
        "g" => (
            with_unit(value, "g", 0),
            Some(with_unit(value / 1000.0, "kg", 2)),
        ),
        "[oz_av]" => (
            with_unit(value, "oz", 1),
            Some(with_unit(value / LB_PER_KG, "kg", 2)),
        ),
        _ => (with_unit(value, friendly_unit_label(canonical), 1), None),
    }
}

fn temperature_pair(value: f64, canonical: &str) -> (String, Option<String>) {
    match canonical {
        "Cel" => (
            with_unit(value, "°C", 1),
            Some(with_unit(value * 9.0 / 5.0 + 32.0, "°F", 0)),
        ),
        "[degF]" => (
            with_unit(value, "°F", 0),
            Some(with_unit((value - 32.0) * 5.0 / 9.0, "°C", 1)),
        ),
        _ => (with_unit(value, friendly_unit_label(canonical), 1), None),
    }
}

fn format_scalar(value: f64, unit: Option<&str>) -> (String, Option<String>) {
    let class = classify_unit(unit);
    match class {
        UnitClass::None => (trim_float(value, 2), None),
        UnitClass::Custom(label) => (with_unit(value, &label, 2), None),
        UnitClass::Ucum {
            canonical,
            dimension,
            ..
        } => match dimension {
            UnitDimension::Length => length_pair(value, &canonical),
            UnitDimension::Mass => mass_pair(value, &canonical),
            UnitDimension::Temperature => temperature_pair(value, &canonical),
            UnitDimension::Ratio if canonical == "%" => (with_unit(value, "%", 1), None),
            UnitDimension::Dimensionless if canonical == "1" => (trim_float(value, 2), None),
            _ => (with_unit(value, friendly_unit_label(&canonical), 2), None),
        },
    }
}

/// Format one measurement value with friendly units and common dual conversions.
pub fn format_measurement_value(value: f64, unit: Option<&str>) -> MeasurementDisplay {
    let (primary, secondary) = format_scalar(value, unit);
    MeasurementDisplay { primary, secondary }
}

/// Format a measurement entry, including an optional maximum for gauges.
pub fn format_measurement_display(
    value: f64,
    max: Option<f64>,
    unit: Option<&str>,
) -> MeasurementDisplay {
    let current = format_measurement_value(value, unit);
    let Some(max) = max else {
        return current;
    };
    let max_display = format_measurement_value(max, unit);
    MeasurementDisplay {
        primary: format!("{} / {}", current.primary, max_display.primary),
        secondary: merge_secondaries(current.secondary, max_display.secondary),
    }
}

fn merge_secondaries(current: Option<String>, max: Option<String>) -> Option<String> {
    match (current, max) {
        (Some(c), Some(m)) => Some(format!("{} / {}", c, m)),
        (Some(c), None) => Some(c),
        (None, Some(m)) => Some(m),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inches_render_as_feet_and_inches_with_cm_alternate() {
        let display = format_measurement_value(71.0, Some("[in_i]"));
        assert_eq!(display.primary, "5′11″");
        assert_eq!(display.secondary.as_deref(), Some("180 cm"));
    }

    #[test]
    fn small_inches_stay_decimal_not_feet_inches() {
        let display = format_measurement_value(6.5, Some("[in_i]"));
        assert_eq!(display.primary, "6.5 in");
        assert_eq!(display.secondary.as_deref(), Some("16.5 cm"));
    }

    #[test]
    fn small_centimeters_show_decimal_inches_alternate() {
        let display = format_measurement_value(16.5, Some("cm"));
        assert_eq!(display.primary, "16.5 cm");
        assert_eq!(display.secondary.as_deref(), Some("6.5 in"));
    }

    #[test]
    fn just_below_compound_threshold_stays_decimal() {
        let display = format_measurement_value(35.0, Some("[in_i]"));
        assert_eq!(display.primary, "35 in");
        assert!(display.secondary.as_ref().is_some_and(|s| s.contains("cm")));
    }

    #[test]
    fn at_compound_threshold_uses_feet_inches() {
        let display = format_measurement_value(36.0, Some("[in_i]"));
        assert_eq!(display.primary, "3′");
    }

    #[test]
    fn centimeters_show_metric_primary_and_imperial_secondary() {
        let display = format_measurement_value(180.0, Some("cm"));
        assert_eq!(display.primary, "180 cm");
        assert_eq!(display.secondary.as_deref(), Some("5′11″"));
    }

    #[test]
    fn kilograms_dual_render_with_pounds() {
        let display = format_measurement_value(82.0, Some("kg"));
        assert_eq!(display.primary, "82 kg");
        assert!(display.secondary.as_ref().is_some_and(|s| s.contains("lb")));
    }

    #[test]
    fn pounds_dual_render_with_kilograms() {
        let display = format_measurement_value(160.0, Some("[lb_av]"));
        assert_eq!(display.primary, "160 lb");
        assert!(display.secondary.as_ref().is_some_and(|s| s.contains("kg")));
    }

    #[test]
    fn bounded_measurement_formats_both_ends() {
        let display = format_measurement_display(3.0, Some(5.0), Some("kg"));
        assert_eq!(display.primary, "3 kg / 5 kg");
        assert!(display.secondary.is_some());
    }

    #[test]
    fn custom_units_stay_simple() {
        let display = format_measurement_value(2.5, Some("stress"));
        assert_eq!(display.primary, "2.5 stress");
        assert!(display.secondary.is_none());
    }
}
