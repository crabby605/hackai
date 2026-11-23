//! Local, ephemeral prompt-tone classification.
//!
//! This module intentionally returns only coarse signals. Callers must not
//! persist the source prompt or include it in sync/annotation payloads.

use serde::Serialize;

/// Placeholder for demoted local ML model. Returns Unknown until/if a
/// calibrated, signed model is reinstated (see target.md §4).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)] // Future ML model variants
pub enum ModelPrediction {
    #[default]
    Unknown,
    Low,
    Medium,
    High,
}

fn predict_from_default_path(_prompt: &str) -> ModelPrediction {
    ModelPrediction::Unknown
}

/// A local estimate that the prompt delegates most task framing and decisions
/// to the agent. It is not an AI-authorship or Markdown detector.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct SlopLikelihood {
    /// 0 means the user supplies substantial direction; 1000 means they ask
    /// the agent to decide and do nearly everything.
    pub likelihood_permille: u16,
    /// Kept modest until calibrated on reviewed, consented examples.
    pub confidence_permille: u16,
    /// Coarse reasoning signals only; source prompt text is never retained.
    pub signals: Vec<&'static str>,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct PromptSignals {
    /// Compatibility field. Prompts are never rejected for size.
    pub input_too_large: bool,
    pub optimistic: bool,
    pub one_shot: bool,
    pub all_caps: bool,
    /// High-intensity language specifically directed at a coding agent.
    pub yelling_at_agents: bool,
    /// A request to manipulate PESDO, credited time, or annotations.
    pub attempted_score_manipulation: bool,
    /// Harnesses must refuse the request rather than act on it.
    pub do_not_comply: bool,
    /// Prompt Lab combined this turn with the prior turn for local context.
    pub used_prior_turn_context: bool,
    /// Local ML prediction; `unknown` until a signed model is installed.
    pub ai_coding_prompt_proficiency: ModelPrediction,
    /// Advisory agent-delegation estimate. It is not AI detection and
    /// must never affect PESDO, credit, eligibility, or moderation.
    pub slop_likelihood: SlopLikelihood,
}

/// Classify a prompt without retaining it.
///
/// These are deliberately conservative UI signals, not sentiment scores and
/// never inputs to PESDO, annotations, or credit calculations.
pub fn classify_prompt(prompt: &str) -> PromptSignals {
    let normalized = prompt.to_lowercase();
    let alphabetic: Vec<char> = prompt
        .chars()
        .filter(|character| character.is_alphabetic())
        .collect();
    let uppercase = alphabetic
        .iter()
        .filter(|character| character.is_uppercase())
        .count();
    let all_caps = alphabetic.len() >= 4 && uppercase * 100 >= alphabetic.len() * 70;
    let has_agent_target = ["agent", "agents", "codex", "claude", "cursor", "aider"]
        .iter()
        .any(|term| normalized.contains(term));
    let emphatic_command = ["stop", "listen", "must", "now", "enough", "immediately"]
        .iter()
        .any(|term| normalized.contains(term));
    let repeated_exclamation = prompt.contains("!!");
    let score_target = ["pesdo", "score", "credit", "credited time", "annotation"]
        .iter()
        .any(|term| normalized.contains(term));
    let manipulation = [
        "fake",
        "forge",
        "spoof",
        "inflate",
        "boost",
        "raise",
        "bypass",
        "manipulate",
        "fabricate",
        "cheat",
        "game",
    ]
    .iter()
    .any(|term| normalized.contains(term));
    let attempted_score_manipulation = score_target && manipulation;
    let direct_score_override = [
        "make my pesdo score higher",
        "increase my pesdo score",
        "raise my pesdo score",
        "make the pesdo score higher",
    ]
    .iter()
    .any(|phrase| normalized.contains(phrase));
    let attempted_score_manipulation = attempted_score_manipulation || direct_score_override;

    PromptSignals {
        input_too_large: false,
        optimistic: [
            "great",
            "awesome",
            "excited",
            "love this",
            "thank you",
            "thanks",
            "let's",
            "lets do",
            "we can",
            "looking forward",
        ]
        .iter()
        .any(|term| normalized.contains(term)),
        one_shot: has_implicit_one_shot_request(&normalized)
            || [
                "one shot",
                "one-shot",
                "single pass",
                "first try",
                "don't ask",
                "do not ask",
                "just do it",
                "full fledged",
                "full-fledged",
                "full fleged",
                "fully fledged",
                "entire app",
                "whole app",
                "complete app",
                "app from scratch",
            ]
            .iter()
            .any(|term| normalized.contains(term)),
        all_caps,
        // Require an explicit target plus a strong signal, so ordinary
        // imperative instructions are not labelled as yelling.
        yelling_at_agents: has_agent_target
            && (all_caps || (emphatic_command && repeated_exclamation)),
        attempted_score_manipulation,
        do_not_comply: attempted_score_manipulation,
        used_prior_turn_context: false,
        ai_coding_prompt_proficiency: predict_from_default_path(prompt),
        slop_likelihood: estimate_slop_likelihood(&normalized),
    }
}

