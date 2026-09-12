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
      strap: 'Run tasks on a shared network.',
    },
    routes: baseRoutes.en,
    hero: {
      badge: 'Official site',
      title: 'Run tasks on a shared network',
      body: 'Send a task, set a credit limit, and let Hivemind choose an available computer. The network checks what happened before confirming the charge.',
      primaryCta: 'Create account',
      secondaryCta: 'Read the docs',
      bullets: [
        'Charges follow the work completed',
        'No images, containers, or packaging to manage',
        'Nothing is charged until the network agrees',
      ],
    },
    sections: {
      stats: [
        { value: '1 credit', label: 'Starting charge, plus the work completed' },
        { value: '100%', label: 'Network checked before charging' },
        { value: '0', label: 'Images or containers to manage' },
        { value: '3', label: 'Simple actions to learn' },
      ],
      features: [
        {
          title: 'The network checks before charging',
          body: 'A computer shares what happened, and the network compares the work before it charges your account. If the network cannot agree, the task stays uncharged.',
        },
        {
          title: 'No setup package',
          body: 'Send the task instructions and input. You do not need to build an image, container, or package.',
        },
        {
          title: 'Your credit limit is a hard ceiling',
          body: 'The task stops when it reaches the limit you set, so it cannot charge more than you approved.',
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
          'A computer reports what happened, and the network checks it before charging.',
          'A task the network cannot confirm stays uncharged and fails instead.',
          'Your browser only talks to this site, never to the machines running work.',
          'Account access is separate from running work and from sharing a computer.',
        ],
        pipelineTitle: 'How Hivemind confirms a charge',
        pipeline: [
          {
            step: '01',
            title: 'Your task stays within its credit limit',
            body: 'Hivemind measures the work and stops when it reaches your limit. Tasks cannot open files, connect to the network, or start other programs.',
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
            title: 'The network agrees, then confirms the charge',
            body: 'Only an agreed result can be charged. If the computers disagree or the check cannot finish, the task fails without a charge.',
          },
        ],
        caveatsTitle: 'What this does not cover',
        caveats: [
          'Computer speed and capacity are reported by each owner and may not be exact.',
          'Windows Workers can join the network and share a computer. A task that is not supported on a computer fails rather than being charged.',
          'Tasks do not run when the network cannot reach the required agreement.',
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
          exampleNote: 'Against that input the function returns 36, prints one line, and records 80 usage units. The total is therefore 81 CPT: one starting charge, plus one per unit. Note the loop accumulator: a value is rebound with let, because a bare name = value assignment is a parse error.',
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
          body: 'Cost is derived from the task record, not from wall-clock time. Every primitive expression, builtin call, user function call, and loop body operation adds usage units as it executes. One usage unit is 1 CPT.',
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
              breakdown: 'The published task record shows 80 usage units for this input; the 1 CPT starting charge makes the total 81 CPT.',
            },
          ],
          platforms: [
            {
              id: 'linux',
              name: 'Linux',
              status: 'Supported',
              check: 'Run the packaged Worker app or the supported Linux worker path. Tasks are checked by the network before charging.',
            },
            {
              id: 'macos',
              name: 'macOS',
              status: 'Supported',
              check: 'The native Worker path can run supported tasks and report them to the network for checking.',
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
              check: 'The packaged Windows Worker connects after sign-in. Tasks needing unavailable features or hardware fail without a charge.',
            },
          ],
          notes: [
            'The worker stops at your max_cpt, so the charge can never exceed the budget you set.',
            'Submission is rejected up front if max_cpt is below the quote for the resources you asked for.',
            'The task record is stored before the network confirms a charge, so the charge can be checked against the recorded work.',
          ],
        },
        settlement: {
          title: 'Network agreement before charging',
          body: 'A managed task is charged only after several participating computers report the same supported result and the network reaches a quorum. A single computer\'s usage report is not enough. If the computers disagree or the check cannot finish, the task fails and is not charged.',
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
      body: '送出工作、設定額度上限，讓 Hivemind 選一台可用的電腦。網路會先比較結果，再確認是否扣款。',
      primaryCta: '建立帳號',
      secondaryCta: '閱讀文件',
      bullets: [
        '依完成的工作計算額度',
        '不用管理映像檔、容器或打包檔',
        '網路達成共識後才會扣款',
      ],
    },
    sections: {
      stats: [
        { value: '1 CPT', label: '起始費用，再加上實際完成的工作' },
        { value: '100%', label: '扣款前經過網路查核' },
        { value: '0', label: '需要管理的映像檔或容器' },
        { value: '3', label: '學會三個簡單動作就能開始' },
      ],
      features: [
        {
          title: '網路查核後才扣款',
          body: '電腦回報做了什麼，網路會比較結果後才從帳號扣款。如果無法達成共識，工作不會扣款。',
        },
        {
          title: '不用準備打包檔',
          body: '送出工作說明和輸入資料即可，不需要自己建立映像檔、容器或套件。',
        },
        {
          title: '額度上限就是最高金額',
          body: '工作用完你設定的上限就會停止，不會扣超過你同意的額度。',
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
          '電腦回報完成的工作，網路會在扣款前進行查核。',
          '網路無法確認的工作會失敗，不會扣款。',
          '瀏覽器只連這個網站，不會直接連到執行工作的電腦。',
          '登入帳號、執行工作和分享電腦是分開的事情。',
        ],
        pipelineTitle: 'Hivemind 如何確認扣款',
        pipeline: [
          {
            step: '01',
            title: '工作遵守額度上限',
            body: 'Hivemind 會計算工作量，額度用完就停止。工作不能開啟檔案、連接網路或啟動其他程式。',
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
            title: '網路達成共識後扣款',
            body: '只有大家同意的結果可以扣款。如果結果不同或查核無法完成，工作會失敗且不扣款。',
          },
        ],
        caveatsTitle: '這套機制不涵蓋什麼',
        caveats: [
          '電腦速度和容量由各自的擁有者回報，可能不完全準確。',
          'Windows Worker 可以加入網路並分享電腦。不支援的工作會失敗，不會扣款。',
          '網路無法達成必要共識時，工作不會執行完成。',
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
          exampleNote: '搭配這份輸入執行，函式回傳 36、印出一行，並記錄 80 個 usage unit，因此總額為 81 CPT：1 點起始費用，加上每單位 1 點。注意迴圈裡的累加寫法：要用 let 重新綁定，因為裸寫 name = value 會是 parse_error。',
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
          body: '費用由工作記錄推導，不是看牆鐘時間。每個基本運算式、內建函式呼叫、使用者函式呼叫與迴圈主體運算，在執行時累加 usage unit；每 1 個 usage unit = 1 CPT，另加每份工作 1 CPT 起始費用。',
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
              breakdown: '這份輸入的公開工作記錄有 80 個 usage unit；加上 1 CPT 起始費用，總額為 81 CPT。',
            },
          ],
          platforms: [
            {
              id: 'linux',
              name: 'Linux',
              status: '支援',
              check: '可以使用支援的 Linux Worker 路徑。網路會在扣款前查核工作結果。',
            },
            {
              id: 'macos',
              name: 'macOS',
              status: '支援',
              check: '原生 Worker 路徑可以執行支援的工作，並回報給網路查核。',
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
              check: '打開 Windows Worker 並登入後即可連線。不符合電腦能力或支援範圍的工作會失敗且不扣款。',
            },
          ],
          notes: [
            'worker 會在 max_cpt 用盡時停止，所以實收金額不可能超過你設定的預算。',
            '若 max_cpt 低於所要求資源的報價，送出當下就會被拒絕。',
            '工作記錄會在網路確認扣款前保存，因此費用可以對照已記錄的工作重新檢查。',
          ],
        },
        settlement: {
          title: '網路達成共識後才扣款',
          body: 'managed 工作只有在多台參與工作的電腦回報相同結果，且網路達成足夠共識後才會扣款。單一電腦回報的用量不算數。如果結果不同或查核無法完成，工作會失敗且不扣款。',
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
