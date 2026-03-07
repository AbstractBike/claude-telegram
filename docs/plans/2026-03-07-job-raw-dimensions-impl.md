# Job Raw Dimensions — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Enrich job postings with raw dimensions (salary, work mode, applicants, etc.) extracted via Firecrawl LLM, feed them into a configurable rank weight map, and produce LLM-generated friendly summaries in the email.

**Architecture:** New `scrape` subcommand in hunter-engine reads jobs from stdin, calls Firecrawl `/v1/scrape` with a JSON schema generated from `dimensions.toml`, and writes enriched jobs to stdout. The ranker adds weighted dimension deltas on top of TF-IDF. The renderer always calls Claude API for friendly 2–3 line per-job summaries. Daily mode sends at most `daily_limit` jobs (default 10, per-person in `config.toml`, asked during onboarding). Weekly mode sends all ranked jobs with no limit.

**Tech Stack:** Rust (tokio/async, reqwest, serde_json), TOML config (toml crate), Firecrawl `/v1/scrape` with `jsonOptions`, Claude Messages API (claude-haiku-4-5-20251001 for render).

---

### Task 1: Add `toml` crate + `dimensions.toml` config file

**Files:**
- Modify: `~/hunters/Cargo.toml`
- Create: `~/hunters/src/dimensions.rs`
- Create: `~/hunters/dimensions.toml`

**Step 1: Add toml dependency**

In `~/hunters/Cargo.toml`, add to `[dependencies]`:
```toml
toml = "0.8"
```

**Step 2: Create `~/hunters/dimensions.toml`**

```toml
# Dimensions extracted from job pages + their influence on ranking score.
# weight = 0.0  → extract only, no ranking effect
# weight > 0.0  → boosts score
# weight < 0.0  → penalizes score
# values        → categorical mapping: value string → weight delta
# If a dimension is absent (None) it is silently skipped in ranking.

[dimensions.job_description]
extract = true
weight = 0.0

[dimensions.days_published]
extract = true
weight = -0.05

[dimensions.applicants]
extract = true
weight = -0.08

[dimensions.salary_max]
extract = true
weight = 0.15

[dimensions.salary_min]
extract = true
weight = 0.0

[dimensions.salary_currency]
extract = true
weight = 0.0

[dimensions.salary_period]
extract = true
weight = 0.0

[dimensions.yoe_min]
extract = true
weight = -0.10

[dimensions.yoe_max]
extract = true
weight = 0.0

[dimensions.education_level]
extract = true
weight = 0.0

[dimensions.hard_skills]
extract = true
weight = 0.0

[dimensions.languages]
extract = true
weight = 0.0

[dimensions.work_mode]
extract = true
[dimensions.work_mode.values]
remote = 0.20
hybrid = 0.08
onsite = -0.10

[dimensions.contract_type]
extract = true
[dimensions.contract_type.values]
permanent = 0.10
temporary = -0.05
freelance = 0.0

[dimensions.seniority]
extract = true
weight = 0.0

[dimensions.is_easy_apply]
extract = true
weight = 0.05

[dimensions.is_agency]
extract = true
weight = -0.05

[dimensions.company_size]
extract = true
weight = 0.0

[dimensions.company_name]
extract = true
weight = 0.0
```

**Step 3: Create `~/hunters/src/dimensions.rs`**

```rust
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DimensionConfig {
    #[serde(default)]
    pub extract: bool,
    #[serde(default)]
    pub weight: f64,
    /// For categorical dimensions: value string → weight delta
    #[serde(default)]
    pub values: HashMap<String, f64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DimensionsFile {
    #[serde(default)]
    pub dimensions: HashMap<String, DimensionConfig>,
}

pub fn load(hunters_dir: &std::path::Path) -> Result<DimensionsFile> {
    let path = hunters_dir.join("dimensions.toml");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_default(); // missing file = empty config = pure TF-IDF
    let parsed: DimensionsFile = toml::from_str(&content)?;
    Ok(parsed)
}

/// Returns only dimensions with extract = true, sorted by name (stable order for schema).
pub fn extractable(dims: &DimensionsFile) -> Vec<(&str, &DimensionConfig)> {
    let mut list: Vec<(&str, &DimensionConfig)> = dims
        .dimensions
        .iter()
        .filter(|(_, v)| v.extract)
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    list.sort_by_key(|(k, _)| *k);
    list
}
```

**Step 4: Add `mod dimensions;` to `~/hunters/src/lib.rs`**

`lib.rs` currently just has module declarations. Add:
```rust
pub mod dimensions;
```

Also add to `~/hunters/src/main.rs`:
```rust
mod dimensions;
```

**Step 5: Build to verify**

```bash
cd ~/hunters && cargo build 2>&1 | tail -5
```
Expected: compiles without errors.

**Step 6: Commit**

```bash
cd ~/hunters && git add Cargo.toml Cargo.lock src/dimensions.rs dimensions.toml src/main.rs && git commit -m "feat: add dimensions.toml config + DimensionsFile loader"
```