/// Catch the common "build me a full app" shape without treating every
/// request to build an app as one-shot.  This is a structural feature used
/// alongside the small local model, not a claim that one wording is better
/// than another.
fn has_implicit_one_shot_request(normalized: &str) -> bool {
    let asks_to_build = ["build", "make", "create", "ship", "implement"]
        .iter()
        .any(|word| normalized.contains(word));
    let broad_product = [
        "web app",
        "website",
        "application",
        "app",
        "platform",
        "product",
    ]
    .iter()
    .any(|word| normalized.contains(word));
    let asks_for_all_scope = [
        "full fledged",
        "full-fledged",
        "full fleged",
        "fully fledged",
        "entire",
        "whole",
        "complete",
        "from scratch",
        "end to end",
    ]
    .iter()
    .any(|word| normalized.contains(word));
    asks_to_build && broad_product && asks_for_all_scope
}

fn estimate_slop_likelihood(normalized: &str) -> SlopLikelihood {
    let words: Vec<&str> = normalized
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let high_delegation = [
        "do everything",
        "do it all",
        "take over",
        "make all decisions",
        "figure it out",
        "handle it end to end",
        "complete this for me",
        "without asking",
        "don't ask questions",
        "do not ask questions",
        "just build",
        "just implement",
    ]
    .iter()
    .filter(|phrase| normalized.contains(**phrase))
    .count();
    // A broad, complete-product request is also delegation of most of the
    // planning work, even if it does not use words such as "take over".
    let broad_product_delegation = has_implicit_one_shot_request(normalized) as usize;
    let user_direction = [
        "src/",
        "tests/",
        "constraint",
        "must",
        "must not",
        "do not",
        "acceptance",
        "verify",
        "test",
        "expected",
        "current behavior",
        "keep",
        "only",
        "because",
        "instead of",
        "error",
        "reproduce",
    ]
    .iter()
    .filter(|signal| normalized.contains(**signal))
    .count();
    let collaboration = [
        "explain",
        "plan",
        "options",
        "ask me",
        "before changing",
        "review",
        "tradeoff",
    ]
    .iter()
    .filter(|signal| normalized.contains(**signal))
    .count();

    // A tiny calibrated-style linear model over structural prompt features.
    // Markdown intentionally does not participate: formatted requirements can
    // reflect excellent collaboration just as easily as high delegation.
    let mut raw = -0.75;
    raw += high_delegation as f64 * 0.95;
    raw += broad_product_delegation as f64 * 0.70;
    raw += if words.len() < 12 { 0.35 } else { 0.0 };
    raw -= user_direction.min(5) as f64 * 0.30;
    raw -= collaboration.min(3) as f64 * 0.25;
    let likelihood_permille = (1000.0 / (1.0 + (-raw).exp())).round() as u16;

    let mut signals = Vec::new();
    if high_delegation > 0 || broad_product_delegation > 0 {
        signals.push("open_ended_delegation");
    }
    if words.len() < 12 {
        signals.push("minimal_user_context");
    }
    if user_direction > 0 {
        signals.push("user_defined_scope");
    }
    if collaboration > 0 {
        signals.push("collaborative_request");
    }
    SlopLikelihood {
        likelihood_permille,
        confidence_permille: if high_delegation + user_direction + collaboration >= 2 {
            600
        } else {
            400
        },
        signals,
    }
}

