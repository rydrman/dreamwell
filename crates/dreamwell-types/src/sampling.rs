use serde::{Deserialize, Serialize};

/// Per-model temperature and top_p saved in settings. Applied whenever that model is used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelSamplingProfile {
    pub model: String,
    pub temperature: f64,
    pub top_p: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SamplingParams {
    pub temperature: f64,
    pub top_p: f64,
}

/// Optional phase- or context-specific overrides (highest priority).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SamplingOverrides {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
}

impl SamplingOverrides {
    pub fn is_empty(self) -> bool {
        self.temperature.is_none() && self.top_p.is_none()
    }
}

/// Resolve sampling: connection defaults → model profile → phase overrides.
pub fn resolve_sampling(
    conn_temperature: f64,
    conn_top_p: f64,
    model: &str,
    profiles: &[ModelSamplingProfile],
    phase: Option<SamplingOverrides>,
) -> SamplingParams {
    let mut temperature = conn_temperature;
    let mut top_p = conn_top_p;

    let model_key = model.trim();
    if !model_key.is_empty() {
        if let Some(profile) = profiles
            .iter()
            .find(|profile| profile.model.trim() == model_key)
        {
            temperature = profile.temperature;
            top_p = profile.top_p;
        }
    }

    if let Some(phase) = phase {
        if let Some(t) = phase.temperature {
            temperature = t;
        }
        if let Some(p) = phase.top_p {
            top_p = p;
        }
    }

    SamplingParams { temperature, top_p }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_sampling_uses_connection_by_default() {
        let params = resolve_sampling(0.8, 0.9, "demo", &[], None);
        assert_eq!(params.temperature, 0.8);
        assert_eq!(params.top_p, 0.9);
    }

    #[test]
    fn resolve_sampling_applies_model_profile() {
        let profiles = vec![ModelSamplingProfile {
            model: "creative".into(),
            temperature: 1.1,
            top_p: 0.95,
        }];
        let params = resolve_sampling(0.8, 0.9, "creative", &profiles, None);
        assert_eq!(params.temperature, 1.1);
        assert_eq!(params.top_p, 0.95);
    }

    #[test]
    fn resolve_sampling_phase_overrides_profile() {
        let profiles = vec![ModelSamplingProfile {
            model: "creative".into(),
            temperature: 1.1,
            top_p: 0.95,
        }];
        let params = resolve_sampling(
            0.8,
            0.9,
            "creative",
            &profiles,
            Some(SamplingOverrides {
                temperature: Some(0.3),
                top_p: None,
            }),
        );
        assert_eq!(params.temperature, 0.3);
        assert_eq!(params.top_p, 0.95);
    }
}