---

### Task 2: Expand `JobMetadata` model

**Files:**
- Modify: `~/hunters/src/model.rs`

**Step 1: Write failing test**

In `~/hunters/src/model.rs`, add to the `#[cfg(test)]` section (create one if absent):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_metadata_new_fields_serialize() {
        let meta = JobMetadata {
            salary_min: Some(40000),
            salary_max: Some(60000),
            salary_currency: Some("EUR".into()),
            salary_period: Some("annual".into()),
            yoe_min: Some(3),
            yoe_max: Some(5),
            education_level: Some("Grado".into()),
            hard_skills: Some(vec!["Python".into(), "SQL".into()]),
            languages: Some(vec![LanguageReq { lang: "English".into(), level: Some("C1".into()) }]),
            is_easy_apply: Some(true),
            is_promoted: Some(false),
            is_agency: Some(false),
            company_name: Some("Ergomed".into()),
            company_size: Some("501-1000".into()),
            company_industry: Some("Pharma".into()),
            job_description: Some("Full job text here.".into()),
            platform: Some("linkedin".into()),
            scraped_at: None,
            // existing fields
            days_published: Some(3),
            applicants: Some(47),
            salary_range: None,
            employment_type: None,
            work_mode: Some("remote".into()),
            contract_type: Some("permanent".into()),
            seniority: Some("Mid-Senior".into()),
            company_description: None,
            scraped: true,
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("salary_min"));
        assert!(json.contains("hard_skills"));
        assert!(json.contains("languages"));
        assert!(json.contains("job_description"));
    }
}
```

**Step 2: Run to verify it fails**

```bash
cd ~/hunters && cargo test test_job_metadata_new_fields_serialize 2>&1 | tail -20
```
Expected: compile error — fields don't exist yet.

**Step 3: Replace `JobMetadata` and add `LanguageReq`**

Replace the entire `JobMetadata` struct in `model.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub url: String,
    pub title: String,
    pub description: String,
    pub source: String,
    pub found_at: DateTime<Utc>,
    pub date: String,
    pub search_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JobMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageReq {
    pub lang: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JobMetadata {
    // Engagement
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days_published: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applicants: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_easy_apply: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_promoted: Option<bool>,

    // Compensation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub salary_range: Option<String>,  // kept for display/fallback
    #[serde(skip_serializing_if = "Option::is_none")]
    pub salary_min: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub salary_max: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub salary_currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub salary_period: Option<String>,  // annual | monthly | hourly

    // Requirements
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yoe_min: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yoe_max: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub education_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hard_skills: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub languages: Option<Vec<LanguageReq>>,

    // Role
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_mode: Option<String>,       // remote | hybrid | onsite
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seniority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub employment_type: Option<String>, // kept from existing

    // Company
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_description: Option<String>, // kept from existing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_industry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_agency: Option<bool>,

    // Content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_description: Option<String>,

    // Traceability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default)]
    pub scraped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scraped_at: Option<DateTime<Utc>>,
}
```

**Step 4: Run test to verify it passes**

```bash
cd ~/hunters && cargo test test_job_metadata_new_fields_serialize 2>&1 | tail -10
```
Expected: `test test_job_metadata_new_fields_serialize ... ok`

**Step 5: Run all tests**

```bash
cd ~/hunters && cargo test 2>&1 | tail -15
```
Expected: all existing tests still pass.

**Step 6: Commit**

```bash
cd ~/hunters && git add src/model.rs && git commit -m "feat: expand JobMetadata with raw dimensions fields + LanguageReq"
```

---

### Task 3: `scrape` subcommand — Firecrawl LLM extraction

**Files:**
- Create: `~/hunters/src/scrape.rs`
- Modify: `~/hunters/src/main.rs`

The scrape command reads a JSON array of `Job`s from stdin, calls Firecrawl `/v1/scrape` for each URL (max 10), merges extracted dimensions into `metadata`, writes enriched jobs to stdout.

**Step 1: Write failing test**

Add to `~/hunters/src/scrape.rs` (create file):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_platform_linkedin() {
        assert_eq!(detect_platform("https://www.linkedin.com/jobs/view/12345"), "linkedin");
    }

    #[test]
    fn test_detect_platform_infojobs() {
        assert_eq!(detect_platform("https://www.infojobs.net/oferta-trabajo/abc"), "infojobs");
    }

    #[test]
    fn test_detect_platform_unknown() {
        assert_eq!(detect_platform("https://example.com/job/123"), "unknown");
    }

    #[test]
    fn test_build_extraction_schema_includes_extractable_dims() {
        use std::collections::HashMap;
        use crate::dimensions::{DimensionConfig, DimensionsFile};
        let mut dims = HashMap::new();
        dims.insert("salary_max".to_string(), DimensionConfig { extract: true, weight: 0.15, values: HashMap::new() });
        dims.insert("work_mode".to_string(), DimensionConfig { extract: true, weight: 0.0, values: HashMap::new() });
        dims.insert("applicants".to_string(), DimensionConfig { extract: false, weight: 0.0, values: HashMap::new() });
        let file = DimensionsFile { dimensions: dims };
        let schema = build_extraction_schema(&file);
        assert!(schema["properties"].get("salary_max").is_some());
        assert!(schema["properties"].get("work_mode").is_some());
        assert!(schema["properties"].get("applicants").is_none()); // extract=false
    }
}
```

**Step 2: Run to verify it fails**

```bash
cd ~/hunters && cargo test test_detect_platform 2>&1 | tail -10
```
Expected: compile error — `scrape` module doesn't exist.

**Step 3: Implement `scrape.rs`**

```rust
use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};
use std::io::Read;
use std::time::Duration;

use crate::dimensions::DimensionsFile;
use crate::model::{Job, JobMetadata, LanguageReq};

// --- platform detection ---

pub fn detect_platform(url: &str) -> &'static str {
    if url.contains("linkedin.com") { return "linkedin"; }
    if url.contains("infojobs.net") { return "infojobs"; }
    if url.contains("glassdoor.com") { return "glassdoor"; }
    if url.contains("indeed.com") { return "indeed"; }
    if url.contains("pharmiweb.com") { return "pharmiweb"; }
    if url.contains("europharmajobs.com") { return "europharmajobs"; }
    if url.contains("himalayas.app") { return "himalayas"; }
    if url.contains("remotefirstjobs.com") { return "remotefirstjobs"; }
    if url.contains("totaljobs.com") { return "totaljobs"; }
    "unknown"
}

// --- schema builder ---

pub fn build_extraction_schema(dims: &DimensionsFile) -> Value {
    let extractable = crate::dimensions::extractable(dims);

    let mut properties = serde_json::Map::new();
    for (name, _cfg) in &extractable {
        let type_def = match *name {
            "days_published" | "applicants" | "salary_min" | "salary_max" | "yoe_min" | "yoe_max" => {
                json!({"type": "integer"})
            }
            "is_easy_apply" | "is_promoted" | "is_agency" => {
                json!({"type": "boolean"})
            }
            "hard_skills" => {
                json!({"type": "array", "items": {"type": "string"}})
            }
            "languages" => {
                json!({
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "lang": {"type": "string"},
                            "level": {"type": "string"}
                        }
                    }
                })
            }
            _ => json!({"type": "string"}),
        };
        properties.insert(name.to_string(), type_def);
    }

    json!({
        "type": "object",
        "properties": properties
    })
}

// --- main scrape function ---

pub async fn scrape(slug: &str) -> Result<()> {
    let api_key = crate::config::firecrawl_api_key()?;
    let hunters_dir = crate::config::hunters_dir();
    let dims = crate::dimensions::load(&hunters_dir)?;
    let schema = build_extraction_schema(&dims);

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let mut jobs: Vec<Job> = serde_json::from_str(&input)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let firecrawl_base = "http://192.168.0.4:3002";

    for job in jobs.iter_mut() {
        let platform = detect_platform(&job.url);
        eprintln!("[scrape] {} ({})", job.url, platform);

        let payload = json!({
            "url": job.url,
            "formats": ["json"],
            "onlyMainContent": true,
            "jsonOptions": {
                "extractionPrompt": "Extract job posting dimensions. Only include fields that are explicitly present in the page. Do not guess.",
                "extractionSchema": schema
            }
        });

        let resp = client
            .post(format!("{}/v1/scrape", firecrawl_base))
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&payload)
            .send()
            .await;

        let extracted: Option<Value> = match resp {
            Err(e) => {
                eprintln!("[scrape] WARN: request failed for {} — {}", job.url, e);
                None
            }
            Ok(r) => {
                let body: Value = match r.json().await {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("[scrape] WARN: parse error for {} — {}", job.url, e);
                        continue;
                    }
                };
                body.get("data")
                    .and_then(|d| d.get("json"))
                    .cloned()
            }
        };

        let mut meta = job.metadata.clone().unwrap_or_default();
        meta.platform = Some(platform.to_string());
        meta.scraped_at = Some(Utc::now());

        if let Some(ext) = extracted {
            meta.scraped = true;
            apply_extracted(&mut meta, &ext);
        }

        job.metadata = Some(meta);
    }

    println!("{}", serde_json::to_string(&jobs)?);
    Ok(())
}

/// Merge extracted JSON values into JobMetadata fields.
fn apply_extracted(meta: &mut JobMetadata, ext: &Value) {
    if let Some(v) = ext.get("days_published").and_then(|v| v.as_u64()) {
        meta.days_published = Some(v as u32);
    }
    if let Some(v) = ext.get("applicants").and_then(|v| v.as_u64()) {
        meta.applicants = Some(v as u32);
    }
    if let Some(v) = ext.get("salary_min").and_then(|v| v.as_u64()) {
        meta.salary_min = Some(v as u32);
    }
    if let Some(v) = ext.get("salary_max").and_then(|v| v.as_u64()) {
        meta.salary_max = Some(v as u32);
    }
    if let Some(v) = ext.get("salary_currency").and_then(|v| v.as_str()) {
        meta.salary_currency = Some(v.to_string());
    }
    if let Some(v) = ext.get("salary_period").and_then(|v| v.as_str()) {
        meta.salary_period = Some(v.to_string());
    }
    if let Some(v) = ext.get("yoe_min").and_then(|v| v.as_u64()) {
        meta.yoe_min = Some(v as u32);
    }
    if let Some(v) = ext.get("yoe_max").and_then(|v| v.as_u64()) {
        meta.yoe_max = Some(v as u32);
    }
    if let Some(v) = ext.get("education_level").and_then(|v| v.as_str()) {
        meta.education_level = Some(v.to_string());
    }
    if let Some(v) = ext.get("work_mode").and_then(|v| v.as_str()) {
        meta.work_mode = Some(v.to_lowercase());
    }
    if let Some(v) = ext.get("contract_type").and_then(|v| v.as_str()) {
        meta.contract_type = Some(v.to_lowercase());
    }
    if let Some(v) = ext.get("seniority").and_then(|v| v.as_str()) {
        meta.seniority = Some(v.to_string());
    }
    if let Some(v) = ext.get("is_easy_apply").and_then(|v| v.as_bool()) {
        meta.is_easy_apply = Some(v);
    }
    if let Some(v) = ext.get("is_promoted").and_then(|v| v.as_bool()) {
        meta.is_promoted = Some(v);
    }
    if let Some(v) = ext.get("is_agency").and_then(|v| v.as_bool()) {
        meta.is_agency = Some(v);
    }
    if let Some(v) = ext.get("company_name").and_then(|v| v.as_str()) {
        meta.company_name = Some(v.to_string());
    }
    if let Some(v) = ext.get("company_size").and_then(|v| v.as_str()) {
        meta.company_size = Some(v.to_string());
    }
    if let Some(arr) = ext.get("hard_skills").and_then(|v| v.as_array()) {
        let skills: Vec<String> = arr.iter()
            .filter_map(|s| s.as_str().map(|s| s.to_string()))
            .collect();
        if !skills.is_empty() {
            meta.hard_skills = Some(skills);
        }
    }
    if let Some(arr) = ext.get("languages").and_then(|v| v.as_array()) {
        let langs: Vec<LanguageReq> = arr.iter()
            .filter_map(|l| {
                let lang = l.get("lang")?.as_str()?.to_string();
                let level = l.get("level").and_then(|v| v.as_str()).map(|s| s.to_string());
                Some(LanguageReq { lang, level })
            })
            .collect();
        if !langs.is_empty() {
            meta.languages = Some(langs);
        }
    }
    if let Some(v) = ext.get("job_description").and_then(|v| v.as_str()) {
        if !v.is_empty() {
            meta.job_description = Some(v.to_string());
        }
    }
}
```

**Step 4: Register in `main.rs`**

Add `mod scrape;` and add the `Scrape` variant:

```rust
mod scrape;
// ...
/// Scrape job pages for raw dimensions
Scrape { slug: String },
// ...
Commands::Scrape { slug } => {
    scrape::scrape(&slug).await?;
}
```

**Step 5: Run tests**

```bash
cd ~/hunters && cargo test test_detect_platform test_build_extraction_schema 2>&1 | tail -15
```
Expected: 4 tests pass.

**Step 6: Build**

```bash
cd ~/hunters && cargo build 2>&1 | tail -5
```

**Step 7: Commit**

```bash
cd ~/hunters && git add src/scrape.rs src/main.rs && git commit -m "feat: add scrape subcommand with Firecrawl LLM extraction"
```

---

### Task 4: Update `rank.rs` to apply dimension weights

**Files:**
- Modify: `~/hunters/src/rank.rs`

**Step 1: Write failing test**

Add to the test module in `rank.rs`:

```rust
#[test]
fn test_dimension_delta_numeric() {
    // salary_max=80000, weight=0.15 → delta = (80000/100000) * 0.15 = 0.12
    let delta = dimension_delta_numeric("salary_max", 80000, 0.15);
    assert!((delta - 0.12).abs() < 0.001);
}

#[test]
fn test_dimension_delta_numeric_capped() {
    // days_published=60 (>30 cap) → normalize=1.0 → delta = 1.0 * -0.05 = -0.05
    let delta = dimension_delta_numeric("days_published", 60, -0.05);
    assert!((delta - (-0.05)).abs() < 0.001);
}

#[test]
fn test_dimension_delta_bool_true() {
    let delta = dimension_delta_bool(true, 0.05);
    assert!((delta - 0.05).abs() < 0.001);
}

#[test]
fn test_dimension_delta_bool_false() {
    let delta = dimension_delta_bool(false, 0.05);
    assert_eq!(delta, 0.0);
}
```

**Step 2: Run to verify they fail**

```bash
cd ~/hunters && cargo test test_dimension_delta 2>&1 | tail -10
```
Expected: compile error — functions don't exist.

**Step 3: Add dimension weight helpers and integrate into `rank()`**

At the top of `rank.rs`, add the imports:
```rust
use crate::dimensions::DimensionsFile;
use crate::config::hunters_dir;
```

Add after the `assign_tier` function:

```rust
/// Normalize a numeric dimension value to [0.0, 1.0].
fn normalize_numeric(name: &str, value: u32) -> f64 {
    let cap = match name {
        "days_published" => 30.0,
        "applicants" => 500.0,
        "salary_max" | "salary_min" => 100_000.0,
        "yoe_min" | "yoe_max" => 10.0,
        _ => 100.0,
    };
    (value as f64 / cap).min(1.0)
}

pub fn dimension_delta_numeric(name: &str, value: u32, weight: f64) -> f64 {
    normalize_numeric(name, value) * weight
}

pub fn dimension_delta_bool(value: bool, weight: f64) -> f64 {
    if value { weight } else { 0.0 }
}

/// Compute the total dimension score delta for a job given loaded dimensions config.
fn metadata_delta(meta: &crate::model::JobMetadata, dims: &DimensionsFile) -> f64 {
    let mut delta = 0.0_f64;

    macro_rules! numeric {
        ($field:expr, $name:literal) => {
            if let Some(v) = $field {
                if let Some(cfg) = dims.dimensions.get($name) {
                    delta += dimension_delta_numeric($name, v, cfg.weight);
                }
            }
        };
    }
    macro_rules! boolean {
        ($field:expr, $name:literal) => {
            if let Some(v) = $field {
                if let Some(cfg) = dims.dimensions.get($name) {
                    delta += dimension_delta_bool(v, cfg.weight);
                }
            }
        };
    }
    macro_rules! categorical {
        ($field:expr, $name:literal) => {
            if let Some(ref v) = $field {
                if let Some(cfg) = dims.dimensions.get($name) {
                    let key = v.to_lowercase();
                    delta += cfg.values.get(&key).copied().unwrap_or(0.0);
                }
            }
        };
    }

    numeric!(meta.days_published, "days_published");
    numeric!(meta.applicants, "applicants");
    numeric!(meta.salary_max, "salary_max");
    numeric!(meta.salary_min, "salary_min");
    numeric!(meta.yoe_min, "yoe_min");
    numeric!(meta.yoe_max, "yoe_max");
    boolean!(meta.is_easy_apply, "is_easy_apply");
    boolean!(meta.is_agency, "is_agency");
    boolean!(meta.is_promoted, "is_promoted");
    categorical!(meta.work_mode, "work_mode");
    categorical!(meta.contract_type, "contract_type");

    delta
}
```

In the `rank()` function, after computing `score` with `cosine_sim`, add:

```rust
// Load dimensions config for weight adjustments
let dims = crate::dimensions::load(&hunters_dir()).unwrap_or_default();

// ... (inside the loop where job.score is set)
let tfidf_score = cosine_sim(&profile_vec, &job_vec);
let dim_delta = job.metadata.as_ref()
    .map(|m| metadata_delta(m, &dims))
    .unwrap_or(0.0);
let rounded = ((tfidf_score + dim_delta) * 1000.0).round() / 1000.0;
job.score = Some(rounded.max(0.0)); // floor at 0
job.tier = Some(assign_tier(rounded));
```

Also update TF-IDF text to use `job_description` when available:

```rust
let text = format!(
    "{} {} {} {}",
    j.title,
    j.description,
    j.metadata.as_ref().and_then(|m| m.job_description.as_deref()).unwrap_or(""),
    j.metadata.as_ref().and_then(|m| m.company_description.as_deref()).unwrap_or("")
);
```

**Step 4: Run tests**

```bash
cd ~/hunters && cargo test test_dimension_delta 2>&1 | tail -10
```
Expected: 4 tests pass.

**Step 5: Run all tests**

```bash
cd ~/hunters && cargo test 2>&1 | tail -15
```

**Step 6: Commit**

```bash
cd ~/hunters && git add src/rank.rs && git commit -m "feat: rank applies dimension weight deltas from dimensions.toml"
```

---

### Task 5: Update `render.rs` to use Claude API for all modes

**Files:**
- Modify: `~/hunters/src/render.rs`
- Create: `~/hunters/render-prompt.md`

The Rust template renderer is replaced with a Claude API call for all modes.
The weekly prompt is kept for the `weekly` mode; `render-prompt.md` handles daily/complete.

**Step 1: Create `~/hunters/render-prompt.md`**

```markdown
You are a headhunter assistant writing a daily job search email for a candidate.

Write a complete HTML email (inline CSS, email-safe) with:

1. A friendly opening paragraph (3-5 sentences in the candidate's language) summarizing
   the day's results. Mention the best opportunities by name. Be warm and encouraging.

2. For each job, a card with:
   - Job title as a clickable link
   - 2-3 lines: why it fits the candidate, key requirements, salary/mode if known
   - Small pill badges for: tier, work_mode, contract_type (if available)
   - Tier 1 jobs get a green left border and light green background
   - Tier 2 jobs get a blue left border and light blue background
   - Tier 3 jobs get a grey left border, compact (title + link only, no summary)

3. A footer with total count, date, and note about data sources.

IMPORTANT:
- Respond with ONLY the HTML content inside a <div>. No ```html fences.
- Use inline CSS only (email clients strip stylesheets).
- Font: Arial, Helvetica, sans-serif throughout.
- Max width: 640px.
- Write in the language specified by LANGUAGE field.
```

**Step 2: Write failing test**

Add to the test module in `render.rs`:

```rust
#[test]
fn test_format_job_context_includes_key_fields() {
    use crate::model::{Job, JobMetadata};
    use chrono::Utc;
    let job = Job {
        url: "https://example.com/job/1".into(),
        title: "Pharmacovigilance Officer".into(),
        description: "short desc".into(),
        source: "test query".into(),
        found_at: Utc::now(),
        date: "2026-03-07".into(),
        search_mode: "daily".into(),
        score: Some(0.35),
        tier: Some(1),
        metadata: Some(JobMetadata {
            salary_min: Some(40000),
            salary_max: Some(55000),
            work_mode: Some("remote".into()),
            job_description: Some("Full description text.".into()),
            ..Default::default()
        }),
    };
    let ctx = format_job_context(&job);
    assert!(ctx.contains("TITLE: Pharmacovigilance Officer"));
    assert!(ctx.contains("SALARY: 40000"));
    assert!(ctx.contains("WORK_MODE: remote"));
    assert!(ctx.contains("Full description text."));
}
```

**Step 3: Run to verify it fails**

```bash
cd ~/hunters && cargo test test_format_job_context 2>&1 | tail -10
```
Expected: compile error — function doesn't exist.

**Step 4: Rewrite `render.rs`**

Replace the entire file content:

```rust
use anyhow::{Context, Result};
use std::io::Read;
use std::time::Duration;

use crate::config::profile_dir;
use crate::model::Job;

/// Format a single job's context for the LLM prompt.
pub fn format_job_context(job: &Job) -> String {
    let mut lines = vec![
        format!("TITLE: {}", job.title),
        format!("URL: {}", job.url),
        format!("TIER: {} | SCORE: {:.3}", job.tier.unwrap_or(3), job.score.unwrap_or(0.0)),
    ];

    if let Some(ref meta) = job.metadata {
        if let (Some(min), Some(max)) = (meta.salary_min, meta.salary_max) {
            let currency = meta.salary_currency.as_deref().unwrap_or("EUR");
            let period = meta.salary_period.as_deref().unwrap_or("annual");
            lines.push(format!("SALARY: {min}–{max} {currency}/{period}"));
        } else if let Some(ref range) = meta.salary_range {
            lines.push(format!("SALARY: {range}"));
        }
        if let Some(ref wm) = meta.work_mode {
            lines.push(format!("WORK_MODE: {wm}"));
        }
        if let Some(ref ct) = meta.contract_type {
            lines.push(format!("CONTRACT: {ct}"));
        }
        if let Some(yoe) = meta.yoe_min {
            if let Some(yoe_max) = meta.yoe_max {
                lines.push(format!("YOE: {yoe}–{yoe_max}"));
            } else {
                lines.push(format!("YOE: {yoe}+"));
            }
        }
        if let Some(n) = meta.applicants {
            lines.push(format!("APPLICANTS: {n}"));
        }
        if let Some(d) = meta.days_published {
            lines.push(format!("DAYS_PUBLISHED: {d}"));
        }
        if let Some(ref skills) = meta.hard_skills {
            lines.push(format!("SKILLS: {}", skills.join(", ")));
        }
        if let Some(ref langs) = meta.languages {
            let lang_str: Vec<String> = langs.iter()
                .map(|l| match &l.level {
                    Some(lv) => format!("{} ({})", l.lang, lv),
                    None => l.lang.clone(),
                })
                .collect();
            lines.push(format!("LANGUAGES: {}", lang_str.join(", ")));
        }
        // Use full job_description if available, otherwise fall back to description snippet
        let desc = meta.job_description.as_deref().unwrap_or(&job.description);
        if !desc.is_empty() {
            let truncated = if desc.len() > 800 {
                let b = desc.floor_char_boundary(800);
                format!("{}...", &desc[..b])
            } else {
                desc.to_string()
            };
            lines.push(format!("DESCRIPTION: {truncated}"));
        }
    } else {
        if !job.description.is_empty() {
            lines.push(format!("DESCRIPTION: {}", job.description));
        }
    }

    lines.join("\n")
}

/// Main render entry point. Reads JSON jobs from stdin, calls Claude API, outputs HTML.
pub fn render(slug: &str, mode: &str) -> Result<()> {
    let profile_dir = profile_dir(slug);
    let hunters_dir = crate::config::hunters_dir();

    let profile_text = std::fs::read_to_string(profile_dir.join("profile.md"))
        .unwrap_or_default();
    let candidate_summary = {
        let s = profile_text.as_str();
        let b = s.floor_char_boundary(500.min(s.len()));
        s[..b].to_string()
    };

    let prompt_file = if mode == "weekly" { "weekly-prompt.md" } else { "render-prompt.md" };
    let system_prompt = std::fs::read_to_string(hunters_dir.join(prompt_file))
        .unwrap_or_else(|_| std::fs::read_to_string(hunters_dir.join("email-prompt.md"))
            .unwrap_or_default());

    let config_path = hunters_dir.join("config.toml");
    let config_text = std::fs::read_to_string(&config_path).unwrap_or_default();
    let language = config_text.lines()
        .find(|l| l.trim_start().starts_with("language"))
        .and_then(|l| l.split('"').nth(1))
        .unwrap_or("es")
        .to_string();

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)
        .context("Failed to read jobs from stdin")?;
    let jobs: Vec<Job> = serde_json::from_str(&input)
        .context("Failed to parse jobs JSON")?;

    let job_contexts: Vec<String> = jobs.iter().map(format_job_context).collect();
    let jobs_block = job_contexts.join("\n---\n");

    let user_message = format!(
        "LANGUAGE: {language}\n\nCANDIDATE PROFILE:\n{candidate_summary}\n\nJOBS ({count}):\n---\n{jobs_block}",
        count = jobs.len(),
    );

    let api_key = std::fs::read_to_string(
        dirs::home_dir().unwrap().join(".config/anthropic/api_key")
    ).context("Missing ~/.config/anthropic/api_key")?.trim().to_string();

    // Synchronous HTTP call using blocking reqwest (render is called from sync context)
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;

    let payload = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 4096,
        "system": system_prompt,
        "messages": [{"role": "user", "content": user_message}]
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .context("Claude API request failed")?;

    let body: serde_json::Value = resp.json().context("Failed to parse Claude response")?;
    let html = body["content"][0]["text"].as_str()
        .context("No text in Claude response")?;

    println!("{}", html);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_display_name_logic() {
        let stripped = "Professional Profile — María del Mar Macías Sánchez";
        let name = stripped.rsplit(" — ").next().unwrap_or(stripped).trim();
        assert_eq!(name, "María del Mar Macías Sánchez");
    }

    #[test]
    fn test_format_job_context_includes_key_fields() {
        use crate::model::{Job, JobMetadata};
        use chrono::Utc;
        let job = Job {
            url: "https://example.com/job/1".into(),
            title: "Pharmacovigilance Officer".into(),
            description: "short desc".into(),
            source: "test query".into(),
            found_at: Utc::now(),
            date: "2026-03-07".into(),
            search_mode: "daily".into(),
            score: Some(0.35),
            tier: Some(1),
            metadata: Some(JobMetadata {
                salary_min: Some(40000),
                salary_max: Some(55000),
                work_mode: Some("remote".into()),
                job_description: Some("Full description text.".into()),
                ..Default::default()
            }),
        };
        let ctx = format_job_context(&job);
        assert!(ctx.contains("TITLE: Pharmacovigilance Officer"));
        assert!(ctx.contains("SALARY: 40000"));
        assert!(ctx.contains("WORK_MODE: remote"));
        assert!(ctx.contains("Full description text."));
    }
}
```

Note: `reqwest` blocking feature must be enabled. Add to `Cargo.toml`:
```toml
reqwest = { version = "0.12", features = ["json", "rustls-tls", "blocking"] }
```

**Step 5: Run tests**

```bash
cd ~/hunters && cargo test test_format_job_context 2>&1 | tail -10
```
Expected: passes.

**Step 6: Run all tests**

```bash
cd ~/hunters && cargo test 2>&1 | tail -15
```

**Step 7: Commit**

```bash
cd ~/hunters && git add src/render.rs render-prompt.md Cargo.toml Cargo.lock && git commit -m "feat: render uses Claude API for LLM summaries in all modes"
```

---

### Task 6: Add `daily_limit` to config + update `job-hunter.sh` pipeline + `sources.md`

**Files:**
- Modify: `~/hunters/config.toml`
- Modify: `~/hunters/job-hunter.sh`
- Modify: `~/hunters/mar/sources.md`

**Context:** Daily mode caps the sent jobs at `daily_limit` (default 10, per-person).
Weekly mode has no cap — sends all ranked jobs. The limit is read from `config.toml`
under `[person.<slug>]` (or `[defaults]` as fallback). Asked during onboarding.

**Step 1: Add `daily_limit` to `~/hunters/config.toml`**

In `[defaults]`:
```toml
[defaults]
search_limit = 10
daily_limit = 10      # max jobs sent per daily run; weekly sends all
```

In `[person.mar]` (or equivalent per-person section), ask during onboarding:
```toml
[person.mar]
daily_limit = 10
```

**Step 2: Read `daily_limit` in `job-hunter.sh`**

After the existing config parsing block, add:
```bash
DAILY_LIMIT=$(sed -n "/^\[person\.$SLUG\]/,/^\[/{ s/^daily_limit *= *\([0-9]*\).*/\1/p; }" "$CONFIG" | head -1)
[[ -z "$DAILY_LIMIT" ]] && DAILY_LIMIT=$(grep '^daily_limit' "$CONFIG" | head -1 | grep -oP '\d+' || echo 10)
```

Then after `RANKED` is computed, apply the cap for daily mode:
```bash
if [[ "$MODE" == "daily" && "$DAILY_LIMIT" -gt 0 ]]; then
  RANKED=$(echo "$RANKED" | jq ".[0:$DAILY_LIMIT]")
  JOB_COUNT=$(echo "$RANKED" | jq 'length')
