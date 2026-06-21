use dreamwell_types::SessionActor;

/// Normalize wire target names (`user` → `pc`).
pub fn normalize_target(target: &str) -> &str {
    match target {
        "user" => "pc",
        other => other,
    }
}

/// Whether an unknown non-world target should auto-vivify an NPC row.
pub fn should_vivify_actor(target: &str) -> bool {
    let normalized = normalize_target(target);
    normalized != "world" && normalized != "pc"
}

pub fn resolve_actor_id(target: &str, actors: &[SessionActor]) -> Option<i64> {
    match normalize_target(target) {
        "world" => None,
        "pc" => actors.iter().find(|a| a.role == "pc").map(|a| a.id),
        other => actors
            .iter()
            .find(|a| a.role == other || a.name.eq_ignore_ascii_case(other))
            .map(|a| a.id),
    }
}

pub fn validate_skill(skill: &str, _actor: &SessionActor) -> String {
    skill.to_string()
}

pub fn skill_modifier(skill: &str, actor: &SessionActor) -> i64 {
    actor.skills.get(skill).copied().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn pc_actor() -> SessionActor {
        SessionActor {
            id: 1,
            game_id: 1,
            role: "pc".into(),
            name: "Alex".into(),
            description: String::new(),
            skills: Default::default(),
            sort_order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn user_alias_resolves_to_pc() {
        assert_eq!(resolve_actor_id("user", &[pc_actor()]), Some(1));
    }

    #[test]
    fn unknown_name_needs_vivify() {
        assert!(should_vivify_actor("Alice"));
        assert!(!should_vivify_actor("world"));
        assert!(!should_vivify_actor("pc"));
        assert!(!should_vivify_actor("user"));
    }
}
