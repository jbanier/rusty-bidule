#!/usr/bin/env python3
"""Analyze a local CSV with lightweight exploratory statistics."""

from __future__ import annotations

import argparse
import csv
from collections import Counter, defaultdict
from datetime import datetime, timezone
import json
import math
from pathlib import Path
import statistics
import sys
from typing import Any

NULL_VALUES = {"", "null", "none", "n/a", "na", "nan"}
TRUE_VALUES = {"true", "yes", "y", "1"}
FALSE_VALUES = {"false", "no", "n", "0"}
DATETIME_FORMATS = (
    "%Y-%m-%d",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%dT%H:%M:%SZ",
    "%m/%d/%Y",
    "%m/%d/%Y %H:%M:%S",
    "%d/%m/%Y",
    "%d/%m/%Y %H:%M:%S",
)
DEFAULT_MAX_ROWS = 100_000
DEFAULT_TOP_N = 20


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Analyze a CSV file for schema, group-bys, distinct values, outliers, trends, and anomalies."
    )
    parser.add_argument("--file", required=True, help="CSV file path")
    parser.add_argument("--delimiter", default="auto", help="CSV delimiter or 'auto'")
    parser.add_argument("--group-by", default="", help="Comma-separated group-by column names")
    parser.add_argument("--metrics", default="", help="Comma-separated numeric metric columns")
    parser.add_argument("--distinct-columns", default="", help="Comma-separated columns for distinct counts")
    parser.add_argument("--time-column", default="", help="Datetime column for trend analysis")
    parser.add_argument("--value-column", default="", help="Numeric value column for trend/anomaly analysis")
    parser.add_argument("--max-rows", type=int, default=DEFAULT_MAX_ROWS, help="Maximum data rows to read")
    parser.add_argument("--top-n", type=int, default=DEFAULT_TOP_N, help="Maximum rows/items per output section")
    return parser.parse_args()


def split_columns(raw: str) -> list[str]:
    return [part.strip() for part in raw.split(",") if part.strip()]


def resolve_file(path: str) -> Path:
    candidate = Path(path).expanduser()
    if candidate.is_file():
        return candidate.resolve()
    if not candidate.is_absolute():
        cwd_candidate = Path.cwd() / candidate
        if cwd_candidate.is_file():
            return cwd_candidate.resolve()
        project_root_candidate = Path(__file__).resolve().parents[3] / candidate
        if project_root_candidate.is_file():
            return project_root_candidate.resolve()
    raise FileNotFoundError(f"CSV file not found: {path}")


def is_null(value: Any) -> bool:
    if value is None:
        return True
    return str(value).strip().lower() in NULL_VALUES


def parse_number(value: Any) -> float | None:
    if is_null(value):
        return None
    text = str(value).strip()
    if text.endswith("%"):
        text = text[:-1].strip()
    if "," in text and text.count(",") == 1 and text.replace(",", "").replace(".", "").replace("-", "").isdigit():
        text = text.replace(",", "")
    try:
        number = float(text)
    except ValueError:
        return None
    if math.isfinite(number):
        return number
    return None


def parse_datetime_value(value: Any) -> datetime | None:
    if is_null(value):
        return None
    text = str(value).strip()
    try:
        parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
        return ensure_utc(parsed)
    except ValueError:
        pass
    for fmt in DATETIME_FORMATS:
        try:
            return ensure_utc(datetime.strptime(text, fmt))
        except ValueError:
            continue
    return None


def ensure_utc(value: datetime) -> datetime:
    if value.tzinfo is None:
        return value.replace(tzinfo=timezone.utc)
    return value.astimezone(timezone.utc)


def parse_bool(value: Any) -> bool | None:
    if is_null(value):
        return None
    text = str(value).strip().lower()
    if text in TRUE_VALUES:
        return True
    if text in FALSE_VALUES:
        return False
    return None


def csv_reader_kwargs(path: Path, delimiter: str) -> dict[str, Any]:
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        sample = handle.read(8192)
    if delimiter and delimiter != "auto":
        return {"delimiter": decode_delimiter(delimiter)}
    try:
        return {"dialect": csv.Sniffer().sniff(sample)}
    except csv.Error:
        return {"dialect": csv.excel}


def decode_delimiter(value: str) -> str:
    if value == r"\t" or value.lower() == "tab":
        return "\t"
    if len(value) != 1:
        raise ValueError("--delimiter must be one character, 'tab', '\\t', or 'auto'")
    return value


