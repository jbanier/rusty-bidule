---
name: data-analyst
description: Analyze a local CSV file for schema, distinct values, group-by summaries, numeric outliers, time trends, and simple anomaly detection without sending data to external services.
metadata:
  keywords: csv, data analysis, groupby, distinct values, outliers, trends, anomaly detection, profiling
---

# Data Analyst

Use this skill when the user asks to inspect or summarize a CSV file locally.

The analyzer returns structured JSON. Summarize the important findings for the user and call out warnings, row caps, parse failures, high-cardinality columns, and assumptions.

Constraints:

- Keep analysis local.
- Do not claim statistical significance; this tool provides exploratory checks.
- Prefer explicit `group_by`, `metrics`, `distinct_columns`, `time_column`, and `value_column` parameters when the user names columns.
- For trend and time-anomaly analysis, provide both `time_column` and `value_column`.
- If the CSV path is relative and direct lookup fails, the script also tries the repository root.

Tools:
  - name: Analyze CSV
    slug: analyze_csv
    description: Profile a CSV and compute schema, distinct values, group-by aggregates, outliers, trends, and anomalies. Required parameter: file. Optional parameters: delimiter, group_by, metrics, distinct_columns, time_column, value_column, max_rows, top_n.
    script: scripts/analyze_csv.py
    filesystem: read_only

## Examples

Basic profile:

```json
{"file": "data/events.csv"}
```

Group by a category and aggregate numeric columns:

```json
{"file": "data/events.csv", "group_by": "status", "metrics": "duration_ms,retry_count"}
```

Distinct values for selected columns:

```json
{"file": "data/events.csv", "distinct_columns": "status,region"}
```

Trend and anomaly checks:

```json
{"file": "data/events.csv", "time_column": "timestamp", "value_column": "duration_ms"}
```
