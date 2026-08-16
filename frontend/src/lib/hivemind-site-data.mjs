// Values below are mirrored from the running system, not written by hand:
//   TASK_ID_MAX_BYTES, MANAGED_TASK_SOURCE_MAX_BYTES, MANAGED_JSON_INPUT_MAX_BYTES,
//   MANAGED_BUDGET_MAX_USAGE_UNITS      -> hivemind-rs/crates/proto/src/lib.rs
//   ExecutionLimits::default()          -> executor-rs/crates/managed-function-runtime/src/lib.rs
//   HTTP routes and validation rules    -> hivemind-rs/crates/master-api/src/{routes,handlers}.rs
//   billing model and syntax            -> docs/MANAGED_FUNCTION_RUNTIME.md
//   product limitations                 -> docs/PUBLIC_NETWORK_LIMITATIONS.md
// Keep one copy of every number here so the two locales can never drift apart.
const LIMITS = {
  taskIdBytes: '255 bytes',
  taskSourceBytes: '64 KiB (65,536 bytes)',
  jsonInputBytes: '1 MiB (1,048,576 bytes)',
  budgetUnits: '1,000,000',
  maxOps: '1,000,000',
  maxCallDepth: '64',
  maxOutputBytes: '1 MiB (1,048,576 bytes)',
  maxLoopIterations: '100,000',
  maxCollectionItems: '100,000',
  maxValueDepth: '64',
  submitPerMinute: '60',
  proverTimeoutSecs: '900',
  proveSeconds: '570-580',
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
    "runtime": "managed-function-v0",
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
      strap: 'Verifiable compute on machines you do not own.',
    },
    routes: baseRoutes.en,
    hero: {
      badge: 'Official site',
      title: 'Verifiable compute on borrowed machines',
      body: 'Submit a function with a JSON input and a spending cap. It runs on spare capacity somewhere on the network, and the bill comes from a proof the network verifies itself, never from what the machine claims it used.',
      primaryCta: 'Create account',
      secondaryCta: 'Read the docs',
      bullets: [
        'Priced per operation executed',
        'No images, no containers, no packaging step',
        'Nothing settles without a passing proof',
      ],
    },
    sections: {
      stats: [
        { value: '1 CPT', label: 'Per usage unit, plus one to invoke' },
        { value: '100%', label: 'Of jobs verified by proof before they settle' },
        { value: '0', label: 'Images or containers to build' },
        { value: '3', label: 'Builtins to learn: len, get, contains' },
      ],
      features: [
        {
          title: 'The bill comes from a check, not a claim',
          body: 'Usage numbers arriving from a worker are a claim. The network verifies the proof on its own side, and a job that fails that check is never settled.',
        },
        {
          title: 'No image to build',
          body: 'You submit source text and a JSON document. There is one job format, managed-function-v0, and no packaging step in front of it.',
        },
        {
          title: 'The budget is a hard ceiling',
          body: 'Execution stops the moment max_cpt is spent and fails with budget_exhausted. You cannot be charged past the number you set.',
        },
        {
          title: 'Your Master is yours; execution may happen elsewhere',
          body: 'A worker may be deployed by another Hivemind user. The scheduler chooses an enabled worker that meets the job requirements, so your own worker is not a guaranteed destination.',
        },
      ],
      workflow: [
        {
          step: '01',
          title: 'Create an account',
          body: 'Three characters for a username, eight for a password.',
        },
        {
          step: '02',
          title: 'Deploy a node',
          body: 'Same binary either way. The argument is master or worker.',
        },
        {
          step: '03',
          title: 'Write the function',
          body: 'let, fn, for, if, and three builtins: len, get, contains.',
        },
        {
          step: '04',
          title: 'Quote, then submit',
          body: 'Ask for a quote first, then submit with a max_cpt you accept.',
        },
      ],
      security: {
        items: [
          'Usage numbers from a worker are claims, and every one is checked before it is billed.',
          'A job that fails verification is not settled. It fails instead.',
          'Your browser only talks to this site, never to the machines running work.',
          'Account access is separate from running work and from managing machines.',
        ],
        pipelineTitle: 'What happens between running a job and being charged for it',
        pipeline: [
          {
            step: '01',
            title: 'The job runs under a hard ceiling',
            body: 'The managed runtime meters every operation and stops at your budget. The language itself has no way to open a file, a socket, or a process.',
          },
          {
            step: '02',
            title: 'The worker produces a receipt',
            body: 'Operations executed, usage units, function calls, loop iterations, and output size are recorded as a structured receipt.',
          },
          {
            step: '03',
            title: 'A proof is generated over that execution',
            body: 'A separate prover process on the worker produces a cryptographic proof of the run. It is not the same process that executed the job.',
          },
          {
            step: '04',
            title: 'The network verifies it, then settles',
            body: 'Verification happens on the trusted side, including a check that the proof came from the exact guest program the network pinned. Settlement only follows a passing check.',
          },
        ],
        caveatsTitle: 'What this does not cover',
        caveats: [
          'Worker capability numbers such as CPU and GPU scores are self-declared and are not yet independently calibrated.',
          'Proving runs on Linux, macOS, and WSL. A native Windows worker has no prover, so its managed jobs fail rather than settle.',
          'The fail-closed policy is the default. An operator can relax it, and the relaxed modes settle from numbers the worker supplied, which is not trust-preserving.',
          'Verification proves the job ran as written. It does not review what the job was written to do.',
        ],
      },
      account: {
        summary: 'Your identity and balance live here. Work is submitted through your Master, then scheduled onto an eligible network worker.',
        panels: [
          {
            title: 'Balance',
            body: 'See your current CPT balance without opening a console.',
          },
          {
            title: 'What CPT is',
            body: 'An internal usage unit for running jobs. It is not money and does not convert to any currency.',
          },
          {
            title: 'Next step',
            body: 'Deploy a Master node to send work, or a Worker node to contribute compute.',
          },
        ],
      },
      docs: {
        quickstart: [
          {
            step: '01',
            title: 'Create an account here',
            body: 'Usernames are at least 3 characters and passwords at least 8. Sign-in returns a bearer token used by every other call.',
          },
          {
            step: '02',
            title: 'Deploy a node',
            body: 'A Master node gives you the task API and Master UI. Workers can be deployed by you or by another user; the scheduler sends work to a suitable available worker.',
          },
          {
            step: '03',
            title: 'Write a managed function',
            body: 'Jobs are plain source text in the managed-function-v0 language, plus a JSON input document. There is no packaging step.',
          },
          {
            step: '04',
            title: 'Quote, submit, collect',
            body: 'Ask for a quote, submit with a budget you accept, then poll the task list until the receipt and result are back.',
          },
        ],
        groups: [
          {
            id: 'account',
            title: 'Account API',
            note: 'Served by this website and by any Master node. These three are the only routes this website itself will call.',
            rows: [
              { method: 'POST', path: ACCOUNT_API[0], note: 'Create an account. Body: username, password.' },
              { method: 'POST', path: ACCOUNT_API[1], note: 'Sign in. Returns a bearer token used as Authorization: Bearer <token>.' },
              { method: 'GET', path: ACCOUNT_API[2], note: 'Read the CPT balance for the signed-in account.' },
            ],
          },
          {
            id: 'tasks',
            title: 'Task API (on the Master node you deploy)',
            note: 'These live on your own Master node, not on this website. Every call needs the bearer token.',
            rows: [
              { method: 'POST', path: '/api/tasks/quote', note: 'Price a resource shape before committing to it. Returns quoted_cpt and a per-component breakdown.' },
              { method: 'POST', path: '/api/tasks', note: 'Submit a job. Rejected if max_cpt is below the quote.' },
              { method: 'GET', path: '/api/tasks', note: 'List your tasks with status, receipt fields, and results.' },
              { method: 'POST', path: '/api/tasks/{task_id}/stop', note: 'Stop a running task. Execution is cancelled with failure code cancelled.' },
              { method: 'GET', path: '/api/workers', note: 'List worker nodes visible to your Master node.' },
              { method: 'GET', path: '/health', note: 'Liveness check. No authentication.' },
            ],
          },
        ],
        taskFields: [
          { name: 'task_id', type: 'string', required: 'yes', note: `ASCII letters, digits, - _ . only. No "..". Up to ${LIMITS.taskIdBytes}.` },
          { name: 'runtime', type: 'string', required: 'yes', note: 'Must be managed-function-v0. Any other value is rejected as an unsupported task runtime.' },
          { name: 'task_source', type: 'string', required: 'yes', note: `The managed function source text. Up to ${LIMITS.taskSourceBytes}.` },
          { name: 'torrent', type: 'string', required: 'yes', note: `The JSON input document, sent as a string. Reachable inside the function as input. Up to ${LIMITS.jsonInputBytes}.` },
          { name: 'max_cpt', type: 'integer', required: 'yes', note: `Your budget and hard ceiling. Must be above 0 and at most ${LIMITS.budgetUnits}. Execution stops with budget_exhausted when spent.` },
          { name: 'cpu_score', type: 'integer', required: 'no', note: 'Minimum CPU capability a worker must have. Non-negative.' },
          { name: 'gpu_score', type: 'integer', required: 'no', note: 'Minimum GPU capability. Non-negative.' },
          { name: 'memory_gb', type: 'integer', required: 'no', note: 'Minimum memory in GB. Non-negative.' },
          { name: 'gpu_memory_gb', type: 'integer', required: 'no', note: 'Minimum GPU memory in GB. Non-negative.' },
          { name: 'storage_gb', type: 'integer', required: 'no', note: 'Minimum storage in GB. Non-negative.' },
          { name: 'host_count', type: 'integer', required: 'no', note: 'How many workers to place the job on. At least 1. Defaults to 1.' },
          { name: 'location', type: 'string', required: 'no', note: 'Preferred worker location label.' },
        ],
        language: {
          intro: 'managed-function-v0 is the only supported job format. It is a small, bounded language: every statement and expression is metered, and there is no way to reach the host.',
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
            'for iterates lists only, and is bounded by the loop limit.',
            'print appends to the receipt output and is bounded by the output limit.',
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
          exampleNote: 'Against that input the function returns 36, prints one line, and records 80 usage units. The job therefore settles at 81 CPT: one for the invocation, plus one per unit. Note the loop accumulator: a value is rebound with let, because a bare name = value assignment is a parse error.',
          submitExample: SUBMIT_EXAMPLE,
        },
        limits: [
          { id: 'taskSource', name: 'Job source size', value: LIMITS.taskSourceBytes, note: 'Rejected at submission if larger.' },
          { id: 'jsonInput', name: 'JSON input size', value: LIMITS.jsonInputBytes, note: 'Rejected at submission if larger.' },
          { id: 'budget', name: 'Budget ceiling (max_cpt)', value: LIMITS.budgetUnits, note: 'Usage units. Must be above 0.' },
          { id: 'ops', name: 'Operations per job', value: LIMITS.maxOps, note: 'Stops with op_limit_exceeded.' },
          { id: 'loops', name: 'Loop iterations', value: LIMITS.maxLoopIterations, note: 'Stops with loop_limit_exceeded.' },
          { id: 'callDepth', name: 'Call depth', value: LIMITS.maxCallDepth, note: 'Stops with call_depth_exceeded.' },
          { id: 'output', name: 'Printed output', value: LIMITS.maxOutputBytes, note: 'Stops with output_limit_exceeded.' },
          { id: 'items', name: 'Items per collection', value: LIMITS.maxCollectionItems, note: 'Stops with value_limit_exceeded.' },
          { id: 'valueDepth', name: 'Value nesting depth', value: LIMITS.maxValueDepth, note: 'Stops with value_limit_exceeded.' },
          { id: 'taskId', name: 'Task id length', value: LIMITS.taskIdBytes, note: 'Longer ids are rejected.' },
          { id: 'submitRate', name: 'Submissions per minute', value: `${LIMITS.submitPerMinute} per account`, note: 'Default. Over the limit returns 429.' },
        ],
          billing: {
          title: 'How a job is priced',
          body: 'Cost is derived from the execution receipt, not from wall-clock time. Every primitive expression, builtin call, user function call, and loop body operation adds usage units as it executes. One usage unit is 1 CPT.',
          formula: 'total_cpt = base_invocation_cpt + usage_units',
          rows: [
            { name: 'Base invocation', value: '1 CPT' },
            { name: 'Each usage unit', value: '1 CPT' },
          ],
          functionRows: [
            {
              id: 'len',
              name: 'len(value)',
              price: '6 CPT + argument usage',
              note: 'The call adds 5 usage units of function overhead plus 1 for the call expression. Evaluating value is metered separately; len adds no other fixed charge.',
            },
            {
              id: 'get',
              name: 'get(target, key)',
              price: '6 CPT + argument usage',
              note: 'The call adds 5 usage units of function overhead plus 1 for the call expression. Evaluating target and key is metered separately.',
            },
            {
              id: 'contains',
              name: 'contains(target, value)',
              price: '6 CPT + argument usage',
              note: 'The call adds 5 usage units of function overhead plus 1 for the call expression. Evaluating target and value is metered separately.',
            },
            {
              id: 'user-function',
              name: 'fn name(args) { ... }',
              price: '6 CPT + arguments + body usage',
              note: 'Each user-function call adds 5 overhead units plus 1 for the call expression, then charges the evaluated arguments and the metered work actually run in its body.',
            },
            {
              id: 'print',
              name: 'print(value)',
              price: '6 CPT + argument usage',
              note: 'print adds 5 usage units of output overhead plus 1 for the print statement. Evaluating value is metered separately; output is still subject to its limit.',
            },
          ],
          examples: [
            {
              id: 'len-receipt',
              title: 'len([1, 2, 3])',
              program: 'len([1, 2, 3]);',
              receiptUsageUnits: 11,
              totalCpt: 12,
              breakdown: '6 fixed call units + 5 units to evaluate the list literal = 11 usage units; 1 base invocation CPT makes 12 CPT total.',
            },
            {
              id: 'get-receipt',
              title: 'get({"hits": 4}, "hits")',
              program: 'get({"hits": 4}, "hits");',
              receiptUsageUnits: 10,
              totalCpt: 11,
              breakdown: '6 fixed call units + 4 units for the map and key arguments = 10 usage units; 1 base invocation CPT makes 11 CPT total.',
            },
            {
              id: 'user-function-receipt',
              title: 'A user function call',
              program: 'fn double(n) { return n * 2; } double(4);',
              receiptUsageUnits: 11,
              totalCpt: 12,
              breakdown: '6 fixed function-call units + 5 units for the argument, body expression, return, and statement evaluation = 11 usage units; 1 base invocation CPT makes 12 CPT total.',
            },
            {
              id: 'print-receipt',
              title: 'print("ok")',
              program: 'print("ok");',
              receiptUsageUnits: 7,
              totalCpt: 8,
              breakdown: '6 fixed print units + 1 unit for the string value = 7 usage units; 1 base invocation CPT makes 8 CPT total.',
            },
            {
              id: 'worked-receipt',
              title: 'The complete example above',
              program: MANAGED_EXAMPLE,
              receiptUsageUnits: 80,
              totalCpt: 81,
              breakdown: 'The published execution receipt records 80 usage units for this input; the 1 CPT base invocation charge makes the settled amount 81 CPT.',
            },
          ],
          platforms: [
            {
              id: 'linux',
              name: 'Linux',
              status: 'Supported',
              proof: 'Managed execution and proof generation are supported when the required prover dependencies are installed.',
            },
            {
              id: 'macos',
              name: 'macOS',
              status: 'Supported',
              proof: 'Managed execution and proof generation are supported through the native worker path.',
            },
            {
              id: 'wsl',
              name: 'WSL',
              status: 'Supported',
              proof: 'Use the Linux worker path inside WSL; the prover must be available in the WSL environment.',
            },
            {
              id: 'windows-native',
              name: 'Native Windows',
              status: 'Fail closed',
              proof: 'The native Windows worker has no managed-function prover, so managed jobs fail closed rather than settle.',
            },
          ],
          notes: [
            'The worker stops at your max_cpt, so the charge can never exceed the budget you set.',
            'Submission is rejected up front if max_cpt is below the quote for the resources you asked for.',
            'The receipt is stored before settlement, so a charge can always be recomputed from it.',
          ],
        },
        settlement: {
          title: 'Proof before settlement',
          body: `A managed job settles only from a cryptographic proof that the network verifies itself. The worker's own usage numbers are treated as a claim. If the proof is missing or fails verification the job fails and is not billed. Proving one job currently takes about ${LIMITS.proveSeconds} seconds, against a ${LIMITS.proverTimeoutSecs} second ceiling.`,
        },
        failures: [
          { code: 'parse_error', note: 'The source did not parse. The message carries line and column.' },
          { code: 'type_error', note: 'An operation received a value of the wrong type.' },
          { code: 'name_error', note: 'An identifier was used before it was defined.' },
          { code: 'arity_error', note: 'A function was called with the wrong number of arguments.' },
          { code: 'key_error', note: 'get was called with a key the map does not have.' },
          { code: 'index_error', note: 'A list index was out of range.' },
          { code: 'input_error', note: 'The JSON input could not be read as expected.' },
          { code: 'budget_exhausted', note: 'The job spent the whole max_cpt budget before finishing.' },
          { code: 'op_limit_exceeded', note: 'The job exceeded the operation ceiling.' },
          { code: 'loop_limit_exceeded', note: 'A for loop exceeded the iteration ceiling.' },
          { code: 'call_depth_exceeded', note: 'Calls nested deeper than the depth ceiling.' },
          { code: 'output_limit_exceeded', note: 'print produced more than the output ceiling.' },
          { code: 'value_limit_exceeded', note: 'A value grew past the size, item, or nesting ceiling.' },
          { code: 'cancelled', note: 'The task was stopped through the stop route.' },
          { code: 'runtime_error', note: 'An evaluation error that does not fall into the categories above.' },
        ],
      },
      terms: {
        summary: 'What Hivemind does today, what it deliberately does not do, and what is expected of you. This describes the current state of the system rather than a future roadmap.',
        groups: [
          {
            title: 'What CPT is',
            items: [
              'CPT is an internal usage and budget unit. It exists so jobs can be metered and capped.',
              'CPT is not money. There is no conversion to any currency or token, and no redemption path.',
              'Balances are held by the network operator. Treat CPT as quota, not as an asset.',
            ],
          },
          {
            title: 'Current operating model',
            items: [
              'Hivemind is designed to be run as a controlled or semi-controlled worker pool.',
              'Open provider onboarding, marketplace bidding, and public settlement are not complete.',
              'Operators are expected to run workers they own or have explicitly allowlisted.',
              'The account that submits a job and the account that deploys the selected worker can be different. Your own worker is not guaranteed to receive your job.',
              'Worker capability numbers are self-declared and are not yet independently calibrated.',
            ],
          },
          {
            title: 'What you may run',
            items: [
              'Only managed-function-v0 jobs. Package and archive execution has been removed.',
              'Jobs must fit the published size, operation, and budget limits.',
              'A job may not reach the filesystem, the network, other processes, or the host environment. The language has no way to express it.',
            ],
          },
          {
            title: 'What is not allowed',
            items: [
              'Attempting to escape the execution sandbox or reach the host from inside a job.',
              'Falsifying resource capability or usage numbers on a worker you operate.',
              'Interfering with scheduling, other accounts, or other workers.',
              'Using another account credentials, or sharing a token you were issued.',
            ],
          },
          {
            title: 'What is not promised',
            items: [
              'No uptime or availability guarantee, and no service level agreement.',
              'No dispute resolution process for billing or execution outcomes.',
              'No durability guarantee for job inputs, outputs, or results.',
              'Capacity is best effort. A job can be rejected, redispatched, or fail.',
            ],
          },
          {
            title: 'What is stored',
            items: [
              'Account records: username and a hashed password. Passwords are never stored in readable form.',
              'Job records: the source you submitted, the JSON input, the execution receipt, and the result.',
              'Settlement records: verification outcome and the amount charged.',
              'Treat job source and input as visible to the network operator. Do not put secrets in them.',
            ],
          },
          {
            title: 'If you operate a node',
            items: [
              'You deploy, secure, and administer your own Master and Worker nodes.',
              'You are responsible for the network egress policy and sandbox mode you configure.',
              'A worker that cannot produce a valid proof will have its managed jobs fail rather than settle.',
              'Nothing on this website controls your node. Operation happens on the surfaces you deploy.',
            ],
          },
        ],
      },
    },
  },
  zh: {
    brand: {
      name: 'Hivemind',
      strap: '跑在別人機器上的可驗證運算。',
    },
    routes: baseRoutes.zh,
    hero: {
      badge: '官方網站',
      title: '跑在別人機器上的可驗證運算',
      body: '送出一個函式、一份 JSON 輸入和一個花費上限。它會跑在網路上某台有餘裕的機器，而帳單來自網路端自己驗證的證明，不是機器自己報的數字。',
      primaryCta: '建立帳號',
      secondaryCta: '閱讀文件',
      bullets: [
        '依實際執行的運算次數計價',
        '沒有映像檔、沒有容器、沒有打包步驟',
        '證明沒過，就不結算',
      ],
    },
    sections: {
      stats: [
        { value: '1 CPT', label: '每個運算單位，另加 1 點呼叫費' },
        { value: '100%', label: '結算前經過證明驗證的工作' },
        { value: '0', label: '需要建置的映像檔或容器' },
        { value: '3', label: '要學的內建函式：len、get、contains' },
      ],
      features: [
        {
          title: '帳單來自查核，不是自我申報',
          body: 'worker 傳回的用量只是宣稱。網路端自己驗證證明，驗不過的工作永遠不會結算。',
        },
        {
          title: '沒有映像檔要建',
          body: '你送出的就是原始碼和一份 JSON。任務格式只有 managed-function-v0 一種，前面沒有打包這一段。',
        },
        {
          title: '預算是硬上限',
          body: 'max_cpt 一用完，執行就停，並以 budget_exhausted 失敗。不可能收你超過你設的那個數字。',
        },
        {
          title: '你的 Master 是你的；執行可能在別人的 worker',
          body: 'worker 可能由其他 Hivemind 使用者部署。排程器會選符合工作需求且已啟用的 worker，所以不保證工作一定送到你自己的 worker。',
        },
      ],
      workflow: [
        {
          step: '01',
          title: '建立帳號',
          body: '使用者名稱至少 3 個字，密碼至少 8 個字。',
        },
        {
          step: '02',
          title: '部署節點',
          body: 'Master 提供任務 API 與操作介面；Worker 可以由你或其他使用者部署，排程器會把工作送到合適且可用的 worker。',
        },
        {
          step: '03',
          title: '寫這個函式',
          body: 'let、fn、for、if，加上三個內建函式：len、get、contains。',
        },
        {
          step: '04',
          title: '先估價，再送出',
          body: '先要一份報價，再帶著你接受的 max_cpt 送出。',
        },
      ],
      security: {
        items: [
          'Worker 回報的用量只是宣稱，計費前一律查核。',
          '驗不過的工作不結算，直接失敗。',
          '瀏覽器只連這個網站，不連執行工作的機器。',
          '帳號存取與執行工作、機器管理分離。',
        ],
        pipelineTitle: '從工作跑完到帳單成立之間，發生了什麼',
        pipeline: [
          {
            step: '01',
            title: '工作在硬上限下執行',
            body: 'managed runtime 對每個運算計量，並在預算用盡時停止。這個語言本身就沒有開檔案、開連線或開行程的能力。',
          },
          {
            step: '02',
            title: 'worker 產生執行回執',
            body: '執行的運算次數、usage units、函式呼叫、迴圈迭代與輸出大小，都被記錄成結構化的回執。',
          },
          {
            step: '03',
            title: '為這次執行產生證明',
            body: 'worker 上另一個獨立的 prover 行程產生這次執行的密碼學證明，它不是執行工作的那個行程。',
          },
          {
            step: '04',
            title: '網路端驗證後才結算',
            body: '驗證發生在受信任的一端，並且會檢查證明是否來自網路釘選的那份 guest 程式。只有通過驗證才會結算。',
          },
        ],
        caveatsTitle: '這套機制不涵蓋什麼',
        caveats: [
          'worker 的能力數值（如 CPU、GPU 分數）目前為自行申報，尚未經獨立校準。',
          '證明只能在 Linux、macOS 與 WSL 上產生。原生 Windows worker 沒有 prover，其 managed 工作會直接失敗而不是結算。',
          'fail-closed 是預設政策。營運方可以放寬，而放寬後的模式會依 worker 提供的數字結算，那並不保有信任性質。',
          '驗證證明的是「工作照著它被寫的樣子執行」，不會審查這份工作被寫來做什麼。',
        ],
      },
      account: {
        summary: '這裡放你的身份與餘額。工作跑在你自己部署的節點上。',
        panels: [
          {
            title: '餘額',
            body: '查看目前的 CPT 餘額，不必打開操作台。',
          },
          {
            title: 'CPT 是什麼',
            body: '執行工作用的內部用量單位。它不是貨幣，也不能兌換成任何幣別。',
          },
          {
            title: '下一步',
            body: '要送工作就部署 Master，要貢獻算力就部署 Worker。',
          },
        ],
      },
      docs: {
        quickstart: [
          {
            step: '01',
            title: '在這裡建立帳號',
            body: '使用者名稱至少 3 個字元，密碼至少 8 個字元。登入後取得 bearer token，之後每一次呼叫都要帶。',
          },
          {
            step: '02',
            title: '部署節點',
            body: 'Master 節點提供任務 API 與 Master UI；Worker 節點負責執行工作並回報。Worker 可以是你部署的，也可以是其他使用者部署的，任務不保證回到你自己的 worker。',
          },
          {
            step: '03',
            title: '寫一個 managed function',
            body: '工作就是 managed-function-v0 語言的純文字原始碼，加上一份 JSON 輸入。沒有打包步驟。',
          },
          {
            step: '04',
            title: '估價、送出、取回',
            body: '先取得報價，帶著你接受的預算送出，然後輪詢任務列表直到回執與結果回來。',
          },
        ],
        groups: [
          {
            id: 'account',
            title: '帳號 API',
            note: '本網站與任何 Master 節點都提供。這三個也是本網站唯一會呼叫的路由。',
            rows: [
              { method: 'POST', path: ACCOUNT_API[0], note: '建立帳號。Body：username、password。' },
              { method: 'POST', path: ACCOUNT_API[1], note: '登入，回傳 bearer token，之後以 Authorization: Bearer <token> 帶入。' },
              { method: 'GET', path: ACCOUNT_API[2], note: '讀取已登入帳號的 CPT 餘額。' },
            ],
          },
          {
            id: 'tasks',
            title: '任務 API（在你自己部署的 Master 節點上）',
            note: '這些路由在你自己的 Master 節點，不在本網站。每次呼叫都需要 bearer token。',
            rows: [
              { method: 'POST', path: '/api/tasks/quote', note: '在正式送出前先估價。回傳 quoted_cpt 與各項目明細。' },
              { method: 'POST', path: '/api/tasks', note: '送出工作。max_cpt 低於報價會被拒絕。' },
              { method: 'GET', path: '/api/tasks', note: '列出你的任務，含狀態、回執欄位與結果。' },
              { method: 'POST', path: '/api/tasks/{task_id}/stop', note: '停止執行中的任務，執行會以 cancelled 失敗代碼中止。' },
              { method: 'GET', path: '/api/workers', note: '列出你的 Master 節點看得到的 worker 節點。' },
              { method: 'GET', path: '/health', note: '存活檢查，不需驗證。' },
            ],
          },
        ],
        taskFields: [
          { name: 'task_id', type: 'string', required: '必填', note: `僅限 ASCII 字母、數字與 - _ .，不可含 ".."，長度上限 ${LIMITS.taskIdBytes}。` },
          { name: 'runtime', type: 'string', required: '必填', note: '必須是 managed-function-v0，其他值會以 unsupported task runtime 拒絕。' },
          { name: 'task_source', type: 'string', required: '必填', note: `managed function 原始碼，上限 ${LIMITS.taskSourceBytes}。` },
          { name: 'torrent', type: 'string', required: '必填', note: `JSON 輸入文件，以字串傳入，在函式中以 input 取用，上限 ${LIMITS.jsonInputBytes}。` },
          { name: 'max_cpt', type: 'integer', required: '必填', note: `你的預算與硬上限，必須大於 0 且不超過 ${LIMITS.budgetUnits}。用盡時以 budget_exhausted 停止。` },
          { name: 'cpu_score', type: 'integer', required: '選填', note: 'worker 需具備的最低 CPU 能力，不可為負。' },
          { name: 'gpu_score', type: 'integer', required: '選填', note: '最低 GPU 能力，不可為負。' },
          { name: 'memory_gb', type: 'integer', required: '選填', note: '最低記憶體（GB），不可為負。' },
          { name: 'gpu_memory_gb', type: 'integer', required: '選填', note: '最低 GPU 記憶體（GB），不可為負。' },
          { name: 'storage_gb', type: 'integer', required: '選填', note: '最低儲存空間（GB），不可為負。' },
          { name: 'host_count', type: 'integer', required: '選填', note: '要放到幾個 worker 上，至少 1，預設 1。' },
          { name: 'location', type: 'string', required: '選填', note: '偏好的 worker 位置標籤。' },
        ],
        language: {
          intro: 'managed-function-v0 是目前唯一支援的工作格式。它是一個小而有界的語言：每個陳述式與運算式都會被計量，而且沒有任何管道可以碰到宿主機。',
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
            'for 只能迭代 list，且受迴圈上限約束。',
            'print 會寫入回執輸出，受輸出上限約束。',
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
          exampleNote: '搭配這份輸入執行，函式回傳 36、印出一行，並記錄 80 個 usage unit，因此結算為 81 CPT：1 點呼叫費，加上每單位 1 點。注意迴圈裡的累加寫法：要用 let 重新綁定，因為裸寫 name = value 會是 parse_error。',
          submitExample: SUBMIT_EXAMPLE,
        },
        limits: [
          { id: 'taskSource', name: '原始碼大小', value: LIMITS.taskSourceBytes, note: '超過即在送出時拒絕。' },
          { id: 'jsonInput', name: 'JSON 輸入大小', value: LIMITS.jsonInputBytes, note: '超過即在送出時拒絕。' },
          { id: 'budget', name: '預算上限（max_cpt）', value: LIMITS.budgetUnits, note: '單位為 usage unit，必須大於 0。' },
          { id: 'ops', name: '單一工作運算次數', value: LIMITS.maxOps, note: '超過以 op_limit_exceeded 中止。' },
          { id: 'loops', name: '迴圈迭代次數', value: LIMITS.maxLoopIterations, note: '超過以 loop_limit_exceeded 中止。' },
          { id: 'callDepth', name: '呼叫深度', value: LIMITS.maxCallDepth, note: '超過以 call_depth_exceeded 中止。' },
          { id: 'output', name: '輸出位元組', value: LIMITS.maxOutputBytes, note: '超過以 output_limit_exceeded 中止。' },
          { id: 'items', name: '單一集合元素數', value: LIMITS.maxCollectionItems, note: '超過以 value_limit_exceeded 中止。' },
          { id: 'valueDepth', name: '值巢狀深度', value: LIMITS.maxValueDepth, note: '超過以 value_limit_exceeded 中止。' },
          { id: 'taskId', name: 'task_id 長度', value: LIMITS.taskIdBytes, note: '過長會被拒絕。' },
          { id: 'submitRate', name: '每分鐘送出次數', value: `每帳號 ${LIMITS.submitPerMinute} 次`, note: '預設值，超過回傳 429。' },
        ],
        billing: {
          title: '一份工作怎麼計價',
          body: '費用由執行回執推導，不是看牆鐘時間。每個基本運算式、內建函式呼叫、使用者函式呼叫與迴圈主體運算，在執行時累加 usage unit；每 1 個 usage unit = 1 CPT，另加每份工作 1 CPT 基本呼叫費。',
          formula: 'total_cpt = base_invocation_cpt + usage_units',
          rows: [
            { name: '基本呼叫費', value: '1 CPT' },
            { name: '每個 usage unit', value: '1 CPT' },
          ],
          functionRows: [
            {
              id: 'len',
              name: 'len(value)',
              price: '6 CPT + 引數用量',
              note: '呼叫固定加 5 個 usage unit，呼叫運算式再加 1 個；value 的計算另按實際運算計量，len 沒有其他固定費。',
            },
            {
              id: 'get',
              name: 'get(target, key)',
              price: '6 CPT + 引數用量',
              note: '呼叫固定加 5 個 usage unit，呼叫運算式再加 1 個；target 與 key 的計算另按實際運算計量。',
            },
            {
              id: 'contains',
              name: 'contains(target, value)',
              price: '6 CPT + 引數用量',
              note: '呼叫固定加 5 個 usage unit，呼叫運算式再加 1 個；target 與 value 的計算另按實際運算計量。',
            },
            {
              id: 'user-function',
              name: 'fn name(args) { ... }',
              price: '6 CPT + 引數 + 本體用量',
              note: '每次使用者函式呼叫固定加 5 個 usage unit，呼叫運算式再加 1 個，接著計算引數與函式本體實際執行的計量工作。',
            },
            {
              id: 'print',
              name: 'print(value)',
              price: '6 CPT + 引數用量',
              note: 'print 固定加 5 個 usage unit，print 陳述式本身再加 1 個；value 的計算也會計量，輸出仍受大小上限限制。',
            },
          ],
          examples: [
            {
              id: 'len-receipt',
              title: 'len([1, 2, 3])',
              program: 'len([1, 2, 3]);',
              receiptUsageUnits: 11,
              totalCpt: 12,
              breakdown: '6 個固定呼叫用量，加上建立 list 的 5 個用量，共 11 個 usage unit；再加 1 CPT 基本呼叫費，合計 12 CPT。',
            },
            {
              id: 'get-receipt',
              title: 'get({"hits": 4}, "hits")',
              program: 'get({"hits": 4}, "hits");',
              receiptUsageUnits: 10,
              totalCpt: 11,
              breakdown: '6 個固定呼叫用量，加上 map 與 key 引數的 4 個用量，共 10 個 usage unit；再加 1 CPT 基本呼叫費，合計 11 CPT。',
            },
            {
              id: 'user-function-receipt',
              title: '使用者函式呼叫',
              program: 'fn double(n) { return n * 2; } double(4);',
              receiptUsageUnits: 11,
              totalCpt: 12,
              breakdown: '6 個固定函式呼叫用量，加上引數、本體運算、return 與陳述式評估的 5 個用量，共 11 個 usage unit；再加 1 CPT 基本呼叫費，合計 12 CPT。',
            },
            {
              id: 'print-receipt',
              title: 'print("ok")',
              program: 'print("ok");',
              receiptUsageUnits: 7,
              totalCpt: 8,
              breakdown: '6 個固定 print 用量，加上字串值的 1 個用量，共 7 個 usage unit；再加 1 CPT 基本呼叫費，合計 8 CPT。',
            },
            {
              id: 'worked-receipt',
              title: '上面的完整範例',
              program: MANAGED_EXAMPLE,
              receiptUsageUnits: 80,
              totalCpt: 81,
              breakdown: '這份輸入的公開執行回執記錄 80 個 usage unit；加上 1 CPT 基本呼叫費，結算為 81 CPT。',
            },
          ],
          platforms: [
            {
              id: 'linux',
              name: 'Linux',
              status: '支援',
              proof: '安裝所需 prover 相依套件後，支援 managed 執行與產生 proof。',
            },
            {
              id: 'macos',
              name: 'macOS',
              status: '支援',
              proof: '透過原生 worker 路徑支援 managed 執行與產生 proof。',
            },
            {
              id: 'wsl',
              name: 'WSL',
              status: '支援',
              proof: '在 WSL 裡使用 Linux worker 路徑；prover 必須存在於 WSL 環境。',
            },
            {
              id: 'windows-native',
              name: '原生 Windows',
              status: 'fail closed',
              proof: '原生 Windows worker 沒有 managed-function prover，因此 managed 工作會 fail closed，不會結算。',
            },
          ],
          notes: [
            'worker 會在 max_cpt 用盡時停止，所以實收金額不可能超過你設定的預算。',
            '若 max_cpt 低於所要求資源的報價，送出當下就會被拒絕。',
            '回執會在結算前存檔，因此費用隨時可以依回執重新計算。',
          ],
        },
        settlement: {
          title: '先有證明，才有結算',
          body: `managed 工作只憑網路端自行驗證的密碼學證明結算，worker 自報的用量只被視為宣稱。證明缺漏或驗證失敗時，工作直接失敗且不計費。目前產生一份證明約需 ${LIMITS.proveSeconds} 秒，上限為 ${LIMITS.proverTimeoutSecs} 秒。`,
        },
        failures: [
          { code: 'parse_error', note: '原始碼無法解析，訊息會帶行號與欄位。' },
          { code: 'type_error', note: '運算收到型別不符的值。' },
          { code: 'name_error', note: '識別字在定義前就被使用。' },
          { code: 'arity_error', note: '函式呼叫的引數數量不符。' },
          { code: 'key_error', note: 'get 取用了 map 沒有的鍵。' },
          { code: 'index_error', note: 'list 索引超出範圍。' },
          { code: 'input_error', note: 'JSON 輸入無法依預期讀取。' },
          { code: 'budget_exhausted', note: '工作在結束前就用光了 max_cpt 預算。' },
          { code: 'op_limit_exceeded', note: '超過運算次數上限。' },
          { code: 'loop_limit_exceeded', note: 'for 迴圈超過迭代上限。' },
          { code: 'call_depth_exceeded', note: '呼叫巢狀超過深度上限。' },
          { code: 'output_limit_exceeded', note: 'print 產生的輸出超過上限。' },
          { code: 'value_limit_exceeded', note: '值超過大小、元素數或巢狀深度上限。' },
          { code: 'cancelled', note: '任務透過 stop 路由被停止。' },
          { code: 'runtime_error', note: '不屬於以上分類的求值錯誤。' },
        ],
      },
      terms: {
        summary: 'Hivemind 目前做什麼、刻意不做什麼，以及對你的期待。這裡描述的是系統現況，不是未來藍圖。',
        groups: [
          {
            title: 'CPT 是什麼',
            items: [
              'CPT 是內部的用量與預算單位，存在的目的是讓工作能被計量與設上限。',
              'CPT 不是貨幣。沒有任何幣別或代幣的兌換管道，也沒有贖回機制。',
              '餘額由網路營運方保管。請把 CPT 當成額度，而不是資產。',
            ],
          },
          {
            title: '目前的運作模式',
            items: [
              'Hivemind 設計上應以受控或半受控的 worker 池運作。',
              '開放式 provider 上線、市集競價與公開結算尚未完成。',
              '營運方應只運行自己擁有或明確列入允許清單的 worker。',
              '送出工作的帳號和部署被選中 worker 的帳號可以不同；你的工作不保證會送到你自己的 worker。',
              'worker 的能力數值目前為自行申報，尚未經獨立校準。',
            ],
          },
          {
            title: '你可以執行什麼',
            items: [
              '只有 managed-function-v0 工作。套件與壓縮檔執行已被移除。',
              '工作必須符合公開的大小、運算次數與預算上限。',
              '工作無法接觸檔案系統、網路、其他行程或宿主環境 —— 這個語言根本無從表達。',
            ],
          },
          {
            title: '不被允許的行為',
            items: [
              '嘗試脫離執行沙箱，或從工作內部觸及宿主機。',
              '在你營運的 worker 上偽造資源能力或用量數值。',
              '干擾排程、其他帳號或其他 worker。',
              '使用他人帳號憑證，或轉交你被核發的 token。',
            ],
          },
          {
            title: '不提供的保證',
            items: [
              '沒有可用性或正常運行時間保證，也沒有服務等級協議。',
              '沒有針對計費或執行結果的爭議處理程序。',
              '對工作的輸入、輸出與結果不提供持久性保證。',
              '算力供給為盡力而為，工作可能被拒絕、重新派送或失敗。',
            ],
          },
          {
            title: '會被保存的資料',
            items: [
              '帳號紀錄：使用者名稱與雜湊後的密碼。密碼不會以可讀形式保存。',
              '工作紀錄：你送出的原始碼、JSON 輸入、執行回執與結果。',
              '結算紀錄：驗證結果與實際扣款金額。',
              '請將工作原始碼與輸入視為網路營運方可見，不要放入機密資訊。',
            ],
          },
          {
            title: '如果你營運節點',
            items: [
              'Master 與 Worker 節點由你自己部署、加固與管理。',
              '你所設定的網路對外流量政策與沙箱模式，責任在你。',
              '無法產生有效證明的 worker，其 managed 工作會直接失敗而不是照樣結算。',
              '本網站不會控制你的節點，操作發生在你自己部署的介面上。',
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
