//! The Vercel Queues consumer function for background jobs.
//!
//! An app that has `api/jobs.rs` (see `nextrs::jobs::vercel_queues::consumer`)
//! deploys it as a second function with a `queue/v2beta` trigger on the jobs
//! topic. Two things must hold in the uploaded output, and this module is
//! where both are made true:
//!
//! 1. the consumer function's `.vc-config.json` carries the trigger, and
//! 2. every function's environment says a consumer exists
//!    (`NEXTRS_JOBS_CONSUMER`), which is what switches job delivery from the
//!    self-POST to the queue. Without a consumer nothing is marked, and an
//!    app keeps the delivery it had.
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The consumer's source file, relative to the app root — and its key under
/// `functions` in `vercel.json`.
pub const CONSUMER_SOURCE: &str = "api/jobs.rs";
/// The Cargo binary target built from [`CONSUMER_SOURCE`].
pub const CONSUMER_BIN: &str = "jobs";
/// Must match `nextrs::jobs::vercel_queues::TOPIC`.
pub const TOPIC: &str = "nx-jobs";
/// Must match `nextrs::jobs::vercel_queues::CONSUMER_ENV`.
pub const CONSUMER_ENV: &str = "NEXTRS_JOBS_CONSUMER";
const TRIGGER_TYPE: &str = "queue/v2beta";

pub fn has_consumer(root: &Path) -> bool {
    root.join(CONSUMER_SOURCE).is_file()
}

/// The trigger as written in `vercel.json` (`vercel build` adds the consumer
/// group name when it copies it into the function config).
///
/// - `retryAfterSeconds`: how soon an unacknowledged message comes back after
///   its visibility timeout. Retries of a *failed job* do not use this; the
///   consumer sends a new delayed message for those.
/// - `maxDeliveries`: a ceiling for a message that is never acknowledged
///   (a job missing from the build, say). A crash loop is already bounded by
///   the row's attempts. It is well above what a long-running job uses up
///   while its message is redelivered to a busy consumer, and the platform
///   does not enforce it exactly.
pub fn trigger() -> Value {
    json!({
        "type": TRIGGER_TYPE,
        "topic": TOPIC,
        "retryAfterSeconds": 60,
        "maxDeliveries": 32,
    })
}

/// The consumer group `vercel build` derives from a `functions` key
/// (`@vercel/build-utils` `sanitizeConsumerName`): `_` → `__`, `/` → `_S`,
/// `.` → `_D`, anything else outside `[A-Za-z0-9-]` → `_XX` hex.
pub fn consumer_group(pattern: &str) -> String {
    let mut out = String::new();
    for c in pattern.chars() {
        match c {
            '_' => out.push_str("__"),
            '/' => out.push_str("_S"),
            '.' => out.push_str("_D"),
            c if c.is_ascii_alphanumeric() || c == '-' => out.push(c),
            c => out.push_str(&format!("_{:02X}", c as u32)),
        }
    }
    out
}

/// The trigger as it must appear in a function's `.vc-config.json`.
pub fn function_trigger() -> Value {
    let mut trigger = trigger();
    trigger["consumer"] = json!(consumer_group(CONSUMER_SOURCE));
    trigger
}

/// Where the consumer function lives in a Build Output directory.
pub fn function_dir(output: &Path) -> PathBuf {
    output.join("functions/api/jobs.func")
}

fn is_jobs_trigger(trigger: &Value) -> bool {
    trigger["type"] == TRIGGER_TYPE && trigger["topic"] == TOPIC
}

fn has_jobs_trigger(config: &Value) -> bool {
    config["experimentalTriggers"]
        .as_array()
        .is_some_and(|triggers| triggers.iter().any(is_jobs_trigger))
}

/// Make `config` (a consumer function's `.vc-config.json`) carry the jobs
/// trigger. Returns whether it had to be added.
pub fn ensure_trigger(config: &mut Value) -> bool {
    if has_jobs_trigger(config) {
        return false;
    }
    config["experimentalTriggers"] = json!([function_trigger()]);
    true
}

/// Tell a function a consumer exists.
pub fn mark_consumer_present(config: &mut Value) {
    if !config["environment"].is_object() {
        config["environment"] = json!({});
    }
    config["environment"][CONSUMER_ENV] = json!("vercel-queues");
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(value).unwrap()),
    )
    .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Finish a `vercel build` output for an app at `root`.
