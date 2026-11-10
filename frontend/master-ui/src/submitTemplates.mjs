// Starter programs for the Submit tab.
//
// Every entry is a program the repository already runs in its tests: `docs` entries
// are the documentation examples in frontend/src/lib/docs/examples.mjs, `repo` entries
// are the files in templates/hcode. submitTemplates.test.mjs keeps these texts identical
// to their sources, so a template cannot drift from the program that is verified.

export const SUBMIT_TEMPLATES = [
  {
    id: 'sum-list',
    docs: 'sum-list',
    label: { en: 'Add up a list', zh: '清單加總' },
    summary: { en: 'Adds every number in input.values.', zh: '把 input.values 裡的數字全部加起來。' },
    source: `let total = 0;
for value in get(input, "values") {
  let total = total + value;
}
total;`,
    input: `{"values": [10, 20, 30]}`,
  },
  {
    id: 'count-matches',
    docs: 'count-matches',
    label: { en: 'Count values above a threshold', zh: '計算達到門檻的數量' },
    summary: { en: 'Counts the values in input.values that reach input.threshold.', zh: '計算 input.values 中達到 input.threshold 的筆數。' },
    source: `let threshold = get(input, "threshold");
let count = 0;
for v in get(input, "values") {
  let count = if v >= threshold { count + 1 } else { count };
}
count;`,
    input: `{"values": [3, 8, 12, 5], "threshold": 5}`,
  },
  {
    id: 'largest-value',
    docs: 'largest-value',
    label: { en: 'Find the largest value', zh: '找出最大值' },
    summary: { en: 'Returns the largest number in input.values.', zh: '回傳 input.values 中最大的數字。' },
    source: `let values = get(input, "values");
let best = get(values, 0);
for v in values {
  let best = if v > best { v } else { best };
}
best;`,
    input: `{"values": [4, 17, 9]}`,
  },
  {
    id: 'batch-sum',
    repo: '03_batch_sum',
    label: { en: 'Summarize payment records', zh: '彙總付款紀錄' },
    summary: { en: 'Counts paid items and totals their amounts.', zh: '統計已付款的筆數與金額總和。' },
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
    input: `{
  "items": [
    { "status": "paid", "amount": 10 },
    { "status": "pending", "amount": 7 },
    { "status": "paid", "amount": 25 }
  ]
}`,
  },
  {
    id: 'policy-gate',
    repo: '01_policy_gate',
    label: { en: 'Approve or reject a request', zh: '核准或拒絕申請' },
    summary: { en: 'Decides from a user risk score and spend against a limit.', zh: '依使用者風險分數與花費是否在上限內決定核准與否。' },
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
    input: `{
  "user": {
    "verified": true,
    "risk_score": 21
  },
  "spend_cpt": 12,
  "max_cpt": 25
}`,
  },
  {
    id: 'weighted-score',
    repo: '02_weighted_score',
    label: { en: 'Weighted score and band', zh: '加權分數與等級' },
    summary: { en: 'Turns metrics into a weighted score and a band.', zh: '把多項指標換算成加權分數與等級。' },
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
    input: `{
  "metrics": {
    "quality": 90,
    "speed": 80,
    "reliability": 85
  }
}`,
  },
  {
    id: 'price-quote',
    repo: '04_price_quote',
    label: { en: 'Estimate a price and check a budget', zh: '估算價格並檢查預算' },
    summary: { en: 'Prices a job per host and checks it against a budget.', zh: '依主機數估算價格，並檢查是否在預算內。' },
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
    input: `{
  "requirements": {
    "cpu_score": 4,
    "gpu_score": 0,
    "memory_gb": 8,
    "storage_gb": 3
  },
  "host_count": 2,
  "max_cpt": 60
}`,
  },
  {
    id: 'route-task',
    repo: '05_route_task',
    label: { en: 'Choose a worker pool and priority', zh: '選擇工作池與優先順序' },
    summary: { en: 'Routes a task to a pool and priority from its requirements.', zh: '依任務需求決定工作池與優先順序。' },
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
    input: `{
  "task": {
    "kind": "analysis",
    "needs_gpu": false,
    "input_mb": 512
  },
  "labels": ["customer", "urgent"]
}`,
  },
];
