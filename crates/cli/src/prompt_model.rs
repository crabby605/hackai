//! Small local inference engine for a server-signed prompt model.
//! Raw prompts never leave this process.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct ModelPrediction {
    pub model_version: Option<String>,
    pub level: String,
    pub confidence_permille: Option<u16>,
}

impl Default for ModelPrediction {
    fn default() -> Self {
        Self {
            model_version: None,
            level: "unknown".into(),
            confidence_permille: None,
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct Model {
    version: String,
    labels: [String; 3],
    bias: [f64; 3],
    weights: BTreeMap<String, [f64; 3]>,
    #[serde(default)]
    trained_on_synthetic: bool,
}

#[derive(Deserialize)]
struct TrainingExample {
    prompt: String,
    label: String,
}

#[derive(Serialize)]
pub struct TrainingResult {
    pub examples: usize,
    pub training_accuracy_permille: u16,
    pub trained_on_synthetic: bool,
}

#[derive(Serialize)]
pub struct DatasetGenerationResult {
    pub examples: usize,
    pub per_level: usize,
    pub source: &'static str,
}

const LABELS: [&str; 3] = ["new", "developing", "proficient"];

/// Produce deterministic, varied coding prompts for local model bootstrapping.
/// These are deliberately not represented as user prompts or as human labels.
pub fn generate_synthetic_jsonl(
    output: &Path,
    per_level: usize,
) -> io::Result<DatasetGenerationResult> {
    if per_level == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "per_level must be greater than zero",
        ));
    }
    let systems = [
        "Rust CLI",
        "React dashboard",
        "Python API",
        "Postgres migration",
        "GitHub Action",
        "Cloudflare Worker",
        "TypeScript library",
        "Android app",
        "Docker service",
        "Go HTTP server",
        "Next.js page",
        "SQLite cache",
        "Kubernetes job",
        "VS Code extension",
        "GraphQL resolver",
        "iOS client",
        "Terraform module",
        "webhook consumer",
        "Redis queue",
        "authentication flow",
    ];
    let tasks = [
        "fix the failing test",
        "add input validation",
        "make the error handling clearer",
        "implement pagination",
        "resolve the timeout",
        "add a configuration option",
        "debug the parsing issue",
        "improve the retry behavior",
        "add structured logging",
        "update the deployment workflow",
        "prevent duplicate records",
        "handle an empty response",
        "make the cache invalidation reliable",
        "add rate limiting",
        "fix the date conversion",
        "improve the accessibility",
        "add a health check",
        "make the migration reversible",
        "fix the authorization bug",
        "reduce the memory usage",
    ];
    let contexts = [
        "the test fails only when the response body is empty",
        "production requests occasionally receive a 429 response",
        "the existing API must remain backward compatible",
        "the bug occurs after a process restart",
        "the current implementation is in src/service.rs",
        "the change must work on Linux and macOS",
        "the database already contains duplicate legacy rows",
        "the UI is used with keyboard navigation",
        "the endpoint processes untrusted JSON",
        "the workflow runs on every pull request",
    ];
    let constraints = [
        "do not add dependencies",
        "preserve the public API",
        "keep the patch small",
        "do not change the database schema",
        "avoid logging secrets",
        "use the existing test helpers",
        "do not modify generated files",
        "keep the operation idempotent",
        "return typed errors",
        "maintain the current latency budget",
    ];
    let checks = [
        "add a regression test that fails before the fix",
        "show the exact command to verify it",
        "include unit tests for the edge case",
        "ensure cargo test passes",
        "describe the expected HTTP status and response body",
        "include a minimal reproduction",
        "list the files changed and why",
        "verify the failure is handled without a panic",
        "add an acceptance test for the happy and failure paths",
        "explain how the change is rolled back",
    ];
    let starts = [
        "Can you",
        "Please",
        "I need help to",
        "Could you",
        "Help me",
    ];

    let mut lines = String::new();
    for level in LABELS {
        for index in 0..per_level {
            let system = systems[index % systems.len()];
            let task = tasks[(index / systems.len()) % tasks.len()];
            let context = contexts[(index / 7) % contexts.len()];
            let constraint = constraints[(index / 11) % constraints.len()];
            let check = checks[(index / 13) % checks.len()];
            let prompt = match level {
                "new" => format!(
                    "{} help with my {}? Something is broken and I need {}.",
                    starts[index % starts.len()],
                    system,
                    task
                ),
                "developing" => format!(
                    "{} {} in the {}. Context: {}. Please suggest the code change.",
                    starts[index % starts.len()],
                    task,
                    system,
                    context
                ),
                _ => format!(
                    "In the {}, {}. Context: {}. Constraints: {}. Success criteria: {}.",
                    system, task, context, constraint, check
                ),
            };
            let record = serde_json::json!({
                "prompt": prompt,
                "label": level,
                "source": "synthetic-v2",
            });
            lines.push_str(&serde_json::to_string(&record).map_err(io::Error::other)?);
            lines.push('\n');
        }
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output, lines)?;
    Ok(DatasetGenerationResult {
        examples: per_level * LABELS.len(),
        per_level,
        source: "synthetic-v2",
    })
}

