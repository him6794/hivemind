// Values below are mirrored from the running system, not written by hand:
//   TASK_ID_MAX_BYTES, MANAGED_TASK_SOURCE_MAX_BYTES, MANAGED_JSON_INPUT_MAX_BYTES
//     -> hivemind-rs/crates/proto/src/lib.rs
//   ExecutionLimits::for_managed_function_budget()
//     -> executor-rs/crates/managed-function-runtime/src/lib.rs
//   managed-function-v1-semantics.json -> v1 runtime and settlement contract
//   HTTP routes and validation rules    -> hivemind-rs/crates/master-api/src/{routes,handlers}.rs
//   product limitations                 -> docs/PUBLIC_NETWORK_LIMITATIONS.md
// Keep one copy of every number here so the two locales can never drift apart.
const LIMITS = {
  taskIdBytes: '255 bytes',
  taskSourceBytes: '64 KiB (65,536 bytes)',
  jsonInputBytes: '1 MiB (1,048,576 bytes)',
  maxCallDepth: '64',
  maxOutputBytes: '1 MiB (1,048,576 bytes)',
  maxValueBytes: '1 MiB (1,048,576 bytes)',
  maxCollectionItems: '100,000',
  maxValueDepth: '64',
  maxValueMaterializationBytes: '16 MiB (16,777,216 bytes)',
  submitPerMinute: '60',
};

const ACCOUNT_API = ['/api/register', '/api/login', '/api/balance'];

const MANAGED_EXAMPLE = `let rows = get(input, "rows");
let total = 0;

fn score(row) {
  return get(row, "hits") * 2 - get(row, "misses");
}

for row in rows {
  let total = total + score(row);
}

print("rows scanned");
total;`;

const MANAGED_EXAMPLE_INPUT = `{"rows": [{"hits": 12, "misses": 3}, {"hits": 8, "misses": 1}]}`;

const SUBMIT_EXAMPLE = `curl -X POST http://localhost:8082/api/tasks \\
  -H "Authorization: Bearer $TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{
    "task_id": "row-score-001",
    "runtime": "managed-function-v1",
    "task_source": "let n = len(get(input, \\"rows\\")); n;",
    "torrent": "{\\"rows\\": [1, 2, 3]}",
    "max_cpt": 500,
    "cpu_score": 1000,
    "memory_gb": 2,
    "host_count": 1
  }'`;

const baseRoutes = {
  en: [
    { id: 'home', label: 'Overview' },
    { id: 'login', label: 'Sign in' },
    { id: 'register', label: 'Create account' },
    { id: 'account', label: 'Account' },
    { id: 'security', label: 'Trust' },
    { id: 'docs', label: 'Docs' },
    { id: 'terms', label: 'Usage rules' },
  ],
  zh: [
    { id: 'home', label: '總覽' },
    { id: 'login', label: '登入' },
    { id: 'register', label: '建立帳號' },
    { id: 'account', label: '帳號中心' },
    { id: 'security', label: '信任與安全' },
    { id: 'docs', label: '文件' },
    { id: 'terms', label: '使用規範' },
  ],
};

