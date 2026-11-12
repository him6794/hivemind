// English text of the documentation pages. Structure and slugs come from
// structure.mjs. Programs are referenced by id from examples.mjs, which the
// Rust test runs. Reference sections render the data in hivemind-site-data.mjs.

export const docsGroupsEn = [
  { id: 'start', title: 'Get started' },
  { id: 'tutorials', title: 'Tutorials' },
  { id: 'templates', title: 'Templates' },
  { id: 'reference', title: 'Reference' },
];

export const docsPagesEn = [
  {
    slug: 'introduction',
    group: 'start',
    title: 'Introduction',
    description: 'hcode is the task language for Hivemind. A task is one program, its JSON input, and a charge cap. These pages explain how to write, test, and price a task, with programs you can copy.',
    blocks: [
      { type: 'h2', id: 'how-it-works', text: 'How a task runs' },
      { type: 'reference', name: 'flow' },
      { type: 'h2', id: 'terms', text: 'Terms' },
      { type: 'reference', name: 'terms' },
      { type: 'h2', id: 'start-here', text: 'Where to start' },
      { type: 'p', text: 'If you are new to hcode, follow Installation and then Your first task. Each tutorial covers one part of the language. The template pages hold complete programs you can adapt.' },
      { type: 'links', slugs: ['installation', 'first-task', 'read-input', 'templates-numbers'] },
    ],
  },
  {
    slug: 'installation',
    group: 'start',
    title: 'Installation',
    description: 'Install Master on Windows, sign in, and check the account balance. Install Worker only if you want to share this computer.',
    blocks: [
      { type: 'h2', id: 'requirements', text: 'Before you start' },
      {
        type: 'list',
        items: [
          '64-bit Windows.',
          'An internet connection.',
          'Microsoft Edge WebView2 Runtime is recommended. Without it, the app opens in your default browser.',
        ],
      },
      { type: 'h2', id: 'master', text: 'Install Master' },
      {
        type: 'steps',
        items: [
          { title: 'Create an account', body: 'Open the account page on this website and register with a username and a password of at least 8 characters.' },
          { title: 'Download Master', body: 'Open the download page and download the Master package. The page lists a SHA-256 value for each package.' },
          {
            title: 'Check the file',
            body: 'Open PowerShell in the folder that holds the download and compare the hash. The output must match the value on the download page.',
            code: { lang: 'powershell', code: 'Get-FileHash .\\<file-name> -Algorithm SHA256' },
          },
          { title: 'Run Master', body: 'Run the setup file, then open Hivemind Master from the Start menu. If Windows warns about an app it does not recognize, check the download address and the SHA-256 value again before you continue.' },
          { title: 'Sign in', body: 'Enter the account you created. Master uses it to connect to the network.' },
          { title: 'Check the balance', body: 'The balance is shown in CPT. A new account starts at 0.00.' },
        ],
      },
      { type: 'h2', id: 'worker', text: 'Share this computer (optional)' },
      { type: 'p', text: 'Worker runs replicas of other users tasks on this computer. Read the Trust and security page before you turn sharing on.' },
      { type: 'p', text: 'In Worker settings, choose always (default) or idle sharing and an idle duration. Windows uses the last keyboard or mouse input. Headless Linux uses system CPU utilization below the configured threshold for that duration; the UI identifies this source. While active, Worker reports BUSY and locally refuses new work; existing tasks finish. Nodepool can still select a BUSY Worker: a refusal can prevent quorum, and valid work by other replicas may still incur charges. Sign in to the local Worker UI to change these settings.' },
      {
        type: 'steps',
        items: [
          { title: 'Download Worker', body: 'Use the same download page. Worker is a separate package.' },
          { title: 'Sign in', body: 'The first sign-in connects this computer to the network.' },
          { title: 'Keep it running', body: 'Leave Worker open while you want to receive tasks.' },
        ],
      },
      { type: 'callout', tone: 'note', text: 'Replicas can run on computers that other users share, and the input is sent to them. Do not put passwords or private data in a task.' },
      { type: 'links', slugs: ['first-task', 'introduction'] },
    ],
  },
  {
    slug: 'first-task',
    group: 'start',
    title: 'Your first task',
    description: 'Submit a program that adds three numbers. You will see the result, the usage units one replica records, and what the task costs.',
    blocks: [
      { type: 'h2', id: 'steps', text: 'Steps' },
      { type: 'reference', name: 'quickstart' },
      { type: 'h2', id: 'program', text: 'The program' },
      { type: 'example', id: 'sum-list' },
      { type: 'h2', id: 'charge', text: 'What the task costs' },
      { type: 'p', text: 'Each successful replica of this program records 25 usage units, so three replicas record 75 units. The platform fee is 10% of that usage, rounded down, which is 7 CPT. The task is charged 82 CPT, and the rest of the 999 CPT hold, which is 917 CPT, is released.' },
      { type: 'p', text: 'The same program over six values records 37 units. Each extra value adds 4 units. The tutorials explain where those units come from.' },
      { type: 'links', slugs: ['read-input', 'loops', 'budget'] },
    ],
  },
  {
    slug: 'read-input',
    group: 'tutorials',
    title: 'Input and output',
    description: 'Programs read the JSON you submit through the name input, and they print text to the log. This lesson covers top-level fields, nested objects, missing fields, and printed output.',
    blocks: [
      { type: 'h2', id: 'input-name', text: 'The input name' },
      { type: 'p', text: '`input` holds the JSON document from the task input, already parsed. `get(input, "name")` returns the value stored under the key `name`.' },
      { type: 'example', id: 'read-field' },
      { type: 'h2', id: 'nested', text: 'Nested fields' },
      { type: 'p', text: 'Call `get` again on the object you read. Each call moves one level down the document.' },
      { type: 'example', id: 'read-nested' },
      { type: 'h2', id: 'missing', text: 'Missing fields' },
      {
        type: 'table',
        head: ['Form', 'When the key is missing'],
        rows: [
          ['`get(d, "key")`', 'Returns `null`, and the program continues.'],
          ['`d["key"]`', 'Fails with `key_error`.'],
        ],
      },
      { type: 'p', text: 'Compare the value with `null` to choose a default. The first example below has no limit in its input, so it returns 10. The second has a limit of 3, and returns 3.' },
      { type: 'example', id: 'lookup-default' },
      { type: 'example', id: 'lookup-default-set' },
      { type: 'h2', id: 'printing', text: 'Printed text' },
      { type: 'p', text: '`print(value);` adds text to the printed output. The final value is reported separately, so this program prints two lines and returns 3.' },
      { type: 'example', id: 'print-log' },
      { type: 'callout', tone: 'note', text: 'Printed text counts toward the output limit of 1 MiB. A program that prints in a long loop can reach it.' },
      { type: 'links', slugs: ['loops', 'collections'] },
    ],
  },
  {
    slug: 'loops',
    group: 'tutorials',
    title: 'Loops and accumulators',
    description: '`for` runs its block once for each element of a list. Inside the loop, `let` updates a variable, and the updated value is still there after the loop ends.',
    blocks: [
      { type: 'h2', id: 'adding', text: 'Adding up a list' },
      { type: 'p', text: 'The loop variable `value` takes each element of the list in turn. `let total = total + value;` rebinds `total` to the new sum, so the value after the loop is the sum of all elements.' },
      { type: 'example', id: 'sum-list' },
      { type: 'h2', id: 'let', text: 'Why let is required' },
      { type: 'p', text: 'Hcode has no bare assignment. A line such as `total = total + value;` is rejected with `parse_error`.' },
      { type: 'code', lang: 'hcode', code: 'let total = 0;\nfor value in get(input, "values") {\n  total = total + value;\n}\ntotal;' },
      { type: 'callout', tone: 'warning', text: 'The program above fails with parse_error. Put let in front of total inside the loop.' },
      { type: 'h2', id: 'cost', text: 'Usage grows with the list' },
      { type: 'p', text: 'Every loop iteration adds usage units. The sum over three values records 25 units, and the sum over six values records 37, so each extra value adds 4 units.' },
      {
        type: 'table',
        head: ['Input', 'Usage units recorded by one replica'],
        rows: [
          ['Three values: 10, 20, 30', '25'],
          ['Six values: 1 to 6', '37'],
        ],
      },
      { type: 'example', id: 'sum-list-six' },
      { type: 'h2', id: 'lists-only', text: 'What the loop accepts' },
      { type: 'p', text: '`for` iterates over lists only. A dict, a string, or a number in that position fails with `type_error`.' },
      { type: 'links', slugs: ['conditions', 'functions'] },
    ],
  },
  {
    slug: 'conditions',
    group: 'tutorials',
    title: 'Conditions and filters',
    description: '`if` chooses between two values. It is an expression, so it always has an else branch, and each branch holds one expression.',
    blocks: [
      { type: 'h2', id: 'choose', text: 'Choosing a value' },
      { type: 'p', text: 'The outer `if` returns one of three results. The inner `if` sits in the else branch, so it is also a single expression.' },
      { type: 'example', id: 'conditions-sign' },
      { type: 'h2', id: 'update', text: 'Updating a value under a condition' },
      { type: 'p', text: 'In an expression, a branch holds one expression. On CPU hcode, an `if` at the start of a statement can hold several statements, as in `if get(row, "ok") { let count = count + 1; }`, which changes the variable only when the condition holds. The expression below rebinds `count` in one step.' },
      { type: 'code', lang: 'hcode', code: 'let count = if get(row, "ok") { count + 1 } else { count };' },
      { type: 'example', id: 'loop-count' },
      { type: 'h2', id: 'contains', text: 'Checking for a value' },
      { type: 'p', text: '`contains` answers whether a list holds an equal element, whether a dict has a key, or whether a string includes a piece of text.' },
      { type: 'example', id: 'contains-tag' },
      { type: 'h2', id: 'operators', text: 'Operators' },
      {
        type: 'table',
        head: ['Kind', 'Operators'],
        rows: [
          ['Arithmetic', '`+`  `-`  `*`  `/`'],
          ['Comparison', '`==`  `!=`  `<`  `<=`  `>`  `>=`'],
          ['Boolean', '`and`  `or`  `not`'],
        ],
      },
      { type: 'p', text: 'Division of integers truncates toward zero, so `-5 / 2` is `-2`.' },
      { type: 'links', slugs: ['functions', 'collections'] },
    ],
  },
  {
    slug: 'functions',
    group: 'tutorials',
    title: 'Functions and recursion',
    description: 'Define a function with fn at the top level. It takes arguments, returns a value, and can call itself up to the depth limit.',
    blocks: [
      { type: 'h2', id: 'define', text: 'Defining and calling a function' },
      { type: 'p', text: '`return` ends the function and gives back its value. A function without `return` gives back the value of its last expression.' },
      { type: 'example', id: 'function-double' },
      { type: 'h2', id: 'recursion', text: 'Recursion' },
      { type: 'p', text: 'A function can call itself, and each call adds one level of depth. The factorial of 10 below makes ten nested calls and records 155 usage units, because every call adds its own cost.' },
      { type: 'example', id: 'factorial' },
      { type: 'h2', id: 'depth', text: 'Call depth' },
      { type: 'p', text: 'Calls can nest up to 64 levels. A deeper call stops the task with `call_depth_exceeded`. A recursion without a stopping case always reaches that limit.' },
      { type: 'callout', tone: 'note', text: 'Write the stopping case first, such as `if n <= 1 { 1 }`, so the recursion ends for every input.' },
      { type: 'h2', id: 'rules', text: 'Rules' },
      {
        type: 'list',
        items: [
          'Define functions at the top level.',
          'Calls nest up to 64 levels.',
          'A function gives back the value of its return statement, or the value of its last expression when there is no return.',
        ],
      },
      { type: 'links', slugs: ['collections', 'budget'] },
    ],
  },
  {
    slug: 'collections',
    group: 'tutorials',
    title: 'Lists and maps',
    description: 'Lists are written with square brackets. Maps use braces and string keys. Both change through index assignment on a variable.',
    blocks: [
      { type: 'h2', id: 'lists', text: 'Lists' },
      { type: 'p', text: 'Indexes start at 0. `out[0] = 10;` replaces the first element of the list held by `out`.' },
      { type: 'example', id: 'list-build' },
      { type: 'h2', id: 'maps', text: 'Maps' },
      { type: 'p', text: 'Keys are strings. Map keys are shown in sorted order, so `age` appears before `name` in the result, whatever order the program wrote them in.' },
      { type: 'example', id: 'dict-build' },
      { type: 'h2', id: 'negative', text: 'Negative indexes' },
      { type: 'p', text: 'Index `-1` is the last element and `-2` is the one before it. Text indexes count characters from the end in the same way.' },
      { type: 'example', id: 'list-negative-index' },
      { type: 'h2', id: 'assignment', text: 'Assignment rules' },
      {
        type: 'list',
        items: [
          'The target of an index assignment must be a variable, as in `values[0] = 1;`.',
          'Nested targets such as `rows[0][1] = 5;` are not supported.',
          'Reading past the end of a list fails with `index_error`. Use `get` when a missing element should give `null` instead.',
        ],
      },
      { type: 'links', slugs: ['text', 'budget'] },
    ],
  },
  {
    slug: 'text',
    group: 'tutorials',
    title: 'Text and Unicode',
    description: 'Strings join with +. len counts UTF-8 bytes, while an index counts characters. Put non-ASCII text in the input, not in the program.',
    blocks: [
      { type: 'h2', id: 'join', text: 'Joining text' },
      { type: 'p', text: 'The + operator joins two strings. A value read from the input joins the same way.' },
      { type: 'example', id: 'text-greeting' },
      { type: 'h2', id: 'bytes', text: 'Bytes and characters' },
      { type: 'p', text: 'The two characters of 你好 take six bytes in UTF-8, so len returns 6. The index word[0] returns one character, 你.' },
      { type: 'example', id: 'text-bytes-chars' },
      { type: 'h2', id: 'source', text: 'Text in the program' },
      { type: 'p', text: 'Source strings use double quotes. The supported escapes are a backslash in front of a double quote, a backslash, n, r, or t.' },
      { type: 'callout', tone: 'warning', text: 'Source literals do not reliably keep non-ASCII characters, and \\uXXXX escapes are not accepted. Send Chinese or other non-ASCII text in the JSON input.' },
      { type: 'h2', id: 'compare', text: 'Comparing text' },
      { type: 'p', text: 'Use == to compare two strings. The batch and routing templates compare a status or label field with a fixed string this way.' },
      { type: 'links', slugs: ['collections', 'templates-text'] },
    ],
  },
  {
    slug: 'budget',
    group: 'tutorials',
    title: 'Usage and max_cpt',
    description: 'Each replica records usage units while it runs, and the charge cap covers the whole task. Measure a small case first, then set max_cpt for the real input.',
    blocks: [
      { type: 'h2', id: 'units', text: 'Usage units' },
      { type: 'p', text: 'One usage unit is one CPT of replica usage. Expressions, statements, function calls, and loop iterations each add units. A run reports the units it recorded in its receipt.' },
      { type: 'example', id: 'sum-list' },
      { type: 'h2', id: 'measure', text: 'Measuring before you submit' },
      {
        type: 'steps',
        items: [
          { title: 'Run a small input', body: 'Submit the program with a short input that has the same shape as the real one, and read the usage units from the result.' },
          { title: 'Scale the estimate', body: 'Work that repeats for each element adds units for each element. The sum over six values costs 37 units, and the sum over three costs 25.' },
          { title: 'Set the cap', body: 'The cap that gives each replica U units is 3 × U + floor(3 × U / 10). For U = 25, that is 82 CPT.' },
        ],
      },
      { type: 'h2', id: 'cap-table', text: 'Cap and budget' },
      {
        type: 'table',
        head: ['max_cpt', 'Budget per replica', 'Worst-case hold'],
        rows: [
          ['82', '25', '82'],
          ['100', '30', '99'],
          ['1000', '303', '999'],
        ],
      },
      { type: 'p', text: 'The budget per replica is the largest whole number B for which 3 × B + floor(3 × B / 10) does not exceed the cap. The fee comes out of the same cap, so the cap is never exceeded.' },
      { type: 'h2', id: 'exhausted', text: 'When the budget runs out' },
      { type: 'p', text: 'A replica that uses its whole budget stops with `budget_exhausted`. Work it completed before the stop can still be billed if the task settles with a quorum. Raise max_cpt or reduce the work.' },
      { type: 'callout', tone: 'note', text: 'Automatic paid retries are disabled. Retry work is never charged to the task owner.' },
      { type: 'links', slugs: ['billing', 'master-api'] },
    ],
  },
  {
    "slug": "pipelines-models",
    "group": "tutorials",
    "title": "Pipelines and models",
    "description": "Run dependent stages, split a feed-forward CPU model, and try a small federated training example with the local CLI.",
    "blocks": [
      {
        "type": "h2",
        "id": "flow",
        "text": "Pass results between stages"
      },
      {
        "type": "p",
        "text": "A pipeline JSON file contains stages, each with items. Each item names a source file, optional params or params_file, artifacts, and max_cpt. Paths are relative to the pipeline file. Tasks receive {params, previous}; previous holds the preceding stage outputs in item order. The first stage receives [pipeline.input]. Return a JSON value; printed text cannot feed the next stage."
      },
      {
        "type": "h2",
        "id": "run",
        "text": "Check and run from your Master"
      },
      {
        "type": "code",
        "lang": "powershell",
        "code": "hivemind-bin pipeline check ./pipeline.json\nhivemind-bin pipeline run ./pipeline.json --api http://127.0.0.1:8082 --username alice --replicas 3 --max-total-cpt 10000 --task-prefix demo --out ./results.json"
      },
      {
        "type": "p",
        "text": "Set HIVEMIND_PASSWORD in your local environment. check runs every stage locally and reports usage and output sizes. Use the same --replicas value (2–7, default 3) for model split, pipeline check and pipeline run. run submits hcode tasks through Master and waits for every item before advancing. Missing caps are measured locally; --no-local-run requires every max_cpt to be written. Caps must cover the resource quote. The total budget includes previous attempts and reserves unsettled tasks at their caps."
      },
      {
        "type": "h2",
        "id": "resume",
        "text": "Continue an interrupted run"
      },
      {
        "type": "p",
        "text": "Repeat the command with --resume and the same prefix, output path, account, API and resource/replica settings. Keep results.json.state.json, which records each attempt before submission. Completed items are reused; failures get IDs ending in -r1, -r2. Changed input, source, parameters or weights prevent resume. Caps and the total budget may increase. Partial failures are recorded; already submitted tasks can still finish and incur charges after the CLI stops."
      },
      {
        "type": "h2",
        "id": "model",
        "text": "Split a CPU model"
      },
      {
        "type": "code",
        "lang": "powershell",
        "code": "hivemind-bin model init ./weights.safetensors --activation relu --final softmax --out ./model.json\nhivemind-bin model split ./model.json --weights ./weights.safetensors --input (Get-Content -Raw ./input.json) --out-dir ./split-model\nhivemind-bin pipeline check ./split-model/pipeline.json"
      },
      {
        "type": "p",
        "text": "init reads F32/F64 N.weight and optional N.bias tensors. Choose hidden and final activations explicitly: linear, relu, gelu and softmax layers are supported. --input accepts JSON text, not a filename; the PowerShell example reads input.json. Input and final output have one row per sample. split measures JSON sizes and runtime allocations, packs adjacent layers, partitions large output rows, and writes sources, weight shards and a pipeline with measured caps. Same-shaped values can differ in encoded size. Assigned Workers can see weights and intermediate values. Use matching CLI, Nodepool and Worker versions: concat and exact binary64 JSON roundtrips update the active semantics digest."
      },
      {
        "type": "h2",
        "id": "training",
        "text": "Try federated averaging"
      },
      {
        "type": "p",
        "text": "The examples/hcode/federated-training example generates synthetic data using only the Python standard library. Each round trains softmax regression on three shards in parallel, averages weights, then feeds the average into the next round. Run its pipeline.json with pipeline check first. Regression tests compare decreasing losses with a fixed independent reference."
      },
      {
        "type": "callout",
        "tone": "note",
        "text": "This is batch CPU inference and small-scale training, not interactive token generation or GPU splitting. Each stage has scheduling and consensus overhead. Splitting does not change CPT prices. Three Workers on one computer demonstrate same-host execution, not cross-host deployment."
      },
      {
        "type": "links",
        "slugs": [
          "budget",
          "limits",
          "master-api"
        ]
      }
    ]
  },
  {
    slug: 'master-api',
    group: 'tutorials',
    title: 'Submit through the Master API',
    description: 'Scripts can use the Master API. The examples assume the Master API listens on 127.0.0.1:8082. Change the address if your Master reports another one.',
    blocks: [
      { type: 'h2', id: 'sign-in', text: 'Sign in' },
      { type: 'p', text: 'Sign in once and keep the bearer token in a variable for the rest of the session. Do not write the password into a script.' },
      {
        type: 'tabs',
        tabs: [
          {
            label: 'PowerShell',
            lang: 'powershell',
            code: `$master = "http://127.0.0.1:8082"
$credential = Get-Credential -Message "Hivemind account"
$body = @{ username = $credential.UserName; password = $credential.GetNetworkCredential().Password } | ConvertTo-Json -Compress
$login = Invoke-RestMethod -Method Post -Uri "$master/api/login" -ContentType "application/json" -Body $body
$headers = @{ Authorization = "Bearer $($login.token)" }`,
          },
          {
            label: 'curl',
            lang: 'bash',
            code: `curl -s -X POST http://127.0.0.1:8082/api/login \\
  -H "Content-Type: application/json" \\
  -d '{"username": "<name>", "password": "<password>"}'
# Copy the token from the response, then:
TOKEN="<token>"`,
          },
        ],
      },
      { type: 'h2', id: 'submit', text: 'Submit a task' },
      { type: 'p', text: 'Save the program as sum.hmf and the request below as task.json. Use a new task_id for each submission. The torrent field holds the input as a JSON string, even though it is not a torrent.' },
      {
        type: 'code',
        lang: 'json',
        code: `{
  "task_id": "sum-example-001",
  "runtime": "hcode",
  "task_source": "let total = 0;\\nfor value in get(input, \\"values\\") {\\n  let total = total + value;\\n}\\ntotal;",
  "torrent": "{\\"values\\": [10, 20, 30]}",
  "max_cpt": 1000
}`,
      },
      {
        type: 'tabs',
        tabs: [
          {
            label: 'PowerShell',
            lang: 'powershell',
            code: `Invoke-RestMethod -Method Post -Uri "$master/api/tasks" -Headers $headers -ContentType "application/json; charset=utf-8" -InFile ./task.json`,
          },
          {
            label: 'curl',
            lang: 'bash',
            code: `curl -s -X POST http://127.0.0.1:8082/api/tasks \\
  -H "Authorization: Bearer $TOKEN" \\
  -H "Content-Type: application/json" \\
  -d @task.json`,
          },
        ],
      },
      { type: 'h2', id: 'read', text: 'Read the log and the result' },
      {
        type: 'tabs',
        tabs: [
          {
            label: 'PowerShell',
            lang: 'powershell',
            code: `Invoke-RestMethod -Method Get -Uri "$master/api/tasks/sum-example-001/log" -Headers $headers
Invoke-RestMethod -Method Get -Uri "$master/api/tasks/sum-example-001/result" -Headers $headers`,
          },
          {
            label: 'curl',
            lang: 'bash',
            code: `curl -s http://127.0.0.1:8082/api/tasks/sum-example-001/log -H "Authorization: Bearer $TOKEN"
curl -s http://127.0.0.1:8082/api/tasks/sum-example-001/result -H "Authorization: Bearer $TOKEN"`,
          },
        ],
      },
      { type: 'h2', id: 'stop', text: 'Stop a task' },
      { type: 'code', lang: 'bash', code: 'curl -s -X POST http://127.0.0.1:8082/api/tasks/sum-example-001/stop -H "Authorization: Bearer $TOKEN"' },
      { type: 'callout', tone: 'warning', text: 'Anyone who can read your shell history can read your password and token. Use Get-Credential or an environment variable, and do not paste tokens into shared logs.' },
      { type: 'links', slugs: ['api-reference', 'billing'] },
    ],
  },
  {
    slug: 'templates-numbers',
    group: 'templates',
    title: 'Numbers and lists',
    description: 'Short programs for totals, averages, extremes, counts, and recursion. Each example runs as published. Copy one and change the input names to fit your task.',
    blocks: [
      { type: 'h2', id: 'sum', text: 'Sum of a list' },
      { type: 'p', text: 'Adds every element of the list named values in the input.' },
      { type: 'example', id: 'sum-list' },
      { type: 'h2', id: 'mean', text: 'Mean of a list' },
      { type: 'p', text: 'Divides the total by the number of elements.' },
      { type: 'example', id: 'mean-list' },
      { type: 'callout', tone: 'note', text: 'Integer division truncates toward zero. The mean of 1, 2, and 4 is 2, not 2.33. Scale the values first if you need a finer result.' },
      { type: 'h2', id: 'largest', text: 'Largest value' },
      { type: 'p', text: 'Starts with the first element and replaces it whenever a larger element appears.' },
      { type: 'example', id: 'largest-value' },
      { type: 'h2', id: 'smallest', text: 'Smallest value' },
      { type: 'p', text: 'The same loop with the comparison reversed.' },
      { type: 'example', id: 'smallest-value' },
      { type: 'h2', id: 'count', text: 'Count matches' },
      { type: 'p', text: 'Counts the elements that reach a threshold from the input.' },
      { type: 'example', id: 'count-matches' },
      { type: 'h2', id: 'factorial', text: 'Factorial' },
      { type: 'p', text: 'A recursive function with a stopping case. The depth follows the input.' },
      { type: 'example', id: 'factorial' },
      { type: 'links', slugs: ['templates-records', 'functions'] },
    ],
  },
  {
    slug: 'templates-records',
    group: 'templates',
    title: 'Records and decisions',
    description: 'Programs that read a list of records and return a summary or a decision. The policy, score, batch, price, and routing examples are the five templates in the templates/hcode folder of the repository, with the same inputs.',
    blocks: [
      { type: 'h2', id: 'total', text: 'Total a field' },
      { type: 'p', text: 'Adds one field from every record in the input.' },
      { type: 'example', id: 'records-total' },
      { type: 'h2', id: 'batch', text: 'Summarize payments' },
      { type: 'p', text: 'Counts the paid records, adds their amounts, and reports how many records the input held. Source: 03_batch_sum.' },
      { type: 'example', id: 'batch-sum' },
      { type: 'h2', id: 'policy', text: 'Approve or reject a request' },
      { type: 'p', text: 'Approves a request only when the user is verified, the risk score is below 50, and the spend fits the cap. Source: 01_policy_gate.' },
      { type: 'example', id: 'policy-gate' },
      { type: 'h2', id: 'score', text: 'Score and band' },
      { type: 'p', text: 'Turns three metrics into a weighted score and a band. Source: 02_weighted_score.' },
      { type: 'example', id: 'weighted-score' },
      { type: 'h2', id: 'quote', text: 'Price a task' },
      { type: 'p', text: 'Estimates the price per host from the resource requirements, multiplies by the host count, and checks the result against the budget. Source: 04_price_quote.' },
      { type: 'example', id: 'price-quote' },
      { type: 'h2', id: 'route', text: 'Choose a pool' },
      { type: 'p', text: 'Picks a worker pool from the task kind and input size, and raises the priority when the labels include urgent. Source: 05_route_task.' },
      { type: 'example', id: 'route-task' },
      { type: 'links', slugs: ['templates-numbers', 'templates-text'] },
    ],
  },
  {
    slug: 'templates-text',
    group: 'templates',
    title: 'Text and checks',
    description: 'Programs that build text, check that required fields are present, and fall back to a default value.',
    blocks: [
      { type: 'h2', id: 'greeting', text: 'Greeting' },
      { type: 'p', text: 'Builds a sentence from a name in the input.' },
      { type: 'example', id: 'text-greeting' },
      { type: 'h2', id: 'required', text: 'Required fields' },
      { type: 'p', text: 'Returns true only when every key in the list is present in the input. Remove one of the keys from the input to see false.' },
      { type: 'example', id: 'validate-fields' },
      { type: 'h2', id: 'default', text: 'Default value' },
      { type: 'p', text: 'Uses 10 when the input has no limit and the given limit otherwise.' },
      { type: 'example', id: 'lookup-default' },
      { type: 'example', id: 'lookup-default-set' },
      { type: 'links', slugs: ['read-input', 'text'] },
    ],
  },
  {
    slug: 'language',
    group: 'reference',
    title: 'Language reference',
    description: 'Statements, expressions, builtins, and the rules that apply to every program.',
    blocks: [{ type: 'reference', name: 'language' }],
  },
  {
    slug: 'settings',
    group: 'reference',
    title: 'Task settings',
    description: 'The fields of a task request and what each one controls.',
    blocks: [{ type: 'reference', name: 'settings' }],
  },
  {
    slug: 'billing',
    group: 'reference',
    title: 'How charges work',
    description: 'The charge formula, the budget per replica, function pricing, and the receipts behind the examples.',
    blocks: [{ type: 'reference', name: 'billing' }],
  },
  {
    slug: 'limits',
    group: 'reference',
    title: 'Limits',
    description: 'The limits every task is checked against. Exceeding one stops the task or rejects the submission.',
    blocks: [{ type: 'reference', name: 'limits' }],
  },
  {
    slug: 'errors',
    group: 'reference',
    title: 'Error codes',
    description: 'The codes a failed replica can record, and what each one means.',
    blocks: [{ type: 'reference', name: 'failures' }],
  },
  {
    slug: 'api-reference',
    group: 'reference',
    title: 'API reference',
    description: 'The account routes and the task routes served by the Master API.',
    blocks: [{ type: 'reference', name: 'api' }],
  },
  {
    slug: 'platforms',
    group: 'reference',
    title: 'Platforms and versions',
    description: 'Supported platforms and the scope of the first public version.',
    blocks: [{ type: 'reference', name: 'platforms' }],
  },
];