///
/// With `api/jobs.rs`: the consumer function must be in the output (an error
/// otherwise — a consumer that silently did not build would leave every job
/// queued forever), its config gets the trigger if `vercel build` did not
/// copy it, and every function is marked. Without it: nothing is touched.
pub fn finish_output(
    root: &Path,
    output: &Path,
    configs: &[PathBuf],
) -> Result<Option<String>, String> {
    if !has_consumer(root) {
        return Ok(None);
    }
    let consumer_config = function_dir(output).join(".vc-config.json");
    if !consumer_config.is_file() {
        return Err(format!(
            "{CONSUMER_SOURCE} exists but the build produced no function for it ({} is missing). Declare its Cargo target (`[[bin]] name = \"{CONSUMER_BIN}\"`, `path = \"{CONSUMER_SOURCE}\"`) and run `nextrs generate`.",
            consumer_config.display()
        ));
    }
    let mut added = false;
    for path in configs {
        let mut config = read_json(path)?;
        if *path == consumer_config {
            added = ensure_trigger(&mut config);
        }
        mark_consumer_present(&mut config);
        write_json(path, &config)?;
    }
    Ok(Some(format!(
        "jobs deliver through Vercel Queues: topic {TOPIC}, consumer {CONSUMER_SOURCE}{}",
        if added {
            " (trigger added to its function config)"
        } else {
            ""
        }
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consumer_group_matches_vercel_build() {
        // The name the spike saw `vercel build` generate for its function.
        assert_eq!(
            consumer_group("api/queue_spike.rs"),
            "api_Squeue__spike_Drs"
        );
        assert_eq!(consumer_group(CONSUMER_SOURCE), "api_Sjobs_Drs");
        assert_eq!(consumer_group("a b"), "a_20b");
    }

    #[test]
    fn function_trigger_names_the_topic_and_group() {
        assert_eq!(
            function_trigger(),
            json!({
                "type": "queue/v2beta",
                "topic": "nx-jobs",
                "retryAfterSeconds": 60,
                "maxDeliveries": 32,
                "consumer": "api_Sjobs_Drs",
            })
        );
    }

    fn output(name: &str, consumer_config: Option<Value>) -> (PathBuf, PathBuf, Vec<PathBuf>) {
        let root =
            std::env::temp_dir().join(format!("nextrs-jobs-queue-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let output = root.join(".vercel/output");
        let index = output.join("functions/api/index.func/.vc-config.json");
        fs::create_dir_all(index.parent().unwrap()).unwrap();
        fs::write(
            &index,
            r#"{"handler":"executable","environment":{"A":"1"}}"#,
        )
        .unwrap();
        let mut configs = vec![index];
        if let Some(config) = consumer_config {
            let path = function_dir(&output).join(".vc-config.json");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, config.to_string()).unwrap();
            configs.push(path);
        }
        (root, output, configs)
    }

    #[test]
    fn an_app_without_a_consumer_is_left_alone() {
        let (root, output, configs) = output("none", None);
        assert_eq!(finish_output(&root, &output, &configs).unwrap(), None);
        let index = read_json(&configs[0]).unwrap();
        assert_eq!(index["environment"], json!({"A": "1"}));
    }

    #[test]
    fn a_consumer_marks_every_function_and_keeps_the_built_trigger() {
        let built = json!({"handler":"executable","experimentalTriggers":[function_trigger()]});
        let (root, output, configs) = output("built", Some(built));
        fs::create_dir_all(root.join("api")).unwrap();
        fs::write(root.join(CONSUMER_SOURCE), "fn main() {}").unwrap();
        let summary = finish_output(&root, &output, &configs).unwrap().unwrap();
        assert!(!summary.contains("trigger added"), "{summary}");
        let index = read_json(&configs[0]).unwrap();
        assert_eq!(
            index["environment"],
            json!({"A": "1", "NEXTRS_JOBS_CONSUMER": "vercel-queues"})
        );
        assert!(
            index.get("experimentalTriggers").is_none(),
            "only the consumer is triggered"
        );
        let consumer = read_json(&configs[1]).unwrap();
        assert_eq!(
            consumer["experimentalTriggers"],
            json!([function_trigger()])
        );
        assert_eq!(
            consumer["environment"]["NEXTRS_JOBS_CONSUMER"],
            "vercel-queues"
        );
    }

    #[test]
    fn a_consumer_built_without_its_trigger_gets_one() {
        let (root, output, configs) = output("bare", Some(json!({"handler":"executable"})));
        fs::create_dir_all(root.join("api")).unwrap();
        fs::write(root.join(CONSUMER_SOURCE), "fn main() {}").unwrap();
        let summary = finish_output(&root, &output, &configs).unwrap().unwrap();
        assert!(summary.contains("trigger added"), "{summary}");
        let consumer = read_json(&configs[1]).unwrap();
        assert_eq!(
            consumer["experimentalTriggers"][0]["consumer"],
            "api_Sjobs_Drs"
        );
    }

    #[test]
    fn a_consumer_source_with_no_built_function_is_an_error() {
        let (root, output, configs) = output("missing", None);
        fs::create_dir_all(root.join("api")).unwrap();
        fs::write(root.join(CONSUMER_SOURCE), "fn main() {}").unwrap();
        let error = finish_output(&root, &output, &configs).unwrap_err();
        assert!(error.contains("produced no function"), "{error}");
        assert!(error.contains("[[bin]]"), "{error}");
    }
}
