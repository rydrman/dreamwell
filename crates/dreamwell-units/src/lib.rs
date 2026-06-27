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

/// Parse an optional unit string into a normalized class for storage and UI.
pub fn classify_unit(unit: Option<&str>) -> UnitClass {
    let Some(raw) = unit.map(str::trim).filter(|s| !s.is_empty()) else {
        return UnitClass::None;
    };
    if ucum::validate(raw).is_ok() {
        let canonical = ucum::canonical(raw).unwrap_or_else(|_| raw.to_string());
        let display_name = ucum::display_name(raw).unwrap_or_else(|_| raw.to_string());
        let dimension = ucum::analyze(raw)
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
    fn empty_unit_is_none() {
        assert!(matches!(classify_unit(None), UnitClass::None));
        assert!(matches!(classify_unit(Some("")), UnitClass::None));
    }
}