fi
```

**Step 3: Update pipeline in `job-hunter.sh`**

Find the `RANKED=$(...)` block (lines ~65-67) and replace with:

```bash
RANKED=$(timeout "$TIMEOUT" bash -c \
  '"$1" search "$2" "$3" --max "$4" 2>&2 | "$1" scrape "$2" 2>&2 | "$1" rank "$2" 2>&2' \
  -- "$ENGINE" "$SLUG" "$MODE" "$MAX_RESULTS")
```

**Step 2: Update `~/hunters/mar/sources.md`**

Replace with queries targeting individual job pages:

```markdown
# Mar Job Hunter — Search Sources
# One firecrawl search query per line. Lines starting with # are ignored.
# Queries use site: operators to return individual job postings, not listing pages.

# LinkedIn job detail pages
site:linkedin.com/jobs/view pharmacovigilance spain remote

# InfoJobs individual postings
site:infojobs.net/oferta-trabajo farmacovigilancia españa

# InfoJobs Málaga
site:infojobs.net/oferta-trabajo farmacovigilancia malaga

# Pharmiweb — pharma-specific board
site:pharmiweb.com/job pharmacovigilance spain remote

# EuroPharmaJobs individual postings
site:europharmajobs.com/job pharmacovigilance spain

# Himalayas — tends to return individual job URLs
pharmacovigilance regulatory affairs spain remote site:himalayas.app

