//! Rex's onboarding walkthrough: a block the server keeps the step of, which clients draw.
use serde_json::{Value, json};

pub const CREATOR: &str = "com.superlogical.onboarding";
pub const FLAVOR: &str = "com.superlogical.onboarding.onboarding";

/// A new onboarding block, starting at the first step.
pub fn layout() -> Value {
    json!({"block": {"flavor": FLAVOR}})
}

/// The walkthrough's steps, in order. The server only keeps which one is current; an
/// empty step means the first.
pub const STEPS: [&str; 5] = ["welcome", "look", "keys", "agents", "done"];

/// The index of `step` in [`STEPS`]; the first for an empty or unknown step.
pub fn index(step: &str) -> usize {
    STEPS.iter().position(|s| *s == step).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_start_at_the_beginning() {
        assert_eq!(index(""), 0);
        assert_eq!(index("agents"), 3);
        assert_eq!(index("something newer"), 0);
    }
}
