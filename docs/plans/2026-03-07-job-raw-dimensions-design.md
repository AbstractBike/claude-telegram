# Job Raw Dimensions — Design

## Overview

Enrich job postings with raw dimensions extracted from their actual pages (salary,
applicants, experience required, work mode, etc.) using Firecrawl LLM extraction.
Dimensions feed the ranker via a configurable weight map, and the renderer uses LLM
to produce friendly 2–3 line summaries per job.

## Pipeline

```
search → scrape → rank → render (LLM) → send
```

### Before
- `search`: Firecrawl search → returns URL + title + snippet (~200 chars)
- `rank`: TF-IDF cosine similarity against profile
- `render`: Rust template (daily) or Claude API (weekly)

### After
- `search`: same, but queries use `site:` operators to target individual job pages
- `scrape`: Firecrawl `/v1/scrape` with `onlyMainContent` + JSON schema → dimensions
- `rank`: TF-IDF + weighted dimension bonuses/penalties (from `dimensions.toml`)
- `render`: Claude API always — friendly summary paragraph + 2–3 lines per job

---

## 1. Search Queries — `sources.md`

Replace aggregator-page queries with `site:` operators pointing to job detail URL paths:

```
# LinkedIn job detail pages
site:linkedin.com/jobs/view pharmacovigilance spain remote

# InfoJobs individual listings
site:infojobs.net/oferta-trabajo pharmacovigilancia españa

# Pharmiweb — pharma-specific board
site:pharmiweb.com/job pharmacovigilance spain remote

# EuroPharmaJobs individual postings
site:europharmajobs.com/job pharmacovigilance spain

# Glassdoor job detail
site:glassdoor.com/job-listing pharmacovigilance spain

# Aggregators that tend to return individual job URLs
pharmacovigilance regulatory affairs spain remote himalayas
site:remotefirstjobs.com pharmacovigilance

# CRO-specific queries
pharmacovigilance spain "PharmaLex" OR "Ergomed" OR "PrimeVigilance" job apply

# Spanish language
site:infojobs.net/oferta-trabajo farmacovigilancia malaga
farmacovigilancia empleo remoto España 2026
```

---

## 2. Scrape Step — `hunter-engine scrape <slug>`

New subcommand inserted after `search` in the pipeline.

### Firecrawl call per job

```json
POST /v1/scrape
{
  "url": "<job_url>",
  "formats": ["json"],
  "onlyMainContent": true,
  "jsonOptions": {
    "extractionPrompt": "Extract job posting dimensions. Return only fields that are explicitly present in the page.",
    "extractionSchema": "<generated from dimensions.toml extract=true fields>"
  }
}
```

- `onlyMainContent: true` strips nav/header/footer/ads
- Schema is generated at runtime from `dimensions.toml` (only `extract = true` fields)
- Response: `{ "json": { "salary_min": 42000, "work_mode": "remote", ... } }`
- On scrape failure or timeout: job continues with `metadata.scraped = false`, no error

### Platform detection

Detected from URL domain, stored in `metadata.platform`:
- `linkedin.com` → `linkedin`
- `infojobs.net` → `infojobs`
- `glassdoor.com` → `glassdoor`
- etc.

---

## 3. Data Model — `JobMetadata` additions

```rust
pub struct JobMetadata {
    // Engagement
    pub days_published: Option<u32>,
    pub applicants: Option<u32>,
    pub is_easy_apply: Option<bool>,
    pub is_promoted: Option<bool>,

    // Compensation (replaces salary_range for structured data)
    pub salary_range: Option<String>,    // kept for display/fallback
    pub salary_min: Option<u32>,
    pub salary_max: Option<u32>,
    pub salary_currency: Option<String>,
    pub salary_period: Option<String>,   // annual | monthly | hourly

    // Requirements
    pub yoe_min: Option<u32>,            // replaces experience_years
    pub yoe_max: Option<u32>,
    pub education_level: Option<String>,
    pub hard_skills: Option<Vec<String>>,
    pub languages: Option<Vec<LanguageReq>>,

    // Role
    pub work_mode: Option<String>,       // remote | hybrid | onsite
    pub contract_type: Option<String>,
    pub seniority: Option<String>,
    pub employment_type: Option<String>, // kept from existing

    // Company
    pub company_name: Option<String>,
    pub company_description: Option<String>, // kept from existing
    pub company_size: Option<String>,
    pub company_industry: Option<String>,
    pub is_agency: Option<bool>,

    // Content
    pub job_description: Option<String>, // full extracted job description text

    // Traceability
    pub platform: Option<String>,
    pub scraped: bool,
    pub scraped_at: Option<DateTime<Utc>>,
}

pub struct LanguageReq {
    pub lang: String,
    pub level: Option<String>,
}
```

---

## 4. Dimensions Config — `~/hunters/dimensions.toml`

Drives both extraction schema generation and rank weight map.

