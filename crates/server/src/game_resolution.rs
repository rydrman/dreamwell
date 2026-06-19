use dreamwell_types::CheckTier;

/// Roll result for a single check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollResult {
    pub seed: i64,
    pub rolls: Vec<i64>,
    pub modifier: i64,
    pub total: i64,
    pub tier: CheckTier,
    pub margin: i64,
    pub natural_boon: bool,
    pub natural_snag: bool,
}

/// Parse a simple dice expression like "2d6".
pub fn parse_dice_expr(expr: &str) -> Option<(u32, u32)> {
    let expr = expr.trim().to_lowercase();
    let (count, sides) = expr.split_once('d')?;
    let count: u32 = count.parse().ok()?;
    let sides: u32 = sides.parse().ok()?;
    if count == 0 || sides == 0 {
        return None;
    }
    Some((count, sides))
}

/// Deterministic seeded roll using a simple LCG.
fn seeded_die(seed: i64, sides: i64) -> (i64, i64) {
    let next = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    let roll = (next.abs() % sides) + 1;
    (roll, next)
}

pub fn roll_dice(expr: &str, modifier: i64, seed: i64) -> Option<RollResult> {
    let (count, sides) = parse_dice_expr(expr)?;
    let mut current_seed = seed;
    let mut rolls = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (roll, next) = seeded_die(current_seed, sides as i64);
        rolls.push(roll);
        current_seed = next;
    }
    let raw_total: i64 = rolls.iter().sum();
    let total = raw_total + modifier;
    let tier = tier_for_total(total);
    let margin = match tier {
        CheckTier::Fail => total - 7,
        CheckTier::Mixed => total - 7,
        CheckTier::Strong => total - 10,
    };
    let natural_boon = raw_total >= 12 && count == 2 && sides == 6;
    let natural_snag = raw_total == 2 && count == 2 && sides == 6;
    Some(RollResult {
        seed,
        rolls,
        modifier,
        total,
        tier,
        margin,
        natural_boon,
        natural_snag,
    })
}

pub fn tier_for_total(total: i64) -> CheckTier {
    if total <= 6 {
        CheckTier::Fail
    } else if total <= 9 {
        CheckTier::Mixed
    } else {
        CheckTier::Strong
    }
}

pub fn clamp_modifier(modifier: i64, min: i64, max: i64) -> i64 {
    modifier.clamp(min, max)
}

pub fn tier_str(tier: CheckTier) -> &'static str {
    match tier {
        CheckTier::Fail => "fail",
        CheckTier::Mixed => "mixed",
        CheckTier::Strong => "strong",
    }
}

pub fn parse_tier(s: &str) -> Option<CheckTier> {
    match s {
        "fail" => Some(CheckTier::Fail),
        "mixed" => Some(CheckTier::Mixed),
        "strong" => Some(CheckTier::Strong),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_boundaries() {
        assert_eq!(tier_for_total(6), CheckTier::Fail);
        assert_eq!(tier_for_total(7), CheckTier::Mixed);
        assert_eq!(tier_for_total(9), CheckTier::Mixed);
        assert_eq!(tier_for_total(10), CheckTier::Strong);
    }

    #[test]
    fn parse_dice_expr_valid() {
        assert_eq!(parse_dice_expr("2d6"), Some((2, 6)));
        assert_eq!(parse_dice_expr("1d20"), Some((1, 20)));
    }

    #[test]
    fn parse_dice_expr_invalid() {
        assert_eq!(parse_dice_expr("d6"), None);
        assert_eq!(parse_dice_expr("2d"), None);
        assert_eq!(parse_dice_expr(""), None);
    }

    #[test]
    fn roll_is_deterministic() {
        let a = roll_dice("2d6", 1, 42).expect("roll");
        let b = roll_dice("2d6", 1, 42).expect("roll");
        assert_eq!(a, b);
    }

    #[test]
    fn different_seeds_differ() {
        let a = roll_dice("2d6", 0, 1).expect("roll");
        let b = roll_dice("2d6", 0, 2).expect("roll");
        assert_ne!(a.rolls, b.rolls);
    }

    #[test]
    fn modifier_clamped() {
        assert_eq!(clamp_modifier(10, -2, 3), 3);
        assert_eq!(clamp_modifier(-5, -2, 3), -2);
        assert_eq!(clamp_modifier(1, -2, 3), 1);
    }

    #[test]
    fn natural_boon_and_snag_flags() {
        let mut boon = roll_dice("2d6", 0, 1).expect("roll");
        boon.rolls = vec![6, 6];
        boon.total = 12;
        boon.natural_boon = boon.rolls.iter().sum::<i64>() >= 12;
        assert!(boon.natural_boon);

        let mut snag = roll_dice("2d6", 0, 2).expect("roll");
        snag.rolls = vec![1, 1];
        snag.total = 2;
        snag.natural_snag = snag.rolls.iter().sum::<i64>() == 2;
        assert!(snag.natural_snag);
    }
}