pub fn predict_from_default_path(prompt: &str) -> ModelPrediction {
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("HACKAI_PROMPT_MODEL_PATH") {
        paths.push(path.into());
    }
    if let Some(path) = dirs::config_dir().map(|path| path.join("hackai/prompt-model.json")) {
        paths.push(path);
    }
    // Useful for local development and Prompt Lab. Release installations use
    // the user config path above (or HACKAI_PROMPT_MODEL_PATH) instead.
    paths.push(Path::new("ml/prompt-model.synthetic-v2.json").to_path_buf());

    paths
        .into_iter()
        .find_map(|path| {
            std::fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Model>(&bytes).ok())
        })
        .map_or_else(ModelPrediction::default, |model| predict(prompt, &model))
}

pub fn train_jsonl(dataset: &Path, output: &Path) -> io::Result<TrainingResult> {
    let content = std::fs::read_to_string(dataset)?;
    let examples: Vec<TrainingExample> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(io::Error::other))
        .collect::<Result<_, _>>()?;
    if examples.len() < 3
        || examples
            .iter()
            .any(|example| !LABELS.contains(&example.label.as_str()))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid prompt training data",
        ));
    }
    let mut model = Model {
        version: "prompt-craft-beta-synthetic-v1".into(),
        labels: LABELS.map(str::to_owned),
        bias: [0.0; 3],
        weights: BTreeMap::new(),
        trained_on_synthetic: true,
    };
    // A small deterministic multiclass perceptron. The trained artifact is
    // consumed by the local softmax inference path above.
    for _ in 0..24 {
        for example in &examples {
            let expected = LABELS
                .iter()
                .position(|label| *label == example.label)
                .unwrap_or(0);
            let predicted = best_index(&logits(&example.prompt, &model));
            if predicted != expected {
                model.bias[expected] += 1.0;
                model.bias[predicted] -= 1.0;
                for token in tokens(&example.prompt) {
                    let weight = model.weights.entry(token).or_insert([0.0; 3]);
                    weight[expected] += 1.0;
                    weight[predicted] -= 1.0;
                }
            }
        }
    }
    let correct = examples
        .iter()
        .filter(|example| LABELS[best_index(&logits(&example.prompt, &model))] == example.label)
        .count();
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        output,
        serde_json::to_vec_pretty(&model).map_err(io::Error::other)?,
    )?;
    Ok(TrainingResult {
        examples: examples.len(),
        training_accuracy_permille: (correct * 1000 / examples.len()) as u16,
        trained_on_synthetic: model.trained_on_synthetic,
    })
}

fn predict(prompt: &str, model: &Model) -> ModelPrediction {
    // A small synthetic model must not be allowed to turn generic verbs such
    // as “fix” into a false claim of proficiency. Require enough task context
    // before consulting its learned token weights.
    if lacks_actionable_context(prompt) {
        return ModelPrediction {
            model_version: Some(model.version.clone()),
            level: "new".into(),
            confidence_permille: Some(800),
        };
    }
    let logits = logits(prompt, model);
    let maximum = logits.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let values = logits.map(|logit| (logit - maximum).exp());
    let total: f64 = values.iter().sum();
    let (index, value) = values
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .unwrap_or((0, &0.0));
    let level = model.labels[index].clone();
    // The model has intentionally small, synthetic training data. Do not let
    // a learned word association promote an otherwise unstructured request to
    // "proficient". A proficient result needs both a concrete target and a
    // constraint or a verification/diagnostic request; this is a guardrail on
    // the prediction, not a claim about the person writing the prompt.
    let level = if level == "proficient" && !supports_proficient_level(prompt) {
        "developing".into()
    } else {
        level
    };
    ModelPrediction {
        model_version: Some(model.version.clone()),
        level,
        confidence_permille: (total > 0.0).then(|| ((value / total) * 1000.0) as u16),
    }
}

fn supports_proficient_level(prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    let words = tokens(prompt);
    if words.len() < 8 {
        return false;
    }
    let has_target = [
        "src/",
        "test",
        "login",
        "endpoint",
        "api",
        "component",
        "function",
        "file",
        "directory",
        "repo",
        "project",
        "database",
        "server",
        "route",
        "schema",
    ]
    .iter()
    .any(|signal| normalized.contains(signal));
    let has_constraint = [
        "must",
        "must not",
        "do not",
        "preserve",
        "unchanged",
        "without",
        "only",
        "keep",
        "avoid",
        "backward compatible",
    ]
    .iter()
    .any(|signal| normalized.contains(signal));
    let has_validation = [
        "test",
        "verify",
        "regression",
        "reproduce",
        "expected",
        "acceptance",
        "check",
    ]
    .iter()
    .any(|signal| normalized.contains(signal));
    has_target && (has_constraint || has_validation)
}

