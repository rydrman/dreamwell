use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SequencePayload {
    pub items: Vec<String>,
    pub position: i64,
    #[serde(default)]
    pub r#loop: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepSequenceResult {
    Ok,
    RejectedEmpty,
    AtEnd,
}

impl SequencePayload {
    pub fn new(items: Vec<String>, position: Option<i64>, r#loop: bool) -> Option<Self> {
        if items.is_empty() {
            return None;
        }
        let len = items.len() as i64;
        let position = position.unwrap_or(0).clamp(0, len - 1);
        Some(Self {
            items,
            position,
            r#loop,
        })
    }

    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn decode(raw: &str) -> Option<Self> {
        let payload: Self = serde_json::from_str(raw).ok()?;
        if payload.items.is_empty() {
            return None;
        }
        Some(payload)
    }

    pub fn active_item(&self) -> Option<&str> {
        usize::try_from(self.position)
            .ok()
            .and_then(|idx| self.items.get(idx))
            .map(String::as_str)
    }

    pub fn step(&mut self, delta: i64) -> StepSequenceResult {
        if self.items.is_empty() {
            return StepSequenceResult::RejectedEmpty;
        }
        let len = self.items.len() as i64;
        let prev = self.position;
        if self.r#loop {
            let wrapped = ((prev + delta) % len + len) % len;
            self.position = wrapped;
            return StepSequenceResult::Ok;
        }
        let clamped = (prev + delta).clamp(0, len - 1);
        self.position = clamped;
        if (delta > 0 && prev >= len - 1) || (delta < 0 && prev <= 0) {
            StepSequenceResult::AtEnd
        } else {
            StepSequenceResult::Ok
        }
    }
}

pub fn clamp_measurement(value: f64, min: Option<f64>, max: Option<f64>) -> f64 {
    let mut v = value;
    if let Some(min) = min {
        v = v.max(min);
    }
    if let Some(max) = max {
        v = v.min(max);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_rejects_empty_items() {
        assert!(SequencePayload::new(vec![], None, false).is_none());
    }

    #[test]
    fn sequence_loops_when_enabled() {
        let mut seq = SequencePayload::new(vec!["a".into(), "b".into()], Some(1), true).unwrap();
        assert_eq!(seq.step(1), StepSequenceResult::Ok);
        assert_eq!(seq.position, 0);
    }

    #[test]
    fn sequence_clamps_without_loop() {
        let mut seq = SequencePayload::new(vec!["a".into(), "b".into()], Some(1), false).unwrap();
        assert_eq!(seq.step(1), StepSequenceResult::AtEnd);
        assert_eq!(seq.position, 1);
    }
}