const definitions = {
  en: {
    brand: {
      name: 'Hivemind',
      strap: 'Run tasks on a shared network.',
    },
    routes: baseRoutes.en,
    hero: {
      badge: 'Official site',
      title: 'Run tasks on a shared network',
      body: 'Send a task, set a per-replica execution allowance, and let Hivemind choose available computers. The network validates execution evidence before it settles the charge and refunds unused held credits.',
      primaryCta: 'Create account',
      secondaryCta: 'Read the docs',
      bullets: [
        'Charges follow valid replica work',
        'No images, containers, or packaging to manage',
        'Unused held credits are refunded after settlement',
      ],
    },
    sections: {
      stats: [
        { value: '10%', label: 'Platform fee on valid replica usage' },
        { value: '3', label: 'Default replicas for enforced local consensus' },
        { value: '0', label: 'Images or containers to manage' },
        { value: '3', label: 'Simple actions to learn' },
      ],
      features: [
        {
          title: 'The network validates work before settlement',
          body: 'Each replica records what it executed. Hivemind validates that evidence, settles valid replica usage with its platform fee, and refunds the unused part of the held credits.',
        },
        {
          title: 'No setup package',
          body: 'Send the task instructions and input. You do not need to build an image, container, or package.',
        },
        {
          title: 'Your allowance is per replica',
          body: 'max_cpt limits one replica’s execution usage. Hivemind holds that allowance for every replica plus a 10% fee, then refunds what valid execution did not use.',
        },
        {
          title: 'Your task can run on any suitable computer',
          body: 'Hivemind chooses an available computer that fits the task. It may be yours or another user\'s, so your own computer is not a guaranteed destination.',
        },
      ],
      workflow: [
        {
          step: '01',
          title: 'Create an account',
          body: 'Choose a username and password, then sign in.',
        },
        {
          step: '02',
          title: 'Choose how to run it',
          body: 'Use the task dashboard to send work, or share a computer with the network.',
        },
        {
          step: '03',
          title: 'Describe the task',
          body: 'Write the instructions and input. Advanced users can use the supported task language.',
        },
        {
          step: '04',
          title: 'Set a limit, then start',
          body: 'Review the estimate, choose the maximum credits you accept, then send it.',
        },
      ],
      security: {
        items: [
          'Each replica signs and reports what it executed, and the network validates the evidence before settlement.',
          'A final task result requires a quorum; valid replica work can still be settled even when replicas disagree.',
          'Your browser only talks to this site, never to the machines running work.',
          'Account access is separate from running work and from sharing a computer.',
        ],
        pipelineTitle: 'How Hivemind confirms a charge',
        pipeline: [
          {
            step: '01',
            title: 'Each replica stays within its allowance',
            body: 'Hivemind measures each replica’s work and stops it before the per-replica allowance is exceeded. Tasks cannot open files, connect to the network, or start other programs.',
          },
          {
            step: '02',
            title: 'Hivemind records what happened',
            body: 'The task record includes completed steps, actions, repeated steps, and output size.',
          },
          {
            step: '03',
            title: 'The network prepares a shared check',
            body: 'Several participating computers run the same supported task so the network can compare their results.',
          },
          {
            step: '04',
            title: 'The network settles valid evidence and refunds the rest',
            body: 'A quorum creates the final result. Independently, valid replica evidence determines usage settlement; unused held credits are returned after reconciliation.',
          },
        ],
        caveatsTitle: 'What this does not cover',
        caveats: [
          'Computer speed and capacity are reported by each owner and may not be exact.',
          'Windows Workers can join the network and share a computer. A task that is not supported on a computer fails rather than being charged.',
          'A task without a quorum has no final result. Settlement still uses any valid replica execution evidence that was recorded.',
          'The network checks that a task followed the agreed rules; it does not review what the task was written to do.',
        ],
      },
      account: {
        summary: 'Your account and credits, in one place. Send a task and Hivemind chooses an available computer — yours or a computer shared by another person.',
        panels: [
          {
            title: 'Credits',
            body: 'See how many credits are available for your next task.',
          },
          {
            title: 'About credits',
            body: 'CPT is an internal Hivemind credit unit for tasks. It is not money and cannot be withdrawn.',
          },
          {
            title: 'Choose what to do next',
            body: 'Run a task from the dashboard, or share a computer if you want to help the network.',
          },
        ],
      },
      docs: {
        quickstart: [
          {
            step: '01',
            title: 'Create an account here',
            body: 'Choose a username and password, then sign in to see your credits and start.',
          },
          {
            step: '02',
            title: 'Start from the task dashboard',
            body: 'The dashboard sends your task to an available computer. A computer shared by another user may run it, so your own computer is not a guaranteed destination.',
          },
          {
            step: '03',
            title: 'Describe the task',
            body: 'Add instructions and input. Technical users can use the supported task language; no package build is needed.',
          },
          {
            step: '04',
            title: 'Set a limit, send, and review',
            body: 'Check the estimate, choose a credit limit, then follow the task until its result is ready.',
          },
        ],
        groups: [
          {
            id: 'account',
            title: 'Account API',
            note: 'Served by this website and by a task service. These three are the only routes this website itself will call.',
            rows: [
              { method: 'POST', path: ACCOUNT_API[0], note: 'Create an account. Body: username, password.' },
              { method: 'POST', path: ACCOUNT_API[1], note: 'Sign in. Returns a bearer token used as Authorization: Bearer <token>.' },
              { method: 'GET', path: ACCOUNT_API[2], note: 'Read the CPT balance for the signed-in account.' },
            ],
          },
          {
            id: 'tasks',
            title: 'Task API (through your task service)',
            note: 'These routes run through your task service, not on this website. Every call needs the bearer token.',
            rows: [
              { method: 'POST', path: '/api/tasks/quote', note: 'Price a resource shape before committing to it. Returns quoted_cpt and a per-component breakdown.' },
              { method: 'POST', path: '/api/tasks', note: 'Submit a job. Rejected if max_cpt is below the quote.' },
              { method: 'GET', path: '/api/tasks', note: 'List your tasks with status, receipt fields, and results.' },
              { method: 'POST', path: '/api/tasks/{task_id}/stop', note: 'Stop a running task. Execution is cancelled with failure code cancelled.' },
              { method: 'GET', path: '/api/workers', note: 'List computers available to your task service.' },
              { method: 'GET', path: '/health', note: 'Liveness check. No authentication.' },
            ],
          },
        ],
        taskFields: [
          { name: 'task_id', type: 'string', required: 'yes', note: `ASCII letters, digits, - _ . only. No "..". Up to ${LIMITS.taskIdBytes}.` },
          { name: 'runtime', type: 'string', required: 'yes', note: 'Must be managed-function-v1. managed-function-v0 is retired and rejected for new tasks.' },
          { name: 'task_source', type: 'string', required: 'yes', note: `The managed function source text. Up to ${LIMITS.taskSourceBytes}.` },
          { name: 'torrent', type: 'string', required: 'yes', note: `The JSON input document, sent as a string. Reachable inside the function as input. Up to ${LIMITS.jsonInputBytes}.` },
          { name: 'max_cpt', type: 'integer', required: 'yes', note: 'A positive per-replica execution allowance. There is no fixed v0-era work ceiling; admission also checks the replica-aware hold against your available CPT.' },
          { name: 'cpu_score', type: 'integer', required: 'no', note: 'Minimum CPU capability a worker must have. Non-negative.' },
          { name: 'gpu_score', type: 'integer', required: 'no', note: 'Minimum GPU capability. Non-negative.' },
          { name: 'memory_gb', type: 'integer', required: 'no', note: 'Minimum memory in GB. Non-negative.' },
          { name: 'gpu_memory_gb', type: 'integer', required: 'no', note: 'Minimum GPU memory in GB. Non-negative.' },
          { name: 'storage_gb', type: 'integer', required: 'no', note: 'Minimum storage in GB. Non-negative.' },
          { name: 'host_count', type: 'integer', required: 'no', note: 'How many workers to place the job on. At least 1. Defaults to 1.' },
          { name: 'location', type: 'string', required: 'no', note: 'Preferred worker location label.' },
        ],
        language: {
          intro: 'managed-function-v1 is the supported managed job format. It is a small, metered language: each replica has a usage allowance, structural safety limits remain bounded, and there is no way to reach the host.',
          statements: [
            'let name = expression;',
            'fn name(a, b) { return expression; }',
            'for item in expression { ... }',
            'return expression;',
            'print(expression);',
            'expression;',
          ],
          expressions: [
            'integers (signed 64-bit), true, false, "strings"',
            'lists [1, 2, 3] and maps {"key": value}',
            'name, name(arg1, arg2)',
            'if condition { a } else { b }',
            '+  -  *  /',
            '==  !=  <  <=  >  >=',
          ],
          builtins: [
            { sig: 'len(value)', note: 'Length of a list, map, or string.' },
            { sig: 'get(target, key)', note: 'Read a map key or a list index.' },
            { sig: 'contains(target, value)', note: 'Membership test on a list, map, or string.' },
          ],
          rules: [
            'input holds the parsed JSON document you submitted.',
            'The last expression statement is the result, unless an earlier return exits first.',
            'There is no bare name = value assignment. Rebind with let, or write into an element with target[key] = value.',
            'Identifiers are ASCII letters, digits, and _, and cannot start with a digit.',
            'Strings are UTF-8 and support \\" \\\\ \\n \\r \\t escapes.',
            'for iterates lists only; its work consumes the per-replica usage allowance.',
            'print appends to the task record output and is bounded by the output limit.',
          ],
          forbidden: [
            'imports',
            'file I/O',
            'network I/O',
            'environment variables',
            'subprocesses',
            'dynamic eval and reflection',
            'arbitrary host functions',
            'unbounded recursion or loops',
          ],
          example: MANAGED_EXAMPLE,
          exampleInput: MANAGED_EXAMPLE_INPUT,
          exampleNote: 'Against that input each replica returns 36, prints one line, and records 80 usage units. V1 settlement combines all valid replica evidence, adds a 10% platform fee, and refunds unused held credits; this receipt is not a whole-task charge quote. Note the loop accumulator: a value is rebound with let, because a bare name = value assignment is a parse error.',
          submitExample: SUBMIT_EXAMPLE,
        },
        limits: [
          { id: 'taskSource', name: 'Job source size', value: LIMITS.taskSourceBytes, note: 'Rejected at submission if larger.' },
          { id: 'jsonInput', name: 'JSON input size', value: LIMITS.jsonInputBytes, note: 'Rejected at submission if larger.' },
          { id: 'callDepth', name: 'Call depth', value: LIMITS.maxCallDepth, note: 'Stops with call_depth_exceeded.' },
          { id: 'output', name: 'Printed output', value: LIMITS.maxOutputBytes, note: 'Stops with output_limit_exceeded.' },
          { id: 'valueBytes', name: 'One materialized value', value: LIMITS.maxValueBytes, note: 'Stops with value_limit_exceeded.' },
          { id: 'items', name: 'Items per collection', value: LIMITS.maxCollectionItems, note: 'Stops with value_limit_exceeded.' },
          { id: 'valueDepth', name: 'Value nesting depth', value: LIMITS.maxValueDepth, note: 'Stops with value_limit_exceeded.' },
          { id: 'materialization', name: 'Cumulative materialized values', value: LIMITS.maxValueMaterializationBytes, note: 'Stops with value_limit_exceeded.' },
          { id: 'taskId', name: 'Task id length', value: LIMITS.taskIdBytes, note: 'Longer ids are rejected.' },
          { id: 'submitRate', name: 'Submissions per minute', value: `${LIMITS.submitPerMinute} per account`, note: 'Default. Over the limit returns 429.' },
        ],
        billing: {
          title: 'How a managed-function-v1 task is settled',
          body: 'V1 measures actual valid replica execution, not wall-clock time. max_cpt is one replica’s usage allowance; Hivemind holds an allowance for every replica plus a 10% fee, settles valid evidence, and refunds what was not used.',
          formula: 'held_cpt = replica_count × max_cpt + 10% hold fee\ncharged_cpt = valid_replica_usage_cpt + 10% actual-usage fee\nrefund_cpt = held_cpt − charged_cpt',
          rows: [
            { name: 'Per-replica allowance', value: 'max_cpt usage units' },
            { name: 'Worst-case hold', value: 'replicas × allowance + 10%' },
            { name: 'Valid execution usage', value: '1 CPT per unit' },
            { name: 'Platform fee', value: '10% of valid usage' },
          ],
          functionRows: [
            {
              id: 'len',
              name: 'len(value)',
              price: '6 usage units + argument usage',
              note: 'The call adds 5 usage units of function overhead plus 1 for the call expression. Evaluating value is metered separately; len adds no other fixed usage.',
            },
            {
              id: 'get',
              name: 'get(target, key)',
              price: '6 usage units + argument usage',
              note: 'The call adds 5 usage units of function overhead plus 1 for the call expression. Evaluating target and key is metered separately.',
            },
            {
              id: 'contains',
              name: 'contains(target, value)',
              price: '6 usage units + argument usage',
              note: 'The call adds 5 usage units of function overhead plus 1 for the call expression. Evaluating target and value is metered separately.',
            },
            {
              id: 'user-function',
              name: 'fn name(args) { ... }',
              price: '6 usage units + arguments + body usage',
              note: 'Each user-function call adds 5 overhead units plus 1 for the call expression, then records the evaluated arguments and the metered work actually run in its body.',
            },
            {
              id: 'print',
              name: 'print(value)',
              price: '6 usage units + argument usage',
              note: 'print adds 5 usage units of output overhead plus 1 for the print statement. Evaluating value is metered separately; output is still subject to its limit.',
            },
          ],
          examples: [
            {
              id: 'len-receipt',
              title: 'len([1, 2, 3])',
              program: 'len([1, 2, 3]);',
              receiptUsageUnits: 11,
              breakdown: 'One replica records 6 fixed call units plus 5 units to evaluate the list literal, for 11 usage units.',
            },
            {
              id: 'get-receipt',
              title: 'get({"hits": 4}, "hits")',
              program: 'get({"hits": 4}, "hits");',
              receiptUsageUnits: 10,
              breakdown: 'One replica records 6 fixed call units plus 4 units for the map and key arguments, for 10 usage units.',
            },
            {
              id: 'user-function-receipt',
              title: 'A user function call',
              program: 'fn double(n) { return n * 2; } double(4);',
              receiptUsageUnits: 11,
              breakdown: 'One replica records 6 fixed function-call units plus 5 units for the argument, body expression, return, and statement evaluation.',
            },
            {
              id: 'print-receipt',
              title: 'print("ok")',
              program: 'print("ok");',
              receiptUsageUnits: 7,
              breakdown: 'One replica records 6 fixed print units plus 1 unit for the string value.',
            },
            {
              id: 'worked-receipt',
              title: 'The complete example above',
              program: MANAGED_EXAMPLE,
              receiptUsageUnits: 80,
              breakdown: 'Each valid replica of this input records 80 usage units. The final task settlement aggregates valid replicas, adds the 10% fee, and refunds unused held CPT.',
            },
          ],
          platforms: [
            {
              id: 'linux',
              name: 'Linux',
              status: 'Supported',
              check: 'Run the packaged Worker app or the supported Linux worker path. The network validates the execution evidence before settlement.',
            },
            {
              id: 'macos',
              name: 'macOS',
              status: 'Supported',
              check: 'The native Worker path can run supported tasks and report execution evidence to the network.',
            },
            {
              id: 'wsl',
              name: 'WSL',
              status: 'Supported',
              check: 'Use the Linux worker path inside WSL when a native Windows setup is not suitable for your task.',
            },
            {
              id: 'windows-native',
              name: 'Native Windows',
              status: 'Supported',
              check: 'The packaged Windows Worker connects after sign-in. Work that cannot run produces no valid execution usage for that replica.',
            },
          ],
          notes: [
            'max_cpt is a per-replica allowance, not a whole-task charge ceiling.',
            'The initial hold covers every selected replica and a 10% fee; it can therefore exceed one max_cpt.',
            'Valid replica execution is settled independently of quorum output agreement, and unused held CPT is refunded.',
          ],
        },
        settlement: {
          title: 'Quorum result and evidence settlement',
          body: 'Several participating computers report the same supported output to create a quorum result. Settlement separately aggregates signed, valid replica execution evidence: valid divergent replicas may be paid for the work they completed, while the rest of the hold is refunded.',
        },
        failures: [
          { code: 'parse_error', note: 'The source did not parse. The message carries line and column.' },
          { code: 'type_error', note: 'An operation received a value of the wrong type.' },
          { code: 'name_error', note: 'An identifier was used before it was defined.' },
          { code: 'arity_error', note: 'A function was called with the wrong number of arguments.' },
          { code: 'key_error', note: 'get was called with a key the map does not have.' },
          { code: 'index_error', note: 'A list index was out of range.' },
          { code: 'input_error', note: 'The JSON input could not be read as expected.' },
          { code: 'budget_exhausted', note: 'A replica spent its max_cpt allowance before finishing.' },
          { code: 'integer_arithmetic_overflow', note: 'A signed 64-bit arithmetic result overflowed; V1 uses checked arithmetic.' },
          { code: 'call_depth_exceeded', note: 'Calls nested deeper than the depth ceiling.' },
          { code: 'output_limit_exceeded', note: 'print produced more than the output ceiling.' },
          { code: 'value_limit_exceeded', note: 'A value grew past the size, item, or nesting ceiling.' },
          { code: 'cancelled', note: 'The task was stopped through the stop route.' },
          { code: 'runtime_error', note: 'An evaluation error that does not fall into the categories above.' },
        ],
      },
      terms: {
        summary: 'Hivemind is still being tested. This page explains how credits and shared computers work today, what is stored, and what is not promised.',
        groups: [
          {
            title: 'What credits are',
            items: [
              'CPT is an internal credit and budget unit for tasks.',
              'CPT is not money or cryptocurrency. There is no conversion or withdrawal path.',
              'Credits stay in your Hivemind account for tasks.',
            ],
          },
          {
            title: 'Current service model',
            items: [
              'Hivemind currently uses a limited group of participating computers.',
              'Open participation, bidding, and public rewards are not available yet.',
              'People who share computers are expected to follow the published rules.',
              'The account that sends a task and the account that shares the computer can be different. A task may run on another user\'s computer and is not guaranteed to run on your own.',
              'Computer capacity is reported by each owner and may not be exact.',
            ],
          },
          {
            title: 'What you may run',
            items: [
              'Only the supported managed task format is available today.',
              'Tasks must fit the published size, operation, and credit limits.',
              'Tasks cannot reach the filesystem, the network, other processes, or the computer running them.',
            ],
          },
          {
            title: 'What is not allowed',
            items: [
              'Trying to escape the task safety limits or reach the computer running a task.',
              'Falsifying computer capacity or activity on a computer you share.',
              'Interfering with task assignment, other accounts, or other computers.',
              'Using another account\'s credentials or sharing a token you were issued.',
            ],
          },
          {
            title: 'What is not promised',
            items: [
              'No uptime or availability guarantee, and no service level agreement.',
              'No dispute resolution process for charges or task outcomes.',
              'No durability guarantee for task inputs, outputs, or results.',
              'Availability is best effort. A task can be rejected, tried again, or fail.',
            ],
          },
          {
            title: 'What is stored',
            items: [
              'Account records: username and a hashed password. Passwords are never stored in readable form.',
              'Task records: the instructions, input, activity record, and result.',
              'Charge records: the network check and amount charged.',
              'People who run Hivemind may see task instructions and input. Do not submit secrets.',
            ],
          },
          {
            title: 'If you share a computer',
            items: [
              'Set up and maintain the computer you share.',
              'You are responsible for the network access and task safety settings you configure.',
              'If the network cannot confirm a task, it fails rather than charging.',
              'This website does not control the computer you share.',
            ],
          },
        ],
      },
    },
  },
  zh: {
    brand: {
      name: 'Hivemind',
      strap: '在共享網路上執行工作。',
    },
    routes: baseRoutes.zh,
    hero: {
      badge: '官方網站',
      title: '在共享網路上執行工作',
      body: '送出工作、設定每個副本的執行額度，讓 Hivemind 選擇可用的電腦。網路會驗證執行證據後結算，並退回未使用的保留額度。',
      primaryCta: '建立帳號',
      secondaryCta: '閱讀文件',
      bullets: [
        '依有效副本的實際工作量結算',
        '不用管理映像檔、容器或打包檔',
        '結算後退回未使用的保留額度',
      ],
    },
    sections: {
      stats: [
        { value: '10%', label: '有效副本用量的平台代理費' },
        { value: '3', label: '本機強制共識的預設副本數' },
        { value: '0', label: '需要管理的映像檔或容器' },
        { value: '3', label: '學會三個簡單動作就能開始' },
      ],
      features: [
        {
          title: '先驗證執行證據再結算',
          body: '每個副本都會記錄實際執行內容。Hivemind 會驗證證據、結算有效副本的用量與 10% 費用，並退回未使用的保留額度。',
        },
        {
          title: '不用準備打包檔',
          body: '送出工作說明和輸入資料即可，不需要自己建立映像檔、容器或套件。',
        },
        {
          title: '額度是每個副本的上限',
          body: 'max_cpt 限制單一副本的執行用量。Hivemind 會為每個副本保留這份額度及 10% 費用，再退回有效執行未使用的部分。',
        },
        {
          title: '工作會跑在合適的電腦上',
          body: 'Hivemind 會選符合條件的可用電腦，可能是你的，也可能是其他人分享的電腦，不保證一定使用你的電腦。',
        },
      ],
      workflow: [
        {
          step: '01',
          title: '建立帳號',
          body: '選擇使用者名稱和密碼，登入後就能開始。',
        },
        {
          step: '02',
          title: '選擇要做什麼',
          body: '從任務頁面送出工作，或分享一台電腦來幫助網路。',
        },
        {
          step: '03',
          title: '說明工作內容',
          body: '填入工作說明和輸入資料；熟悉技術的人也可以使用支援的工作語言。',
        },
        {
          step: '04',
          title: '設定上限並開始',
          body: '查看預估額度，設定你願意支付的最高額度，再送出工作。',
        },
      ],
      security: {
        items: [
          '每個副本都會簽署並回報實際執行內容，網路會在結算前驗證證據。',
          '最終結果需要達成共識；即使副本結果不同，有效副本已完成的工作仍可能結算。',
          '瀏覽器只連這個網站，不會直接連到執行工作的電腦。',
          '登入帳號、執行工作和分享電腦是分開的事情。',
        ],
        pipelineTitle: 'Hivemind 如何確認扣款',
        pipeline: [
          {
            step: '01',
            title: '每個副本遵守自己的額度',
            body: 'Hivemind 會計算每個副本的工作量，在超過該副本額度前停止。工作不能開啟檔案、連接網路或啟動其他程式。',
          },
          {
            step: '02',
            title: '記錄完成了什麼',
            body: '工作記錄包含完成的步驟、動作、重複次數和輸出大小。',
          },
          {
            step: '03',
            title: '多台電腦一起查核',
            body: '幾台參與工作的電腦會執行相同的支援工作，讓網路比較結果。',
          },
          {
            step: '04',
            title: '結算有效證據並退回剩餘額度',
            body: '共識會產生最終結果；有效副本的執行證據則獨立決定用量結算，未使用的保留額度會在對帳後退回。',
          },
        ],
        caveatsTitle: '這套機制不涵蓋什麼',
        caveats: [
          '電腦速度和容量由各自的擁有者回報，可能不完全準確。',
          'Windows Worker 可以加入網路並分享電腦。不支援的工作會失敗，不會扣款。',
          '無法達成共識的工作沒有最終結果；已記錄的有效副本執行證據仍可作為結算依據。',
          '網路會查核工作是否依照規則執行，不會審查工作內容本身想做什麼。',
        ],
      },
      account: {
        summary: '你的帳號與額度都在這裡。送出工作後，Hivemind 會選一台可用的電腦，可能是你的，也可能是其他人分享的電腦。',
        panels: [
          {
            title: '額度',
            body: '查看下一份工作可以使用多少額度。',
          },
          {
            title: '額度是什麼',
            body: 'CPT 是 Hivemind 內部使用的工作額度，不是貨幣，也不能提領。',
          },
          {
            title: '選擇下一步',
            body: '從任務頁面開始工作，或分享一台電腦來幫助網路。',
          },
        ],
      },
      docs: {
        quickstart: [
          {
            step: '01',
            title: '在這裡建立帳號',
            body: '選擇使用者名稱和密碼，登入後就能查看額度並開始。',
          },
          {
            step: '02',
            title: '從任務頁面開始',
            body: '任務會交給一台可用的電腦。也可能由其他使用者分享的電腦執行，不保證一定跑在你自己的電腦。',
          },
          {
            step: '03',
            title: '說明工作內容',
            body: '填入工作說明和輸入資料；熟悉技術的人可以使用支援的工作語言，不需要建立打包檔。',
          },
          {
            step: '04',
            title: '設定上限、送出並查看',
            body: '確認預估額度，設定上限後送出，接著查看工作進度和結果。',
          },
        ],
        groups: [
          {
            id: 'account',
            title: '帳號 API',
            note: '本網站與任務服務都提供。這三個也是本網站唯一會呼叫的路由。',
            rows: [
              { method: 'POST', path: ACCOUNT_API[0], note: '建立帳號。Body：username、password。' },
              { method: 'POST', path: ACCOUNT_API[1], note: '登入，回傳 bearer token，之後以 Authorization: Bearer <token> 帶入。' },
              { method: 'GET', path: ACCOUNT_API[2], note: '讀取已登入帳號的 CPT 餘額。' },
            ],
          },
          {
            id: 'tasks',
            title: '任務 API（透過你的任務服務）',
            note: '這些路由透過你的任務服務運作，不在本網站。每次呼叫都需要 bearer token。',
            rows: [
              { method: 'POST', path: '/api/tasks/quote', note: '在正式送出前先估價。回傳 quoted_cpt 與各項目明細。' },
              { method: 'POST', path: '/api/tasks', note: '送出工作。max_cpt 低於報價會被拒絕。' },
              { method: 'GET', path: '/api/tasks', note: '列出你的任務，含狀態、工作記錄欄位與結果。' },
              { method: 'POST', path: '/api/tasks/{task_id}/stop', note: '停止執行中的任務，執行會以 cancelled 失敗代碼中止。' },
              { method: 'GET', path: '/api/workers', note: '列出你的任務服務可以使用的電腦。' },
              { method: 'GET', path: '/health', note: '存活檢查，不需驗證。' },
            ],
          },
        ],
        taskFields: [
          { name: 'task_id', type: 'string', required: '必填', note: `僅限 ASCII 字母、數字與 - _ .，不可含 ".."，長度上限 ${LIMITS.taskIdBytes}。` },
          { name: 'runtime', type: 'string', required: '必填', note: '必須是 managed-function-v1。managed-function-v0 已退役，新工作會被拒絕。' },
          { name: 'task_source', type: 'string', required: '必填', note: `managed function 原始碼，上限 ${LIMITS.taskSourceBytes}。` },
          { name: 'torrent', type: 'string', required: '必填', note: `JSON 輸入文件，以字串傳入，在函式中以 input 取用，上限 ${LIMITS.jsonInputBytes}。` },
          { name: 'max_cpt', type: 'integer', required: '必填', note: '正整數的單一副本執行額度。沒有舊 V0 的固定工作上限；系統還會確認你的 CPT 足以支付副本數量加費用的保留額度。' },
          { name: 'cpu_score', type: 'integer', required: '選填', note: 'worker 需具備的最低 CPU 能力，不可為負。' },
          { name: 'gpu_score', type: 'integer', required: '選填', note: '最低 GPU 能力，不可為負。' },
          { name: 'memory_gb', type: 'integer', required: '選填', note: '最低記憶體（GB），不可為負。' },
          { name: 'gpu_memory_gb', type: 'integer', required: '選填', note: '最低 GPU 記憶體（GB），不可為負。' },
          { name: 'storage_gb', type: 'integer', required: '選填', note: '最低儲存空間（GB），不可為負。' },
          { name: 'host_count', type: 'integer', required: '選填', note: '要放到幾個 worker 上，至少 1，預設 1。' },
          { name: 'location', type: 'string', required: '選填', note: '偏好的 worker 位置標籤。' },
        ],
        language: {
          intro: 'managed-function-v1 是目前支援的 managed 工作格式。它是一個小型、會計量的語言：每個副本都有用量額度，結構性安全限制仍受約束，且沒有任何管道可以碰到宿主機。',
          statements: [
            'let name = expression;',
            'fn name(a, b) { return expression; }',
            'for item in expression { ... }',
            'return expression;',
            'print(expression);',
            'expression;',
          ],
          expressions: [
            '整數（有號 64 位元）、true、false、"字串"',
            'list [1, 2, 3] 與 map {"key": value}',
            'name、name(arg1, arg2)',
            'if condition { a } else { b }',
            '+  -  *  /',
            '==  !=  <  <=  >  >=',
          ],
          builtins: [
            { sig: 'len(value)', note: '取得 list、map 或字串的長度。' },
            { sig: 'get(target, key)', note: '讀取 map 的鍵或 list 的索引。' },
            { sig: 'contains(target, value)', note: '判斷 list、map 或字串是否包含某值。' },
          ],
          rules: [
            'input 就是你送出的那份 JSON，已解析好。',
            '最後一個運算式陳述句就是回傳值，除非更早的 return 先結束。',
            '沒有裸寫的 name = value 賦值。請用 let 重新綁定，或以 target[key] = value 寫入元素。',
            '識別字由 ASCII 字母、數字與 _ 組成，且不可以數字開頭。',
            '字串為 UTF-8，支援 \\" \\\\ \\n \\r \\t 跳脫。',
            'for 只能迭代 list；其中的工作會消耗每個副本的用量額度。',
            'print 會寫入工作記錄的輸出，受輸出上限約束。',
          ],
          forbidden: [
            'import',
            '檔案 I/O',
            '網路 I/O',
            '環境變數',
            '子行程',
            '動態 eval 與反射',
            '任意宿主函式',
            '無界遞迴或迴圈',
          ],
          example: MANAGED_EXAMPLE,
          exampleInput: MANAGED_EXAMPLE_INPUT,
          exampleNote: '搭配這份輸入執行時，每個副本都會回傳 36、印出一行，並記錄 80 個 usage unit。V1 會合計所有有效副本的證據、加上 10% 平台費，並退回未使用的保留額度；這份收據不是整份任務的報價。注意迴圈裡的累加寫法：要用 let 重新綁定，因為裸寫 name = value 會是 parse_error。',
          submitExample: SUBMIT_EXAMPLE,
        },
        limits: [
          { id: 'taskSource', name: '原始碼大小', value: LIMITS.taskSourceBytes, note: '超過即在送出時拒絕。' },
          { id: 'jsonInput', name: 'JSON 輸入大小', value: LIMITS.jsonInputBytes, note: '超過即在送出時拒絕。' },
          { id: 'callDepth', name: '呼叫深度', value: LIMITS.maxCallDepth, note: '超過以 call_depth_exceeded 中止。' },
          { id: 'output', name: '輸出位元組', value: LIMITS.maxOutputBytes, note: '超過以 output_limit_exceeded 中止。' },
          { id: 'valueBytes', name: '單一具體化值大小', value: LIMITS.maxValueBytes, note: '超過以 value_limit_exceeded 中止。' },
          { id: 'items', name: '單一集合元素數', value: LIMITS.maxCollectionItems, note: '超過以 value_limit_exceeded 中止。' },
          { id: 'valueDepth', name: '值巢狀深度', value: LIMITS.maxValueDepth, note: '超過以 value_limit_exceeded 中止。' },
          { id: 'materialization', name: '累積具體化值大小', value: LIMITS.maxValueMaterializationBytes, note: '超過以 value_limit_exceeded 中止。' },
          { id: 'taskId', name: 'task_id 長度', value: LIMITS.taskIdBytes, note: '過長會被拒絕。' },
          { id: 'submitRate', name: '每分鐘送出次數', value: `每帳號 ${LIMITS.submitPerMinute} 次`, note: '預設值，超過回傳 429。' },
        ],
        billing: {
          title: 'managed-function-v1 怎麼結算',
          body: 'V1 依有效副本的實際執行量結算，而不是看牆鐘時間。max_cpt 是單一副本的用量額度；Hivemind 會為每個副本保留額度及 10% 費用，結算有效證據後退回未使用的部分。',
          formula: 'held_cpt = 副本數 × max_cpt + 10% 保留費\ncharged_cpt = 有效副本用量 + 10% 實際用量費\nrefund_cpt = held_cpt − charged_cpt',
          rows: [
            { name: '每個副本的額度', value: 'max_cpt 個 usage unit' },
            { name: '最壞情況保留額度', value: '副本數 × 額度 + 10%' },
            { name: '有效執行用量', value: '每單位 1 CPT' },
            { name: '平台費', value: '有效用量的 10%' },
          ],
          functionRows: [
            {
              id: 'len',
              name: 'len(value)',
              price: '6 個 usage unit + 引數用量',
              note: '呼叫固定加 5 個 usage unit，呼叫運算式再加 1 個；value 的計算另按實際運算計量，len 沒有其他固定用量。',
            },
            {
              id: 'get',
              name: 'get(target, key)',
              price: '6 個 usage unit + 引數用量',
              note: '呼叫固定加 5 個 usage unit，呼叫運算式再加 1 個；target 與 key 的計算另按實際運算計量。',
            },
            {
              id: 'contains',
              name: 'contains(target, value)',
              price: '6 個 usage unit + 引數用量',
              note: '呼叫固定加 5 個 usage unit，呼叫運算式再加 1 個；target 與 value 的計算另按實際運算計量。',
            },
            {
              id: 'user-function',
              name: 'fn name(args) { ... }',
              price: '6 個 usage unit + 引數 + 本體用量',
              note: '每次使用者函式呼叫固定加 5 個 usage unit，呼叫運算式再加 1 個，接著記錄引數與函式本體實際執行的計量工作。',
            },
            {
              id: 'print',
              name: 'print(value)',
              price: '6 個 usage unit + 引數用量',
              note: 'print 固定加 5 個 usage unit，print 陳述式本身再加 1 個；value 的計算也會計量，輸出仍受大小上限限制。',
            },
          ],
          examples: [
            {
              id: 'len-receipt',
              title: 'len([1, 2, 3])',
              program: 'len([1, 2, 3]);',
              receiptUsageUnits: 11,
              breakdown: '單一副本會記錄 6 個固定呼叫用量，加上建立 list 的 5 個用量，共 11 個 usage unit。',
            },
            {
              id: 'get-receipt',
              title: 'get({"hits": 4}, "hits")',
              program: 'get({"hits": 4}, "hits");',
              receiptUsageUnits: 10,
              breakdown: '單一副本會記錄 6 個固定呼叫用量，加上 map 與 key 引數的 4 個用量，共 10 個 usage unit。',
            },
            {
              id: 'user-function-receipt',
              title: '使用者函式呼叫',
              program: 'fn double(n) { return n * 2; } double(4);',
              receiptUsageUnits: 11,
              breakdown: '單一副本會記錄 6 個固定函式呼叫用量，加上引數、本體運算、return 與陳述式評估的 5 個用量。',
            },
            {
              id: 'print-receipt',
              title: 'print("ok")',
              program: 'print("ok");',
              receiptUsageUnits: 7,
              breakdown: '單一副本會記錄 6 個固定 print 用量，加上字串值的 1 個用量。',
            },
            {
              id: 'worked-receipt',
              title: '上面的完整範例',
              program: MANAGED_EXAMPLE,
              receiptUsageUnits: 80,
              breakdown: '這份輸入的每個有效副本會記錄 80 個 usage unit。最終結算會合計有效副本、加上 10% 費用，並退回未使用的 CPT。',
            },
          ],
          platforms: [
            {
              id: 'linux',
              name: 'Linux',
              status: '支援',
              check: '可以使用支援的 Linux Worker 路徑。網路會在結算前驗證執行證據。',
            },
            {
              id: 'macos',
              name: 'macOS',
              status: '支援',
              check: '原生 Worker 路徑可以執行支援的工作，並回報執行證據給網路。',
            },
            {
              id: 'wsl',
              name: 'WSL',
              status: '支援',
              check: '如果原生 Windows 設定不適合你的工作，可以在 WSL 內使用 Linux Worker 路徑。',
            },
            {
              id: 'windows-native',
              name: '原生 Windows',
              status: '支援',
              check: '打開 Windows Worker 並登入後即可連線。無法執行的工作不會為該副本產生有效執行用量。',
            },
          ],
          notes: [
            'max_cpt 是每個副本的額度，不是整份任務的最高收費上限。',
            '初始保留額度涵蓋每個副本與 10% 費用，因此可能大於單一 max_cpt。',
            '有效副本執行會獨立於共識輸出結果結算，未使用的保留 CPT 會退回。',
          ],
        },
        settlement: {
          title: '共識結果與執行證據結算',
          body: '多台參與電腦回報相同的支援輸出後，才會產生共識結果。結算則獨立合計已簽署且有效的副本執行證據：結果不同但執行有效的副本仍可能按完成的工作獲得結算，剩餘保留額度會退回。',
        },
        failures: [
          { code: 'parse_error', note: '原始碼無法解析，訊息會帶行號與欄位。' },
          { code: 'type_error', note: '運算收到型別不符的值。' },
          { code: 'name_error', note: '識別字在定義前就被使用。' },
          { code: 'arity_error', note: '函式呼叫的引數數量不符。' },
          { code: 'key_error', note: 'get 取用了 map 沒有的鍵。' },
          { code: 'index_error', note: 'list 索引超出範圍。' },
          { code: 'input_error', note: 'JSON 輸入無法依預期讀取。' },
          { code: 'budget_exhausted', note: '單一副本在完成前用光了自己的 max_cpt 額度。' },
          { code: 'integer_arithmetic_overflow', note: '有號 64 位元運算結果溢位；V1 使用 checked arithmetic。' },
          { code: 'call_depth_exceeded', note: '呼叫巢狀超過深度上限。' },
          { code: 'output_limit_exceeded', note: 'print 產生的輸出超過上限。' },
          { code: 'value_limit_exceeded', note: '值超過大小、元素數或巢狀深度上限。' },
          { code: 'cancelled', note: '任務透過 stop 路由被停止。' },
          { code: 'runtime_error', note: '不屬於以上分類的求值錯誤。' },
        ],
      },
      terms: {
        summary: 'Hivemind 目前仍在測試。這一頁說明今天的額度、共享電腦、資料保存方式，以及沒有提供的保證。',
        groups: [
          {
            title: '額度是什麼',
            items: [
              'CPT 是執行工作時使用的內部額度和預算單位。',
              'CPT 不是貨幣或加密貨幣，不能兌換或提領。',
              '額度會留在你的 Hivemind 帳號中，用於執行工作。',
            ],
          },
          {
            title: '目前的服務方式',
            items: [
              'Hivemind 目前使用一組有限的參與電腦。',
              '公開加入、競價和公開獎勵目前尚未提供。',
              '分享電腦的人需要遵守已公布的規則。',
              '送出工作的帳號和分享電腦的帳號可以不同。工作可能跑在其他使用者的電腦，不保證跑在你自己的電腦。',
              '電腦能力由擁有者回報，可能不完全準確。',
            ],
          },
          {
            title: '你可以執行什麼',
            items: [
              '目前只提供支援的 managed 工作格式。',
              '工作必須符合公開的大小、運算次數和額度上限。',
              '工作無法接觸檔案系統、網路、其他行程或執行它的電腦。',
            ],
          },
          {
            title: '不被允許的行為',
            items: [
              '嘗試離開工作安全限制，或從工作接觸執行它的電腦。',
              '在你分享的電腦上偽造能力或活動數據。',
              '干擾任務分派、其他帳號或其他電腦。',
              '使用他人帳號憑證，或分享發給你的 token。',
            ],
          },
          {
            title: '不提供的保證',
            items: [
              '沒有可用性或正常運行時間保證，也沒有服務等級協議。',
              '沒有針對扣款或工作結果的爭議處理程序。',
              '對工作的輸入、輸出與結果不提供持久性保證。',
              '電腦供給為盡力而為，工作可能被拒絕、重試或失敗。',
            ],
          },
          {
            title: '會被保存的資料',
            items: [
              '帳號紀錄：使用者名稱與雜湊後的密碼。密碼不會以可讀形式保存。',
              '工作紀錄：工作說明、輸入資料、活動記錄與結果。',
              '扣款紀錄：網路查核結果與實際扣款額度。',
              '執行 Hivemind 的人可能看得到工作說明和輸入資料，請不要送出機密資訊。',
            ],
          },
          {
            title: '如果你分享一台電腦',
            items: [
              '自行設定並維護你分享的電腦。',
              '你需要負責自己設定的網路存取和工作安全設定。',
              '網路無法確認的工作會失敗，不會扣款。',
              '本網站不會控制你分享的電腦。',
            ],
          },
        ],
      },
    },
  },
};

export function normalizeLocale(value) {
  return String(value || '').toLowerCase().startsWith('zh') ? 'zh' : 'en';
}

export function getSiteDefinition(locale) {
  return definitions[normalizeLocale(locale)];
}