def read_csv(path: Path, delimiter: str, max_rows: int) -> tuple[list[str], list[dict[str, str]], bool]:
    reader_kwargs = csv_reader_kwargs(path, delimiter)
    rows: list[dict[str, str]] = []
    truncated = False
    with path.open("r", encoding="utf-8-sig", newline="") as handle:
        reader = csv.DictReader(handle, **reader_kwargs)
        if not reader.fieldnames:
            raise ValueError("CSV has no header row")
        fieldnames = [name.strip() if name else "" for name in reader.fieldnames]
        if any(not name for name in fieldnames):
            raise ValueError("CSV contains an empty column name")
        if len(set(fieldnames)) != len(fieldnames):
            raise ValueError("CSV contains duplicate column names")
        reader.fieldnames = fieldnames
        for index, row in enumerate(reader, start=1):
            if index > max_rows:
                truncated = True
                break
            rows.append({column: (row.get(column) or "") for column in fieldnames})
    return fieldnames, rows, truncated


def infer_schema(columns: list[str], rows: list[dict[str, str]]) -> tuple[dict[str, Any], list[str], list[str]]:
    schema: dict[str, Any] = {}
    numeric_columns: list[str] = []
    datetime_columns: list[str] = []
    row_count = len(rows)
    for column in columns:
        values = [row.get(column, "") for row in rows]
        non_null = [value for value in values if not is_null(value)]
        null_count = row_count - len(non_null)
        numeric_count = sum(1 for value in non_null if parse_number(value) is not None)
        datetime_count = sum(1 for value in non_null if parse_datetime_value(value) is not None)
        bool_count = sum(1 for value in non_null if parse_bool(value) is not None)
        unique_count = len(set(non_null))
        inferred = "string"
        if non_null:
            if bool_count == len(non_null):
                inferred = "boolean"
            elif numeric_count == len(non_null):
                inferred = "integer" if all(float(parse_number(value) or 0).is_integer() for value in non_null) else "number"
                numeric_columns.append(column)
            elif datetime_count == len(non_null):
                inferred = "datetime"
                datetime_columns.append(column)
            elif numeric_count / len(non_null) >= 0.9:
                inferred = "mostly_number"
                numeric_columns.append(column)
            elif datetime_count / len(non_null) >= 0.9:
                inferred = "mostly_datetime"
                datetime_columns.append(column)
        schema[column] = {
            "type": inferred,
            "non_null_count": len(non_null),
            "null_count": null_count,
            "null_rate": safe_ratio(null_count, row_count),
            "unique_count": unique_count,
            "sample_values": list(dict.fromkeys(non_null[:5])),
        }
    return schema, numeric_columns, datetime_columns


def safe_ratio(numerator: float, denominator: float) -> float | None:
    if denominator == 0:
        return None
    return round(numerator / denominator, 6)


def numeric_values(rows: list[dict[str, str]], column: str) -> list[tuple[int, float]]:
    values: list[tuple[int, float]] = []
    for row_index, row in enumerate(rows, start=1):
        number = parse_number(row.get(column, ""))
        if number is not None:
            values.append((row_index, number))
    return values


def numeric_summary(rows: list[dict[str, str]], columns: list[str]) -> dict[str, Any]:
    summary: dict[str, Any] = {}
    for column in columns:
        values = [number for _, number in numeric_values(rows, column)]
        if not values:
            continue
        item: dict[str, Any] = {
            "count": len(values),
            "min": round_float(min(values)),
            "max": round_float(max(values)),
            "mean": round_float(statistics.fmean(values)),
            "median": round_float(statistics.median(values)),
        }
        item["stddev"] = round_float(statistics.stdev(values)) if len(values) >= 2 else None
        summary[column] = item
    return summary


def round_float(value: float | None) -> float | None:
    if value is None or not math.isfinite(value):
        return None
    return round(value, 6)


def distinct_values(rows: list[dict[str, str]], columns: list[str], top_n: int) -> dict[str, Any]:
    output: dict[str, Any] = {}
    for column in columns:
        counter = Counter(row.get(column, "") for row in rows if not is_null(row.get(column, "")))
        output[column] = {
            "unique_count": len(counter),
            "top_values": [
                {"value": value, "count": count, "rate": safe_ratio(count, len(rows))}
                for value, count in counter.most_common(top_n)
            ],
            "truncated": len(counter) > top_n,
        }
    return output