# RemoteFirstJobs
site:remotefirstjobs.com pharmacovigilance

# CRO-specific (PharmaLex, Ergomed, PrimeVigilance)
pharmacovigilance spain "PharmaLex" OR "Ergomed" OR "PrimeVigilance" job apply

# Spanish language
farmacovigilancia empleo remoto España 2026
```

**Step 3: Test dry run (no email)**

```bash
~/hunters/job-hunter.sh mar daily 3 --no-email 2>&1
```
Expected: output shows `[scrape]` log lines and job list with tier + URL.
Verify at least some jobs show scraped metadata in the output.

**Step 4: Commit**

```bash
cd ~/hunters && git add job-hunter.sh mar/sources.md && git commit -m "feat: add scrape step to pipeline, update sources.md with site: queries"
```

---

### Task 7: Build release binary

**Step 1: Build**

```bash
cd ~/hunters && cargo build --release 2>&1 | tail -10
```
Expected: compiles clean.

**Step 2: Full dry run**

```bash
~/hunters/job-hunter.sh mar daily 5 --no-email 2>&1
```
Expected: 5 jobs processed, scrape logs visible, dimension data in output.

**Step 3: Commit release binary note**

```bash
cd ~/hunters && git add -u && git commit -m "build: release binary updated with scrape + dimension rank + LLM render" --allow-empty
```

---

### Summary of files changed

| File | Action |
|------|--------|
| `~/hunters/Cargo.toml` | Add `toml`, `reqwest/blocking` |
| `~/hunters/dimensions.toml` | NEW — dimension config + weights |
| `~/hunters/render-prompt.md` | NEW — LLM render system prompt |
| `~/hunters/src/dimensions.rs` | NEW — TOML loader + helpers |
| `~/hunters/src/model.rs` | Expand `JobMetadata`, add `LanguageReq` |
| `~/hunters/src/scrape.rs` | NEW — Firecrawl LLM extraction |
| `~/hunters/src/rank.rs` | Add dimension weight deltas |
| `~/hunters/src/render.rs` | Replace Rust template with Claude API |
| `~/hunters/src/main.rs` | Register `scrape` subcommand |
| `~/hunters/job-hunter.sh` | Insert `scrape` into pipeline |
| `~/hunters/mar/sources.md` | Replace with `site:` targeted queries |