fn lacks_actionable_context(prompt: &str) -> bool {
    let words: Vec<&str> = prompt
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    if words.len() <= 7 {
        return true;
    }
    let alphabetic: Vec<char> = prompt
        .chars()
        .filter(|character| character.is_alphabetic())
        .collect();
    let uppercase = alphabetic
        .iter()
        .filter(|character| character.is_uppercase())
        .count();
    if alphabetic.len() >= 4 && uppercase * 100 >= alphabetic.len() * 70 {
        return true;
    }
    let normalized = prompt.to_lowercase();
    let context = [
        "src/",
        "test",
        "error",
        "line ",
        "login",
        "endpoint",
        "api",
        "component",
        "function",
        "file",
        "directory",
        "repo",
        "project",
        "database",
        "server",
        "rust",
        "typescript",
        "javascript",
        " js ",
        "python",
        "route",
        "schema",
    ]
    .iter()
    .any(|signal| normalized.contains(signal));
    !context && words.len() < 16
}

fn logits(prompt: &str, model: &Model) -> [f64; 3] {
    let mut logits = model.bias;
    for token in tokens(prompt) {
        if let Some(weights) = model.weights.get(&token) {
            for (logit, weight) in logits.iter_mut().zip(weights) {
                *logit += weight;
            }
        }
    }
    logits
}

fn tokens(prompt: &str) -> BTreeSet<String> {
    prompt
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn best_index(values: &[f64; 3]) -> usize {
    values
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(index, _)| index)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{Model, generate_synthetic_jsonl, predict};
    use std::collections::BTreeMap;

    #[test]
    fn uses_model_weights_locally() {
        let model = Model {
            version: "test".into(),
            labels: ["new".into(), "developing".into(), "proficient".into()],
            bias: [0.0, 0.0, 0.0],
            weights: BTreeMap::from([("test".into(), [0.0, 0.0, 3.0])]),
            trained_on_synthetic: false,
        };
        let result = predict(
            "run the test in src/lib.rs and verify cargo test passes",
            &model,
        );
        assert_eq!(result.level, "proficient");
        assert_eq!(result.model_version.as_deref(), Some("test"));
    }

    #[test]
    fn does_not_call_a_vague_or_all_caps_prompt_proficient() {
        let model = Model {
            version: "test".into(),
            labels: ["new".into(), "developing".into(), "proficient".into()],
            bias: [0.0, 0.0, 5.0],
            weights: BTreeMap::new(),
            trained_on_synthetic: true,
        };
        assert_eq!(predict("hi can you make a web app", &model).level, "new");
        assert_eq!(
            predict("BRO WHY IS IT NOT WORKING JS FIX IUT", &model).level,
            "new"
        );
    }

    #[test]
    fn prediction_guardrails_are_table_driven() {
        let model = Model {
            version: "test".into(),
            labels: ["new".into(), "developing".into(), "proficient".into()],
            // This intentionally tries to produce a false positive. The
            // structural guardrails below must keep vague prompts below it.
            bias: [0.0, 0.0, 5.0],
            weights: BTreeMap::new(),
            trained_on_synthetic: true,
        };
        let cases = [
            ("hi can you make a web app", "new"),
            ("BRO FIX IT", "new"),
            ("BRO WHY IS IT NOT WORKING JS FIX IUT", "new"),
            (
                "The /login in the / directory is broken; can you fix it?",
                "developing",
            ),
            (
                "In src/routes/users.rs, add pagination to the users endpoint. Keep the response shape unchanged and add a regression test.",
                "proficient",
            ),
            (
                "Run the test in src/lib.rs and verify cargo test passes after the parser fix.",
                "proficient",
            ),
        ];

        for (prompt, expected) in cases {
            assert_eq!(predict(prompt, &model).level, expected, "{prompt}");
        }
    }

    #[test]
    fn generates_balanced_synthetic_data() {
        let output =
            std::env::temp_dir().join(format!("hackai-prompts-{}.jsonl", std::process::id()));
        let result = generate_synthetic_jsonl(&output, 2).unwrap();
        let data = std::fs::read_to_string(&output).unwrap();
        let _ = std::fs::remove_file(output);
        assert_eq!(result.examples, 6);
        assert_eq!(data.lines().count(), 6);
        assert!(data.contains("synthetic-v2"));
    }
}