def choose_distinct_columns(rows: list[dict[str, str]], columns: list[str], requested: list[str]) -> list[str]:
    if requested:
        return requested
    if not rows:
        return []
    selected = []
    limit = max(20, min(50, len(rows) // 5 or 1))
    for column in columns:
        values = [row.get(column, "") for row in rows if not is_null(row.get(column, ""))]
        unique_count = len(set(values))
        if values and unique_count <= limit:
            selected.append(column)
    return selected[:10]


def group_by_summary(
    rows: list[dict[str, str]],
    group_columns: list[str],
    metric_columns: list[str],
    top_n: int,
) -> dict[str, Any] | None:
    if not group_columns:
        return None
    groups: dict[tuple[str, ...], list[dict[str, str]]] = defaultdict(list)
    for row in rows:
        key = tuple(row.get(column, "") for column in group_columns)
        groups[key].append(row)
    ranked = sorted(groups.items(), key=lambda item: len(item[1]), reverse=True)
    output_groups = []
    for key, group_rows in ranked[:top_n]:
        aggregates = numeric_summary(group_rows, metric_columns)
        output_groups.append(
            {
                "key": dict(zip(group_columns, key, strict=True)),
                "count": len(group_rows),
                "rate": safe_ratio(len(group_rows), len(rows)),
                "metrics": aggregates,
            }
        )
    return {
        "columns": group_columns,
        "group_count": len(groups),
        "groups": output_groups,
        "truncated": len(groups) > top_n,
    }


def percentile(values: list[float], p: float) -> float:
    if not values:
        raise ValueError("percentile requires values")
    sorted_values = sorted(values)
    position = (len(sorted_values) - 1) * p
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return sorted_values[int(position)]
    weight = position - lower
    return sorted_values[lower] * (1 - weight) + sorted_values[upper] * weight


def outliers(rows: list[dict[str, str]], columns: list[str], top_n: int) -> dict[str, Any]:
    output: dict[str, Any] = {}
    for column in columns:
        indexed_values = numeric_values(rows, column)
        values = [value for _, value in indexed_values]
        if len(values) < 4:
            continue
        q1 = percentile(values, 0.25)
        q3 = percentile(values, 0.75)
        iqr = q3 - q1
        if iqr == 0:
            continue
        lower = q1 - 1.5 * iqr
        upper = q3 + 1.5 * iqr
        mean = statistics.fmean(values)
        stddev = statistics.stdev(values) if len(values) >= 2 else 0
        candidates = []
        for row_index, value in indexed_values:
            if value < lower or value > upper:
                z_score = (value - mean) / stddev if stddev else None
                candidates.append(
                    {
                        "row_number": row_index,
                        "value": round_float(value),
                        "z_score": round_float(z_score),
                        "side": "low" if value < lower else "high",
                    }
                )
        candidates.sort(key=lambda item: abs(item.get("z_score") or 0), reverse=True)
        output[column] = {
            "method": "iqr_1_5",
            "lower_bound": round_float(lower),
            "upper_bound": round_float(upper),
            "count": len(candidates),
            "examples": candidates[:top_n],
            "truncated": len(candidates) > top_n,
        }
    return output


def trend_and_anomalies(
    rows: list[dict[str, str]],
    time_column: str,
    value_column: str,
    top_n: int,
) -> tuple[dict[str, Any] | None, list[dict[str, Any]]]:
    if not time_column or not value_column:
        return None, []
    points = []
    for row_index, row in enumerate(rows, start=1):
        timestamp = parse_datetime_value(row.get(time_column, ""))
        value = parse_number(row.get(value_column, ""))
        if timestamp is not None and value is not None:
            points.append((timestamp, value, row_index))
    points.sort(key=lambda item: item[0])
    if len(points) < 2:
        return {
            "time_column": time_column,
            "value_column": value_column,
            "point_count": len(points),
            "warning": "Trend analysis requires at least two parseable time/value points.",
        }, []

    first_time, first_value, _ = points[0]
    last_time, last_value, _ = points[-1]
    x_values = [(timestamp - first_time).total_seconds() / 86400 for timestamp, _, _ in points]
    y_values = [value for _, value, _ in points]
    slope = linear_slope(x_values, y_values)
    delta = last_value - first_value
    trend = {
        "time_column": time_column,
        "value_column": value_column,
        "point_count": len(points),
        "start": {"time": first_time.isoformat(), "value": round_float(first_value)},
        "end": {"time": last_time.isoformat(), "value": round_float(last_value)},
        "delta": round_float(delta),
        "percent_change": safe_ratio(delta, first_value) if first_value else None,
        "slope_per_day": round_float(slope),
        "direction": "up" if slope > 0 else "down" if slope < 0 else "flat",
    }

    anomalies = value_anomalies(points, top_n) + delta_anomalies(points, top_n)
    anomalies.sort(key=lambda item: abs(item.get("score") or 0), reverse=True)
    return trend, anomalies[:top_n]


def linear_slope(x_values: list[float], y_values: list[float]) -> float:
    mean_x = statistics.fmean(x_values)
    mean_y = statistics.fmean(y_values)
    denominator = sum((x - mean_x) ** 2 for x in x_values)
    if denominator == 0:
        return 0.0
    numerator = sum((x - mean_x) * (y - mean_y) for x, y in zip(x_values, y_values, strict=True))
    return numerator / denominator


def value_anomalies(points: list[tuple[datetime, float, int]], top_n: int) -> list[dict[str, Any]]:
    values = [value for _, value, _ in points]
    if len(values) < 3:
        return []
    mean = statistics.fmean(values)
    stddev = statistics.stdev(values)
    if stddev == 0:
        return []
    anomalies = []
    for timestamp, value, row_index in points:
        z_score = (value - mean) / stddev
        if abs(z_score) >= 3:
            anomalies.append(
                {
                    "kind": "value_z_score",
                    "row_number": row_index,
                    "time": timestamp.isoformat(),
                    "value": round_float(value),
                    "score": round_float(z_score),
                }
            )
    return anomalies[:top_n]


def delta_anomalies(points: list[tuple[datetime, float, int]], top_n: int) -> list[dict[str, Any]]:
    if len(points) < 4:
        return []
    deltas = []
    for previous, current in zip(points, points[1:], strict=False):
        deltas.append((current[1] - previous[1], current))
    delta_values = [delta for delta, _ in deltas]
    stddev = statistics.stdev(delta_values)
    if stddev == 0:
        return []
    mean = statistics.fmean(delta_values)
    anomalies = []
    for delta, (timestamp, value, row_index) in deltas:
        z_score = (delta - mean) / stddev
        if abs(z_score) >= 3:
            anomalies.append(
                {
                    "kind": "delta_z_score",
                    "row_number": row_index,
                    "time": timestamp.isoformat(),
                    "value": round_float(value),
                    "delta": round_float(delta),
                    "score": round_float(z_score),
                }
            )
    return anomalies[:top_n]


def validate_columns(requested: list[str], columns: list[str], label: str) -> list[str]:
    missing = [column for column in requested if column not in columns]
    if missing:
        raise ValueError(f"{label} contains unknown column(s): {', '.join(missing)}")
    return requested


def main() -> int:
    args = parse_args()
    warnings: list[str] = []
    try:
        if args.max_rows <= 0:
            raise ValueError("--max-rows must be positive")
        top_n = max(1, args.top_n)
        path = resolve_file(args.file)
        columns, rows, truncated = read_csv(path, args.delimiter, args.max_rows)
        if truncated:
            warnings.append(f"Input was capped at {args.max_rows} rows.")
        schema, numeric_columns, datetime_columns = infer_schema(columns, rows)
        metric_columns = validate_columns(split_columns(args.metrics), columns, "metrics")
        if not metric_columns:
            metric_columns = numeric_columns[:10]
        group_columns = validate_columns(split_columns(args.group_by), columns, "group_by")
        requested_distinct = validate_columns(split_columns(args.distinct_columns), columns, "distinct_columns")
        distinct_columns = choose_distinct_columns(rows, columns, requested_distinct)
        if args.time_column and args.time_column not in columns:
            raise ValueError(f"time_column contains unknown column: {args.time_column}")
        if args.value_column and args.value_column not in columns:
            raise ValueError(f"value_column contains unknown column: {args.value_column}")
        if args.value_column and args.value_column not in numeric_columns:
            warnings.append(f"value_column '{args.value_column}' is not fully numeric; non-numeric values are ignored.")
        if args.time_column and args.time_column not in datetime_columns:
            warnings.append(f"time_column '{args.time_column}' is not fully datetime; unparseable values are ignored.")

        trend, time_anomalies = trend_and_anomalies(rows, args.time_column, args.value_column, top_n)
        result = {
            "file": str(path),
            "row_count": len(rows),
            "column_count": len(columns),
            "columns": columns,
            "schema": schema,
            "summary": {
                "numeric": numeric_summary(rows, metric_columns),
            },
            "distinct_values": distinct_values(rows, distinct_columns, top_n),
            "group_by": group_by_summary(rows, group_columns, metric_columns, top_n),
            "outliers": outliers(rows, metric_columns, top_n),
            "trends": trend,
            "anomalies": {
                "time_series": time_anomalies,
                "data_quality": data_quality_anomalies(schema, rows, top_n),
            },
            "warnings": warnings,
        }
        print(json.dumps(result, indent=2, sort_keys=True))
        return 0
    except Exception as exc:
        print(json.dumps({"error": str(exc)}, indent=2), file=sys.stderr)
        return 1


def data_quality_anomalies(schema: dict[str, Any], rows: list[dict[str, str]], top_n: int) -> list[dict[str, Any]]:
    anomalies = []
    row_count = len(rows)
    for column, info in schema.items():
        null_rate = info.get("null_rate")
        if null_rate is not None and null_rate >= 0.5:
            anomalies.append(
                {
                    "kind": "high_null_rate",
                    "column": column,
                    "null_rate": null_rate,
                    "null_count": info.get("null_count"),
                }
            )
        if row_count > 1 and info.get("unique_count") == 1:
            anomalies.append({"kind": "constant_column", "column": column})
    return anomalies[:top_n]


if __name__ == "__main__":
    raise SystemExit(main())
