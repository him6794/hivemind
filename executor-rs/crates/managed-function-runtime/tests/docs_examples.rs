//! Programs published on the documentation pages.
//!
//! Mirror of frontend/src/lib/docs/examples.mjs. Update both files together.
//! Each program must produce the value, printed output, and usage units the
//! pages show, so a runtime change that invalidates a published example fails
//! here instead of on the site.

use managed_function_runtime::{ExecutionLimits, ManagedExecutor, Status, render_output};

/// (id, source, input, result, output, usage units)
const EXAMPLES: &[(&str, &str, &str, &str, &str, u64)] = &[
    (
        "sum-list",
        r#"let total = 0;
for value in get(input, "values") {
  let total = total + value;
}
total;"#,
        r#"{"values": [10, 20, 30]}"#,
        "60",
        "",
        25,
    ),
    (
        "sum-list-six",
        r#"let total = 0;
for value in get(input, "values") {
  let total = total + value;
}
total;"#,
        r#"{"values": [1, 2, 3, 4, 5, 6]}"#,
        "21",
        "",
        37,
    ),
    (
        "read-field",
        r#"get(input, "name");"#,
        r#"{"name": "Ada"}"#,
        "Ada",
        "",
        9,
    ),
    (
        "read-nested",
        r#"let order = get(input, "order");
get(order, "total") * 2;"#,
        r#"{"order": {"total": 42}}"#,
        "84",
        "",
        20,
    ),
    (
        "loop-count",
        r#"let count = 0;
for row in get(input, "rows") {
  let count = if get(row, "ok") { count + 1 } else { count };
}
count;"#,
        r#"{"rows": [{"ok": true}, {"ok": false}, {"ok": true}]}"#,
        "2",
        "",
        50,
    ),
    (
        "conditions-sign",
        r#"let x = get(input, "x");
if x > 0 { "positive" } else { if x == 0 { "zero" } else { "negative" } };"#,
        r#"{"x": -4}"#,
        "negative",
        "",
        19,
    ),
    (
        "contains-tag",
        r#"contains(get(input, "tags"), "beta");"#,
        r#"{"tags": ["alpha", "beta"]}"#,
        "true",
        "",
        16,
    ),
    (
        "function-double",
        r#"fn double(n) {
  return n * 2;
}
double(get(input, "n"));"#,
        r#"{"n": 21}"#,
        "42",
        "",
        18,
    ),
    (
        "factorial",
        r#"fn fact(n) {
  return if n <= 1 { 1 } else { n * fact(n - 1) };
}
fact(get(input, "n"));"#,
        r#"{"n": 10}"#,
        "3628800",
        "",
        155,
    ),
    (
        "list-build",
        r"let out = [0, 0, 0];
out[0] = 10;
out[1] = 20;
out[2] = 30;
out;",
        r"{}",
        "[10,20,30]",
        "",
        16,
    ),
    (
        "dict-build",
        r#"let person = {"name": "Ada"};
person["age"] = 36;
person;"#,
        r"{}",
        "{\"age\":36,\"name\":\"Ada\"}",
        "",
        8,
    ),
    (
        "list-negative-index",
        r#"let values = get(input, "values");
values[-1];"#,
        r#"{"values": [1, 2, 3]}"#,
        "3",
        "",
        14,
    ),
    (
        "text-greeting",
        r#""Hello, " + get(input, "name") + "!";"#,
        r#"{"name": "小明"}"#,
        "Hello, 小明!",
        "",
        13,
    ),
    (
        "text-bytes-chars",
        r#"let word = get(input, "word");
[len(word), word[0]];"#,
        r#"{"word": "你好"}"#,
        "[6,\"你\"]",
        "",
        21,
    ),
    (
        "print-log",
        r#"print("step 1");
print("step 2");
3;"#,
        r"{}",
        "3",
        "step 1\nstep 2\n",
        16,
    ),
    (
        "mean-list",
        r#"let values = get(input, "values");
let total = 0;
for v in values {
  let total = total + v;
}
total / len(values);"#,
        r#"{"values": [10, 20, 30]}"#,
        "20",
        "",
        35,
    ),
    (
        "largest-value",
        r#"let values = get(input, "values");
let best = get(values, 0);
for v in values {
  let best = if v > best { v } else { best };
}
best;"#,
        r#"{"values": [4, 17, 9]}"#,
        "17",
        "",
        40,
    ),
    (
        "smallest-value",
        r#"let values = get(input, "values");
let best = get(values, 0);
for v in values {
  let best = if v < best { v } else { best };
}
best;"#,
        r#"{"values": [4, 17, 9]}"#,
        "4",
        "",
        40,
    ),
    (
        "count-matches",
        r#"let threshold = get(input, "threshold");
let count = 0;
for v in get(input, "values") {
  let count = if v >= threshold { count + 1 } else { count };
}
count;"#,
        r#"{"values": [3, 8, 12, 5], "threshold": 5}"#,
        "3",
        "",
        52,
    ),
    (
        "records-total",
        r#"let total = 0;
for row in get(input, "rows") {
  let total = total + get(row, "amount");
}
total;"#,
        r#"{"rows": [{"amount": 12}, {"amount": 30}]}"#,
        "42",
        "",
        35,
    ),
    (
        "policy-gate",
        r#"let user = get(input, "user");
let spend = get(input, "spend_cpt");
let max = get(input, "max_cpt");
let verified = get(user, "verified");
let risk = get(user, "risk_score");

let allowed = if verified == true {
    if risk < 50 {
        if spend <= max { true } else { false }
    } else {
        false
    }
} else {
    false
};

{
    "allowed": allowed,
    "risk_score": risk,
    "spend_cpt": spend
};"#,
        r#"{"user": {"verified": true, "risk_score": 21}, "spend_cpt": 12, "max_cpt": 25}"#,
        "{\"allowed\":true,\"risk_score\":21,\"spend_cpt\":12}",
        "",
        64,
    ),
    (
        "weighted-score",
        r#"let metrics = get(input, "metrics");
let quality = get(metrics, "quality");
let speed = get(metrics, "speed");
let reliability = get(metrics, "reliability");

let score = quality * 5 + speed * 3 + reliability * 2;
let band = if score >= 850 {
    "gold"
} else {
    if score >= 700 { "silver" } else { "bronze" }
};

{
    "score": score,
    "band": band
};"#,
        r#"{"metrics": {"quality": 90, "speed": 80, "reliability": 85}}"#,
        "{\"band\":\"gold\",\"score\":860}",
        "",
        58,
    ),
    (
        "batch-sum",
        r#"let total = 0;
let paid_count = 0;

for item in get(input, "items") {
    let paid = get(item, "status") == "paid";
    let total = if paid { total + get(item, "amount") } else { total };
    let paid_count = if paid { paid_count + 1 } else { paid_count };
}

{
    "paid_count": paid_count,
    "paid_total": total,
    "input_count": len(get(input, "items"))
};"#,
        r#"{"items": [{"status": "paid", "amount": 10}, {"status": "pending", "amount": 7}, {"status": "paid", "amount": 25}]}"#,
        "{\"input_count\":3,\"paid_count\":2,\"paid_total\":35}",
        "",
        110,
    ),
    (
        "price-quote",
        r#"let req = get(input, "requirements");
let cpu = get(req, "cpu_score");
let gpu = get(req, "gpu_score");
let memory = get(req, "memory_gb");
let storage = get(req, "storage_gb");
let host_count = get(input, "host_count");
let budget = get(input, "max_cpt");

let per_host = 10 + cpu * 2 + gpu * 5 + memory + storage;
let total = per_host * host_count;

{
    "per_host_cpt": per_host,
    "total_cpt": total,
    "within_budget": total <= budget
};"#,
        r#"{"requirements": {"cpu_score": 4, "gpu_score": 0, "memory_gb": 8, "storage_gb": 3}, "host_count": 2, "max_cpt": 60}"#,
        "{\"per_host_cpt\":29,\"total_cpt\":58,\"within_budget\":true}",
        "",
        88,
    ),
    (
        "route-task",
        r#"let task = get(input, "task");
let kind = get(task, "kind");
let needs_gpu = get(task, "needs_gpu");
let input_mb = get(task, "input_mb");

let pool = if needs_gpu == true {
    "gpu_pool"
} else {
    if input_mb > 1000 {
        "storage_pool"
    } else {
        if kind == "analysis" { "cpu_pool" } else { "default_pool" }
    }
};

let priority = if contains(get(input, "labels"), "urgent") { 10 } else { 3 };

{
    "pool": pool,
    "priority": priority
};"#,
        r#"{"task": {"kind": "analysis", "needs_gpu": false, "input_mb": 512}, "labels": ["customer", "urgent"]}"#,
        "{\"pool\":\"cpu_pool\",\"priority\":10}",
        "",
        72,
    ),
    (
        "validate-fields",
        r#"let ok = true;
for key in ["name", "email"] {
  let ok = if contains(input, key) { ok } else { false };
}
ok;"#,
        r#"{"name": "Ada", "email": "ada@example.com"}"#,
        "true",
        "",
        30,
    ),
    (
        "lookup-default",
        r#"let limit = get(input, "limit");
if limit == null { 10 } else { limit };"#,
        r"{}",
        "10",
        "",
        15,
    ),
    (
        "lookup-default-set",
        r#"let limit = get(input, "limit");
if limit == null { 10 } else { limit };"#,
        r#"{"limit": 3}"#,
        "3",
        "",
        15,
    ),
];

#[test]
fn published_examples_produce_the_documented_results() {
    let mut mismatches = Vec::new();
    for (id, source, input, result, output, usage_units) in EXAMPLES {
        let run =
            match ManagedExecutor.execute_json_input(source, ExecutionLimits::default(), input) {
                Ok(run) => run,
                Err(error) => {
                    mismatches.push(format!("{id}: did not run: {error:?}"));
                    continue;
                }
            };
        let value = render_output(&run.value);
        if run.status != Status::Completed
            || value != *result
            || run.output != *output
            || run.receipt.usage_units != *usage_units
        {
            mismatches.push(format!(
                "{id}: value {value:?}, output {:?}, usage units {}",
                run.output, run.receipt.usage_units
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "published examples disagree with the runtime:\n{}",
        mismatches.join("\n")
    );
}