```toml
# Dimensions extracted from job pages + their influence on ranking score.
# weight = 0   → extract only, no ranking effect
# weight > 0   → boosts score
# weight < 0   → penalizes score
# values = {}  → categorical mapping (value → weight delta)
# If dimension is absent (None), it is silently skipped in ranking.

[dimensions.job_description]
extract = true
weight = 0              # full text used by TF-IDF separately

[dimensions.days_published]
extract = true
weight = -0.05          # older = penalize

[dimensions.applicants]
extract = true
weight = -0.08          # high competition = penalize

[dimensions.salary_max]
extract = true
weight = 0.15           # higher salary = boost

[dimensions.yoe_min]
extract = true
weight = -0.10          # high bar = penalize

[dimensions.is_easy_apply]
extract = true
weight = 0.05

[dimensions.is_agency]
extract = true
weight = -0.05

[dimensions.work_mode]
extract = true
values = { remote = 0.20, hybrid = 0.08, onsite = -0.10 }

[dimensions.hard_skills]
extract = true
weight = 0

[dimensions.contract_type]
extract = true
values = { permanent = 0.10, temporary = -0.05, freelance = 0.0 }

[dimensions.salary_min]
extract = true
weight = 0

[dimensions.salary_currency]
extract = true
weight = 0

[dimensions.salary_period]
extract = true
weight = 0

[dimensions.yoe_max]
extract = true
weight = 0

[dimensions.education_level]
extract = true
weight = 0

[dimensions.languages]
extract = true
weight = 0

[dimensions.seniority]
extract = true
weight = 0

[dimensions.is_easy_apply]
extract = true
weight = 0.05

[dimensions.company_size]
extract = true
weight = 0

[dimensions.is_agency]
extract = true
weight = -0.05

[dimensions.days_published]
extract = true
weight = -0.05
```

---

## 5. Rank — Dimension Weights

Score formula:

```
score_final = score_tfidf + Σ dimension_delta(dim, value)
```

### Numeric dimensions

```
delta = normalize(value) * weight
```

Normalization per field:
- `days_published`: `min(value / 30.0, 1.0)` (cap at 30 days)
- `applicants`: `min(value / 500.0, 1.0)` (cap at 500)
- `salary_max`: `min(value / 100_000.0, 1.0)`
- `yoe_min`: `min(value / 10.0, 1.0)` (cap at 10 years)

### Boolean dimensions

```
delta = if value { weight } else { 0.0 }
```

### Categorical dimensions

```
delta = values.get(value).unwrap_or(0.0)
```

### Missing values

If `metadata` is `None` or a specific field is `None` → `delta = 0.0`, silently skipped.
Ranking still works with pure TF-IDF for jobs without scraped metadata.

### TF-IDF uses `job_description`

When `metadata.job_description` is `Some(text)`, rank uses it instead of the
short `description` snippet for TF-IDF scoring.

---

## 6. Render — LLM Summaries (all modes)

Replace Rust template renderer with Claude API for all modes (not just weekly).

### Input to Claude

```
System: <render-prompt.md>

User:
LANGUAGE: es
CANDIDATE: <profile.md first 500 chars>

JOBS ({{count}}):
{% for job %}
---
TITLE: {{title}}
URL: {{url}}
TIER: {{tier}} | SCORE: {{score}}
{% if salary_min %}SALARY: {{salary_min}}–{{salary_max}} {{currency}}/{{period}}{% endif %}
{% if work_mode %}WORK_MODE: {{work_mode}}{% endif %}
{% if contract_type %}CONTRACT: {{contract_type}}{% endif %}
{% if yoe_min %}YOE: {{yoe_min}}{% if yoe_max %}–{{yoe_max}}{% endif %}{% endif %}
{% if applicants %}APPLICANTS: {{applicants}}{% endif %}
{% if days_published %}DAYS_PUBLISHED: {{days_published}}{% endif %}
{% if hard_skills %}SKILLS: {{hard_skills | join(", ")}}{% endif %}
DESCRIPTION: {{job_description or description}}
{% endfor %}
```

### Output format (HTML email)

- **Header paragraph**: 3–5 sentences, friendly tone, highlights best opportunities
- **Per-job block** (Tier 1 styled differently):
  - Title + link
  - 2–3 line summary covering: fit reason, key requirements, salary/mode if known
  - Pill badges: tier, work mode, contract type
- **Footer**: count, date, sources used

### `~/hunters/render-prompt.md`

New prompt file replacing `email-prompt.md` for daily mode.
`weekly-prompt.md` kept for backward compatibility (weekly mode gets richer digest).

---

## 7. `job-hunter.sh` changes

```bash
RANKED=$(timeout "$TIMEOUT" bash -c \
  '"$1" search "$2" "$3" --max "$4" 2>&2 \
   | "$1" scrape "$2" 2>&2 \
   | "$1" rank "$2" 2>&2' \
  -- "$ENGINE" "$SLUG" "$MODE" "$MAX_RESULTS")
```

---

## 8. File layout additions

```
~/hunters/
├── dimensions.toml          # NEW: dimension definitions + weights
├── render-prompt.md         # NEW: LLM render prompt (replaces email-prompt.md daily)
├── email-prompt.md          # kept (weekly fallback)
├── weekly-prompt.md         # kept
├── job-hunter.sh            # updated: adds scrape step
└── src/
    ├── scrape.rs            # NEW: scrape subcommand
    ├── model.rs             # updated: JobMetadata + LanguageReq
    ├── rank.rs              # updated: dimension weights from dimensions.toml
    └── render.rs            # updated: always LLM, uses dimensions
```

---

## 9. Cost estimate

Per job (daily mode, 10 jobs):
- Firecrawl scrape + LLM extraction: ~5 credits/job (local instance, effectively free)
- Claude render (all 10 jobs): ~2000 tokens input + 800 output ≈ $0.004
- Total daily: ~$0.004 — negligible

---

## Open questions

- LinkedIn blocks scrapers aggressively — may need fallback to description-only for those URLs
- `dimensions.toml` lives in `~/hunters/` (global) — per-person overrides TBD