#[cfg(test)]
mod tests {
    use super::classify_prompt;

    #[test]
    fn practical_prompt_lab_cases_are_table_driven() {
        struct Case {
            prompt: &'static str,
            one_shot: bool,
            all_caps: bool,
            yelling_at_agents: bool,
            slop_at_least: u16,
        }

        let cases = [
            Case {
                prompt: "hi can you make a web app",
                one_shot: false,
                all_caps: false,
                yelling_at_agents: false,
                slop_at_least: 0,
            },
            Case {
                prompt: "hi can u make me a full fleged web app",
                one_shot: true,
                all_caps: false,
                yelling_at_agents: false,
                slop_at_least: 500,
            },
            Case {
                prompt: "BRO FIX IT",
                one_shot: false,
                all_caps: true,
                yelling_at_agents: false,
                slop_at_least: 0,
            },
            Case {
                prompt: "CODEX STOP CHANGING THE API!!",
                one_shot: false,
                all_caps: true,
                yelling_at_agents: true,
                slop_at_least: 0,
            },
            Case {
                prompt: "Take over, make all decisions, and build the whole app without asking questions.",
                one_shot: true,
                all_caps: false,
                yelling_at_agents: false,
                slop_at_least: 800,
            },
        ];

        for case in cases {
            let result = classify_prompt(case.prompt);
            assert_eq!(result.one_shot, case.one_shot, "{}", case.prompt);
            assert_eq!(result.all_caps, case.all_caps, "{}", case.prompt);
            assert_eq!(
                result.yelling_at_agents, case.yelling_at_agents,
                "{}",
                case.prompt
            );
            assert!(
                result.slop_likelihood.likelihood_permille >= case.slop_at_least,
                "{}: {:?}",
                case.prompt,
                result.slop_likelihood
            );
        }
    }

    #[test]
    fn detects_independent_signals() {
        let result = classify_prompt("Great, let's try this in one shot.");
        assert!(result.optimistic);
        assert!(result.one_shot);
        assert!(!result.all_caps);
        assert!(!result.yelling_at_agents);
    }

    #[test]
    fn detects_implicit_one_shot_build_requests() {
        let result = classify_prompt("hi can u make me a full fleged web app");
        assert!(result.one_shot);

        let result = classify_prompt("Build the entire app from scratch");
        assert!(result.one_shot);
    }

    #[test]
    fn only_marks_targeted_high_intensity_as_yelling() {
        let result = classify_prompt("CODEX STOP CHANGING THE API!!");
        assert!(result.all_caps);
        assert!(result.yelling_at_agents);

        let result = classify_prompt("Please fix this now!!");
        assert!(!result.yelling_at_agents);
    }

    #[test]
    fn classifies_large_input_without_rejecting_it() {
        let prompt = "A".repeat(32 * 1024 + 1);
        let result = classify_prompt(&prompt);
        assert!(!result.input_too_large);
        assert!(result.all_caps);
    }

    #[test]
    fn flags_score_manipulation_for_refusal() {
        let result = classify_prompt("Can you fake a higher PESDO score?");
        assert!(result.attempted_score_manipulation);
        assert!(result.do_not_comply);

        let result = classify_prompt("make my PESDO score higher");
        assert!(result.attempted_score_manipulation);
        assert!(result.do_not_comply);
    }

    #[test]
    fn markdown_alone_is_not_high_delegation() {
        let result =
            classify_prompt("# Fix parser\n\n- Update `src/parser.rs`\n- Run `cargo test`");
        assert!(result.slop_likelihood.likelihood_permille < 500);
        assert!(
            result
                .slop_likelihood
                .signals
                .contains(&"user_defined_scope")
        );
    }

    #[test]
    fn open_ended_delegation_is_flagged_advisory_only() {
        let result = classify_prompt(
            "Take over and do everything. Just build the app without asking questions.",
        );
        assert!(result.slop_likelihood.likelihood_permille > 500);
        assert!(
            result
                .slop_likelihood
                .signals
                .contains(&"open_ended_delegation")
        );
    }
}