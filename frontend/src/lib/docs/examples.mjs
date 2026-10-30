// Programs published on the documentation pages. Each entry is run by
// executor-rs/crates/managed-function-runtime/tests/docs_examples.rs, which
// checks the value, printed output, and usage units shown here. The frontend
// contract test checks that the Rust test still contains the same text.
//
// result: the final value as the Master log shows it (a top-level string is printed raw).
// usage:  usage units recorded by one successful replica.

export const EXAMPLES = [
  {
    id: 'sum-list',
    source: `let total = 0;
for value in get(input, "values") {
  let total = total + value;
}
total;`,
    input: '{"values": [10, 20, 30]}',
    result: '60',
    output: '',
    usage: 25,
  },
  {
    id: 'sum-list-six',
    source: `let total = 0;
for value in get(input, "values") {
  let total = total + value;
}
total;`,
    input: '{"values": [1, 2, 3, 4, 5, 6]}',
    result: '21',
    output: '',
    usage: 37,
  },
  {
    id: 'read-field',
    source: `get(input, "name");`,
    input: '{"name": "Ada"}',
    result: 'Ada',
    output: '',
    usage: 9,
  },
  {
    id: 'read-nested',
    source: `let order = get(input, "order");
get(order, "total") * 2;`,
    input: '{"order": {"total": 42}}',
    result: '84',
    output: '',
    usage: 20,
  },
  {
    id: 'loop-count',
    source: `let count = 0;
for row in get(input, "rows") {
  let count = if get(row, "ok") { count + 1 } else { count };
}
count;`,
    input: '{"rows": [{"ok": true}, {"ok": false}, {"ok": true}]}',
    result: '2',
    output: '',
    usage: 50,
  },
  {
    id: 'conditions-sign',
    source: `let x = get(input, "x");
if x > 0 { "positive" } else { if x == 0 { "zero" } else { "negative" } };`,
    input: '{"x": -4}',
    result: 'negative',
    output: '',
    usage: 19,
  },
  {
    id: 'contains-tag',
    source: `contains(get(input, "tags"), "beta");`,
    input: '{"tags": ["alpha", "beta"]}',
    result: 'true',
    output: '',
    usage: 16,
  },
  {
    id: 'function-double',
    source: `fn double(n) {
  return n * 2;
}
double(get(input, "n"));`,
    input: '{"n": 21}',
    result: '42',
    output: '',
    usage: 18,
  },
  {
    id: 'factorial',
    source: `fn fact(n) {
  return if n <= 1 { 1 } else { n * fact(n - 1) };
}
fact(get(input, "n"));`,
    input: '{"n": 10}',
    result: '3628800',
    output: '',
    usage: 155,
  },
  {
    id: 'list-build',
    source: `let out = [0, 0, 0];
out[0] = 10;
out[1] = 20;
out[2] = 30;
out;`,
    input: '{}',
    result: '[10,20,30]',
    output: '',
    usage: 16,
  },
  {
    id: 'dict-build',
    source: `let person = {"name": "Ada"};
person["age"] = 36;
person;`,
    input: '{}',
    result: '{"age":36,"name":"Ada"}',
    output: '',
    usage: 8,
  },
  {
    id: 'list-negative-index',
    source: `let values = get(input, "values");
values[-1];`,
    input: '{"values": [1, 2, 3]}',
    result: '3',
    output: '',
    usage: 14,
  },
  {
    id: 'text-greeting',
    source: `"Hello, " + get(input, "name") + "!";`,
    input: '{"name": "小明"}',
    result: 'Hello, 小明!',
    output: '',
    usage: 13,
  },
  {
    id: 'text-bytes-chars',
    source: `let word = get(input, "word");
[len(word), word[0]];`,
    input: '{"word": "你好"}',
    result: '[6,"你"]',
    output: '',
    usage: 21,
  },
  {
    id: 'print-log',
    source: `print("step 1");
print("step 2");
3;`,
    input: '{}',
    result: '3',
    output: 'step 1\nstep 2\n',
    usage: 16,
  },
  {
    id: 'mean-list',
    source: `let values = get(input, "values");
let total = 0;
for v in values {
  let total = total + v;
}
total / len(values);`,
    input: '{"values": [10, 20, 30]}',
    result: '20',
    output: '',
    usage: 35,
  },
  {
    id: 'largest-value',
    source: `let values = get(input, "values");
let best = get(values, 0);
for v in values {
  let best = if v > best { v } else { best };
}
best;`,
    input: '{"values": [4, 17, 9]}',
    result: '17',
    output: '',
    usage: 40,
  },
  {
    id: 'smallest-value',
    source: `let values = get(input, "values");
let best = get(values, 0);
for v in values {
  let best = if v < best { v } else { best };
}
best;`,
    input: '{"values": [4, 17, 9]}',
    result: '4',
    output: '',
    usage: 40,
  },
  {
    id: 'count-matches',
    source: `let threshold = get(input, "threshold");
let count = 0;
for v in get(input, "values") {
  let count = if v >= threshold { count + 1 } else { count };
}
count;`,
    input: '{"values": [3, 8, 12, 5], "threshold": 5}',
    result: '3',
    output: '',
    usage: 52,
  },
  {
    id: 'records-total',
    source: `let total = 0;
for row in get(input, "rows") {
  let total = total + get(row, "amount");
}
total;`,
    input: '{"rows": [{"amount": 12}, {"amount": 30}]}',
    result: '42',
    output: '',
    usage: 35,
  },
  {
    id: 'policy-gate',
    source: `let user = get(input, "user");
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
};`,
    input: '{"user": {"verified": true, "risk_score": 21}, "spend_cpt": 12, "max_cpt": 25}',
    result: '{"allowed":true,"risk_score":21,"spend_cpt":12}',
    output: '',
    usage: 64,
  },
  {
    id: 'weighted-score',
    source: `let metrics = get(input, "metrics");
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
};`,
    input: '{"metrics": {"quality": 90, "speed": 80, "reliability": 85}}',
    result: '{"band":"gold","score":860}',
    output: '',
    usage: 58,
  },
  {
    id: 'batch-sum',
    source: `let total = 0;
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
};`,
    input: '{"items": [{"status": "paid", "amount": 10}, {"status": "pending", "amount": 7}, {"status": "paid", "amount": 25}]}',
    result: '{"input_count":3,"paid_count":2,"paid_total":35}',
    output: '',
    usage: 110,
  },
  {
    id: 'price-quote',
    source: `let req = get(input, "requirements");
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
};`,
    input: '{"requirements": {"cpu_score": 4, "gpu_score": 0, "memory_gb": 8, "storage_gb": 3}, "host_count": 2, "max_cpt": 60}',
    result: '{"per_host_cpt":29,"total_cpt":58,"within_budget":true}',
    output: '',
    usage: 88,
  },
  {
    id: 'route-task',
    source: `let task = get(input, "task");
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
};`,
    input: '{"task": {"kind": "analysis", "needs_gpu": false, "input_mb": 512}, "labels": ["customer", "urgent"]}',
    result: '{"pool":"cpu_pool","priority":10}',
    output: '',
    usage: 72,
  },
  {
    id: 'validate-fields',
    source: `let ok = true;
for key in ["name", "email"] {
  let ok = if contains(input, key) { ok } else { false };
}
ok;`,
    input: '{"name": "Ada", "email": "ada@example.com"}',
    result: 'true',
    output: '',
    usage: 30,
  },
  {
    id: 'lookup-default',
    source: `let limit = get(input, "limit");
if limit == null { 10 } else { limit };`,
    input: '{}',
    result: '10',
    output: '',
    usage: 15,
  },
  {
    id: 'lookup-default-set',
    source: `let limit = get(input, "limit");
if limit == null { 10 } else { limit };`,
    input: '{"limit": 3}',
    result: '3',
    output: '',
    usage: 15,
  },
];
