// Public copy and reference data for the official site.
//
// Values below are mirrored from the running system, not written by hand:
//   TASK_ID_MAX_BYTES, MANAGED_TASK_SOURCE_MAX_BYTES, MANAGED_JSON_INPUT_MAX_BYTES
//     -> hivemind-rs/crates/proto/src/lib.rs
//   ExecutionLimits::for_managed_function_budget()
//     -> executor-rs/crates/managed-function-runtime/src/lib.rs
//   Settlement rules for the current billing identity
//     -> hivemind-rs/crates/task-scheduler/src/task_repository.rs
//   HTTP routes and validation rules    -> hivemind-rs/crates/master-api/src/{routes,handlers}.rs
//   product limitations                 -> docs/PUBLIC_NETWORK_LIMITATIONS.md
// Keep one copy of every number here so the two locales can never drift apart.
import { docsGroupsEn, docsPagesEn } from './docs/pages-en.mjs';
import { docsGroupsZh, docsPagesZh } from './docs/pages-zh.mjs';

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
    "runtime": "hcode",
    "task_source": "let n = len(get(input, \\"rows\\")); n;",
    "torrent": "{\\"rows\\": [1, 2, 3]}",
    "max_cpt": 500,
    "cpu_score": 1000,
    "memory_gb": 2,
    "host_count": 1
  }'`;

const baseRoutes = {
  en: [
    { id: 'home', label: 'Home' },
    { id: 'login', label: 'Sign in' },
    { id: 'register', label: 'Create account' },
    { id: 'account', label: 'Account' },
    { id: 'forgot-password', label: 'Forgot password' },
    { id: 'forgot-username', label: 'Forgot username' },
    { id: 'verify-email', label: 'Verify email' },
    { id: 'reset-password', label: 'Reset password' },
    { id: 'pricing', label: 'Pricing' },
    { id: 'share', label: 'Share your computer' },
    { id: 'download', label: 'Download' },
    { id: 'security', label: 'Trust and security' },
    { id: 'faq', label: 'Status and FAQ' },
    { id: 'docs', label: 'Documentation' },
    { id: 'terms', label: 'Usage rules' },
    { id: 'terms-of-service', label: 'Terms of service' },
    { id: 'privacy', label: 'Privacy policy' },
  ],
  zh: [
    { id: 'home', label: '首頁' },
    { id: 'login', label: '登入' },
    { id: 'register', label: '建立帳號' },
    { id: 'account', label: '帳號中心' },
    { id: 'forgot-password', label: '忘記密碼' },
    { id: 'forgot-username', label: '忘記使用者名稱' },
    { id: 'verify-email', label: '驗證電子郵件' },
    { id: 'reset-password', label: '重設密碼' },
    { id: 'pricing', label: '價格' },
    { id: 'share', label: '分享你的電腦' },
    { id: 'download', label: '下載' },
    { id: 'security', label: '信任與安全' },
    { id: 'faq', label: '目前狀態與常見問題' },
    { id: 'docs', label: '文件' },
    { id: 'terms', label: '使用規範' },
    { id: 'terms-of-service', label: '用戶條款' },
    { id: 'privacy', label: '隱私權政策' },
  ],
};

const definitions = {
  en: {
    brand: {
      name: 'Hivemind',
      strap: 'Batch computing on shared computers',
      tagline: 'Run compute tasks on spare computers.',
    },
    routes: baseRoutes.en,
    // Top navigation. Every entry opens a page.
    nav: [
      { id: 'pricing', label: 'Pricing', route: 'pricing' },
      { id: 'share', label: 'Share your computer', route: 'share' },
      { id: 'security', label: 'Trust', route: 'security' },
      { id: 'faq', label: 'FAQ', route: 'faq' },
      { id: 'docs', label: 'Documentation', route: 'docs' },
      { id: 'download', label: 'Download', route: 'download' },
    ],
    footer: [
      {
        title: 'Product',
        links: [
          { label: 'Pricing', route: 'pricing' },
          { label: 'Share your computer', route: 'share' },
          { label: 'Download', route: 'download' },
          { label: 'Status', route: 'faq' },
        ],
      },
      {
        title: 'Developers',
        links: [
          { label: 'Documentation', route: 'docs' },
          { label: 'Task language', route: 'docs', slug: 'language' },
          { label: 'API reference', route: 'docs', slug: 'api-reference' },
        ],
      },
      {
        title: 'Trust',
        links: [
          { label: 'Security', route: 'security' },
          { label: 'Usage rules', route: 'terms' },
          { label: 'Terms of service', route: 'terms-of-service' },
          { label: 'Privacy policy', route: 'privacy' },
          { label: 'FAQ', route: 'faq' },
        ],
      },
      {
        title: 'Account',
        links: [
          { label: 'Sign in', route: 'login' },
          { label: 'Create account', route: 'register' },
          { label: 'Account', route: 'account' },
        ],
      },
    ],
    hero: {
      badge: 'First public version · Windows',
      title: 'Run compute tasks on spare computers',
      body: 'Describe a task and set the most you are willing to pay. Hivemind runs it on available computers and charges only for work that several computers confirm.',
      primaryCta: 'Create account',
      secondaryCta: 'Share your computer',
      bullets: [
        'Pay only for confirmed work',
        'No images, containers, or packages to build',
        'Unused credit returns to your account',
      ],
      mockTitle: 'Example task',
      mockNote: 'Illustration only',
      mockRows: [
        { label: 'Maximum you set', value: '100 CPT' },
        { label: 'Computers that ran it', value: '3' },
        { label: 'Result', value: 'Confirmed' },
        { label: 'Charged', value: '55 CPT', emphasis: true },
        { label: 'Returned', value: 'The rest of the hold' },
      ],
    },
    sections: {
      stats: [
        { value: '10%', label: 'platform fee on confirmed usage' },
        { value: '3', label: 'computers per task by default' },
        { value: '0', label: 'images or containers to build' },
      ],
      hub: {
        title: 'Start here',
        items: [
          {
            id: 'run',
            title: 'Run a task',
            body: 'Create an account, install Master, and send your first task from the Master window.',
            cta: 'Create account',
            route: 'register',
          },
          {
            id: 'share',
            title: 'Share your computer',
            body: 'Install Worker and sign in. Keep it running while you want to receive tasks.',
            cta: 'Installation steps',
            route: 'download',
          },
          {
            id: 'build',
            title: 'Build with hcode',
            body: 'Read the task language and the API reference. Developers can send tasks through the Master API.',
            cta: 'Read the documentation',
            route: 'docs',
          },
        ],
      },
      workflow: [
        {
          step: '01',
          title: 'Describe the task',
          body: 'Write the instructions and input data, or use hcode for custom logic. You do not build an image or container.',
        },
        {
          step: '02',
          title: 'Set the most it may cost',
          body: 'The maximum includes the 10% platform fee and covers every computer that runs the task.',
        },
        {
          step: '03',
          title: 'Pay for confirmed work',
          body: 'Several computers run the same task. The network compares their results and charges only for work it can confirm.',
        },
      ],
      workflowTitle: 'From task to confirmed result',
      workflowBody: 'Three steps. Nothing is charged until the network confirms the work.',
      pricing: {
        title: 'You set the limit. Confirmed work sets the charge.',
        body: 'Hivemind never charges more than the maximum you set, and it charges only for work the network has confirmed.',
        points: [
          'The maximum already includes the 10% platform fee.',
          'Charges follow the measured work of computers that agree on the result.',
          'A task without a confirmed result is not charged.',
          'Credit you do not use returns to your account.',
        ],
        example: {
          title: 'Example: a task with a 100 CPT maximum',
          rows: [
            { label: 'Maximum you set', value: '100 CPT' },
            { label: 'Confirmed work', value: '50 CPT' },
            { label: 'Platform fee (10%)', value: '5 CPT' },
            { label: 'Charged', value: '55 CPT', emphasis: true },
            { label: 'Returned', value: 'The rest of the hold' },
          ],
          note: 'Illustration only. Actual amounts depend on the measured work.',
        },
        creditNote: 'CPT is an internal unit for tasks. It is not money and cannot be withdrawn.',
      },
      featuresTitle: 'Why it works this way',
      features: [
        {
          title: 'Charges follow confirmed work',
          body: 'Each computer records the work it ran. The network compares these records and settles only the work it can confirm. Credit held for unconfirmed work returns to your account.',
        },
        {
          title: 'Nothing to package',
          body: 'Write the task and its input. You do not build an image or configure a container.',
        },
        {
          title: 'One limit for the whole task',
          body: 'The maximum applies to the whole task, not to each computer. Automatic paid retries are off, so a retry never adds a charge.',
        },
        {
          title: 'Tasks run on suitable computers',
          body: 'Hivemind picks an available computer that meets the task requirements. It may be your computer or one shared by another user, so your own computer is not a guaranteed destination.',
        },
      ],
      share: {
        title: 'Share the spare capacity of your computer.',
        body: 'Worker runs on 64-bit Windows. While it is signed in and running, the network can send it tasks.',
        steps: [
          { step: '01', title: 'Download Worker', body: 'Get the Windows package from the download page.' },
          { step: '02', title: 'Sign in', body: 'Your first sign-in connects this computer to the network.' },
          { step: '03', title: 'Keep it running', body: 'Leave Worker running while you want to receive tasks. You can stop sharing at any time.' },
        ],
        panelTitle: 'What runs on your computer',
        panelItems: [
          'Tasks are written in hcode, which cannot reach files, the network, or other programs.',
          'Each task runs within the limits its sender set.',
          'You can stop sharing from the Worker window at any time.',
          'Credit from shared work is an internal balance for your own tasks. It is not money.',
        ],
      },
      trust: {
        title: 'Results are checked before anything is charged.',
        cards: [
          { title: 'Several computers', body: 'Each task runs on several computers. The network compares what they return.' },
          { title: 'Matching results', body: 'A result counts only when enough matching results agree.' },
          { title: 'No confirmation, no charge', body: 'If the network cannot confirm the work, the task is not charged.' },
          { title: 'A closed task language', body: 'hcode cannot reach files, the network, or other programs on the computer.' },
        ],
        linkLabel: 'Read how checks work',
      },
      status: {
        title: 'What is available today',
        body: 'This is the first public version. Some parts of the platform are not available yet.',
        rows: [
          { label: 'Client platform', value: 'Windows, 64-bit' },
          { label: 'Task language', value: 'hcode' },
          { label: 'Linux, macOS, and Android clients', value: 'Not yet available' },
          { label: 'Downloads', value: 'Listed on the download page when a release is published' },
        ],
      },
      faq: {
        pageTitle: 'Status and questions',
        title: 'Questions',
        items: [
          {
            question: 'Do I need to install anything to run a task?',
            answer: 'Yes. Install Master, which runs on Windows. You do not build images or containers.',
          },
          {
            question: 'What is hcode?',
            answer: 'A small task language built for this network. A program written in hcode cannot reach files, the network, or other programs.',
          },
          {
            question: 'Can my own computer run my task?',
            answer: 'Possibly. Hivemind picks computers that meet the task requirements, and some of them belong to other users. Your own computer is not guaranteed.',
          },
          {
            question: 'Is CPT real money?',
            answer: 'No. CPT is an internal unit for tasks. It cannot be exchanged or withdrawn.',
          },
          {
            question: 'What happens if a task fails?',
            answer: 'A task without a confirmed result is not charged. Credit you did not use returns to your account.',
          },
          {
            question: 'Which computers can share?',
            answer: 'Windows computers at this stage. Linux, macOS, and Android are not available yet.',
          },
        ],
      },
      finalCta: {
        title: 'Create an account to get started.',
        body: 'Your account holds your credit balance. You sign in with it in Master and in Worker.',
        primaryCta: 'Create account',
        secondaryCta: 'Read the documentation',
      },
      security: {
        items: [
          'Each task runs on several computers, and the network compares what they record before anything is charged.',
          'A result counts only when enough matching results agree. Without agreement, there is no final result and nothing is charged.',
          'Your browser connects only to this website, never to the computers that run tasks.',
          'Signing in to your account is separate from running tasks and from sharing a computer.',
        ],
        pipelineTitle: 'How the network confirms a charge',
        pipeline: [
          {
            step: '01',
            title: 'Each replica has a fixed budget',
            body: 'After the fee, the execution budget is split into equal integer allowances for the default three replicas. A replica stops when it reaches its allowance. Tasks cannot open files, use the network, or start other programs.',
          },
          {
            step: '02',
            title: 'Each replica keeps a record',
            body: 'The task record lists the steps completed, the actions taken, repeated steps, and output size.',
          },
          {
            step: '03',
            title: 'Replicas are compared',
            body: 'Several computers run the same task. The network compares their results to see whether they agree.',
          },
          {
            step: '04',
            title: 'Only confirmed work is charged',
            body: 'With a quorum, the network charges valid replica usage plus the 10% fee, never more than max_cpt, and returns the rest of the hold. Without a quorum, nothing is charged.',
          },
        ],
        caveatsTitle: 'What this does not cover',
        caveats: [
          'Computer speed and capacity are reported by each owner. Hivemind does not verify the hardware.',
          'Agreement between replicas shows that they produced the same result. It does not prove that the result is correct.',
          'The first public version provides Windows clients only. Linux, macOS, and Android clients are not included.',
          'The network checks that a task follows the published rules. It does not review what the task is meant to do.',
          'Availability is best effort. A task can be rejected or fail, and a failed task is not charged.',
        ],
      },
      account: {
        summary: 'Your account holds your credit balance. Credits pay for the tasks you send.',
        panels: [
          {
            title: 'Credits',
            body: 'The balance available for your next task.',
          },
          {
            title: 'About credits',
            body: 'CPT is an internal unit for tasks. It is not money and cannot be withdrawn.',
          },
          {
            title: 'Next steps',
            body: 'Send a task from the Master window, or share a computer from the Worker window.',
          },
        ],
      },
      download: {
        title: 'Download and install',
        intro: 'Hivemind has two Windows applications. Master sends tasks. Worker shares your computer with the network. Both run on 64-bit Windows.',
        requirementsTitle: 'Requirements',
        requirements: [
          '64-bit Windows',
          'An internet connection',
          'Microsoft Edge WebView2 Runtime is recommended. Without it, the app opens in your default browser.',
        ],
        packages: [
          {
            id: 'master',
            name: 'Hivemind Master',
            summary: 'Sends tasks and shows their progress and results. Starting Master opens the Master window.',
          },
          {
            id: 'worker',
            name: 'Hivemind Worker',
            summary: 'Shares the spare capacity of this computer with the network. Keep it running while you want to receive tasks.',
          },
        ],
        unavailable: 'This release has not been published yet. The download link appears here when it is available.',
        versionLabel: 'Version',
        hashLabel: 'SHA-256',
        downloadLabel: 'Download',
        masterStepsTitle: 'Install Master',
        masterSteps: [
          'Download the package and extract it to a folder you can write to.',
          'Check the SHA-256 value (see below).',
          'Run hivemind-master.exe. The Master window opens.',
          'Sign in with your Hivemind account.',
          'Enter a task in the Master window and send it.',
        ],
        workerStepsTitle: 'Install Worker',
        workerSteps: [
          'Download the package and extract it to a folder you can write to.',
          'Check the SHA-256 value (see below).',
          'Run hivemind-worker.exe. The Worker window opens.',
          'Sign in with your Hivemind account. The first sign-in registers this computer with the network automatically.',
          'Keep Worker running while you want to receive tasks. When you close the window, you can choose Cancel, Keep in background, or Quit.',
        ],
        verifyTitle: 'Check the SHA-256 value',
        verifyBody: 'Open PowerShell in the folder that contains the file and run the command below. The output must match the SHA-256 value listed for that package on this page.',
        verifyCommand: 'Get-FileHash .\\<file-name> -Algorithm SHA256',
        warningTitle: 'If Windows shows a warning',
        warningBody: 'Windows may warn about an app it does not recognize. Before you run the file, confirm that the download address and the SHA-256 value match this page.',
        advancedTitle: 'Advanced settings',
        advancedBody: 'Master reads an optional .env file placed next to hivemind-master.exe. Worker can also be configured with the .env.worker.example file included in its package. Most people do not need either. Normal sign-in does not store your password or a reusable VPN key.',
      },
      docs: {
        groups: docsGroupsEn,
        pages: docsPagesEn,
        title: 'Write and run an hcode task',
        intro: 'hcode is the language for Hivemind tasks. You write a short program, give it a JSON input, and set the most the whole task may cost. Several computers run the same program, Nodepool checks that their results agree, and only confirmed work is charged.',
        flow: [
          { step: '1', title: 'Submit', body: 'Send the program, its JSON input, and the charge cap (max_cpt) from Master. The charge cap is the most the whole task can cost.' },
          { step: '2', title: 'Run', body: 'By default, Nodepool gives the task to three different Workers. Each one runs the program and reports a receipt that records its measured usage.' },
          { step: '3', title: 'Agree', body: 'A result is accepted when a quorum agrees. With three replicas, two matching results are enough. No single Worker decides the result.' },
          { step: '4', title: 'Settle', body: 'Nodepool charges the valid usage recorded by the replicas, adds the 10% platform fee, and releases the rest of the hold. A task that does not reach a quorum is not charged.' },
        ],
        terms: [
          { term: 'Task', body: 'One submitted program, with its input and settings.' },
          { term: 'Replica', body: 'One Worker run of a task. Three replicas run each task by default.' },
          { term: 'Quorum', body: 'The number of matching replica results needed to accept a result. With three replicas, the quorum is two.' },
          { term: 'Charge cap (max_cpt)', body: 'The most the whole task can cost, including the platform fee and all replicas. You set it for each task.' },
          { term: 'Usage unit', body: 'One measured step of work. One usage unit is 1 CPT of replica usage.' },
          { term: 'CPT', body: 'The credit unit that pays for tasks. It is not money. There is no conversion or withdrawal.' },
          { term: 'Hold', body: 'Credit reserved before a task runs, sized for the worst case. Reserved credit that is not used is released after settlement.' },
          { term: 'Receipt', body: 'The record a replica keeps of its run: status, usage, and counters. Nodepool validates receipts before they affect a charge.' },
          { term: 'Master', body: 'The desktop app where you sign in, submit tasks, and read the log.' },
          { term: 'Worker', body: 'The desktop app that runs replicas on your computer and shares its capacity.' },
          { term: 'Nodepool', body: 'The service that schedules tasks, checks results, and settles charges.' },
        ],
        quickstart: [
          { step: '01', title: 'Create an account', body: 'Choose a username and a password of at least 8 characters. Sign in to see your CPT balance.' },
          { step: '02', title: 'Install Master', body: 'Download Master from the download page and run hivemind-master.exe. The Master window opens. Sign in with the account you created.' },
          { step: '03', title: 'Write the program and its input', body: 'Use the program in the next section. It reads a list from the input, adds the numbers together, and returns the total.' },
          { step: '04', title: 'Set the charge cap and submit', body: 'Paste the program and the input into the submit form. Set Task charge cap (CPT, fee included) to 1000, then press Submit. Replicas can run on computers that another Hivemind user shares, so your own computer is not a guaranteed destination.' },
          { step: '05', title: 'Read the result', body: 'Open Log for the final value and any printed text. If all three replicas succeed and each records 25 usage units, the task is charged 82 CPT: 75 units of usage plus a 7 CPT fee. The unused part of the 999 CPT hold, 917 CPT, is released.' },
        ],
        program: {
          intro: 'A program is a sequence of statements. The value of the last expression statement is the result, unless an earlier return ends the program first.',
          points: [
            'The name input holds the JSON you submitted, already parsed. Read a field with get(input, "key").',
            'Put text such as Chinese in the JSON input. String literals in the source are not reliable for non-ASCII text.',
            'hcode is not JavaScript or Python. Write get(input, "values"), not input.values.',
            'Source comments are not supported. Do not use //, #, or /* */.',
            'print(value); adds text to the log. Printed text is separate from the final value.',
            'The final value is shown as compact JSON. A top-level string is shown as plain text.',
          ],
          example: MANAGED_EXAMPLE,
          exampleInput: MANAGED_EXAMPLE_INPUT,
          exampleResult: '36',
          exampleNote: 'With this input, each replica returns 36, prints rows scanned, and records 80 usage units. Inside the loop, let total = total + score(row); updates the total declared before the loop. A bare total = total + score(row); is a parse error.',
        },
        language: {
          intro: 'hcode has a small set of statements, expressions, and three builtin functions. Syntax outside this set fails with parse_error, and a name that is not defined fails with name_error.',
          statements: [
            { form: 'let name = expression;', note: 'Creates a variable, or rebinds an existing one. Inside a for body, let total = total + value; updates the total declared before the loop.' },
            { form: 'target[index] = expression;', note: 'Replaces one element of a list or dict. The target must be a variable name.' },
            { form: 'fn name(a, b) { return expression; }', note: 'Defines a function at the top level. Calls can nest up to 64 levels deep.' },
            { form: 'for item in list { ... }', note: 'Runs the block once for each element of a list. Any other value fails with type_error.' },
            { form: 'print(expression);', note: 'Adds text to the log. Printed output is limited to 1 MiB.' },
            { form: 'return expression;', note: 'Ends the program, or the function, with a value.' },
            { form: 'expression;', note: 'The last such statement in the program gives the result.' },
          ],
          expressions: [
            { form: '42   -7', note: 'Signed 64-bit integers. A result outside that range fails with integer_arithmetic_overflow.' },
            { form: 'true   false   null', note: 'Boolean values and null.' },
            { form: '"text"', note: 'Double quotes. Escapes: \\" \\\\ \\n \\r \\t. There is no \\uXXXX escape.' },
            { form: '[1, 2, 3]   {"key": value}', note: 'Lists and dicts. Dict keys are strings.' },
            { form: '+   -   *   /', note: 'Integer arithmetic. Division truncates toward zero, so -5 / 2 is -2. Division by zero fails with runtime_error.' },
            { form: '==   !=   <   <=   >   >=', note: 'Comparisons.' },
            { form: 'and   or   not', note: 'Boolean operators.' },
            { form: 'if condition { a } else { b }', note: 'An expression. The else branch is required, and each branch holds one expression.' },
            { form: 'list[i]   text[i]   dict["key"]', note: 'Indexing. Negative indexes count from the end of a list or text. A missing dict key fails with key_error. An index outside the list or text fails with index_error.' },
          ],
          builtins: [
            { sig: 'len(value)', note: 'Number of elements in a list or dict. For a string, the number of UTF-8 bytes.' },
            { sig: 'get(target, key)', note: 'Reads a dict by string key, or a list by an index that is zero or greater. A missing key or an index past the end returns null. A string target fails with type_error.' },
            { sig: 'contains(target, value)', note: 'True when a dict has the key, a string contains the text, or a list holds an equal element.' },
          ],
          unavailable: [
            'Floating-point numbers, and math functions such as abs, min, max, and sqrt. CPU hcode uses integers only.',
            'while, break, continue, the % operator, and member access such as input.values.',
            'Source comments, imports, file and network access, environment variables, subprocesses, eval, reflection, and host functions.',
            'Unbounded loops and unbounded recursion. A for loop runs over a list, and recursion stops at the call-depth limit.',
          ],
          rules: [
            'Identifiers use ASCII letters, digits, and _, and cannot start with a digit.',
            'There is no bare assignment. Use let to rebind a variable.',
            'JSON input and printed output are UTF-8. In source literals, use only the escapes listed above.',
          ],
        },
        settings: {
          intro: 'These fields make up the body of POST /api/tasks.',
          fields: [
            { name: 'task_id', type: 'text', required: 'required', note: 'A unique name for the task. Use ASCII letters, digits, hyphen, underscore, and dot. Two dots in a row are not allowed. At most 255 bytes.' },
            { name: 'runtime', type: 'text', required: 'required', note: 'Set to hcode. Retired runtime names are rejected.' },
            { name: 'task_source', type: 'text', required: 'required', note: 'The program text. It cannot be empty, and it can be at most 64 KiB.' },
            { name: 'torrent', type: 'text', required: 'required', note: 'The JSON input, sent as a string. The field keeps its older name, and it does not hold a torrent. Send the string null when the program needs no input. At most 1 MiB.' },
            { name: 'max_cpt', type: 'integer', required: 'required', note: 'A positive whole number: the most the whole task can cost, including the 10% platform fee and all replicas. See How charges work.' },
            { name: 'cpu_score', type: 'integer', required: 'optional', note: 'Lowest CPU score a computer must have to receive the task. Zero or greater.' },
            { name: 'gpu_score', type: 'integer', required: 'optional', note: 'Lowest GPU score a computer must have. Zero or greater.' },
            { name: 'memory_gb', type: 'integer', required: 'optional', note: 'Lowest memory in GB a computer must have. Zero or greater.' },
            { name: 'gpu_memory_gb', type: 'integer', required: 'optional', note: 'Lowest GPU memory in GB a computer must have. Zero or greater.' },
            { name: 'storage_gb', type: 'integer', required: 'optional', note: 'Lowest storage in GB a computer must have. Zero or greater.' },
            { name: 'host_count', type: 'integer', required: 'optional', note: 'Number of computers to place the task on. At least 1. Defaults to 1.' },
            { name: 'location', type: 'text', required: 'optional', note: 'Preferred location label for computers.' },
          ],
        },
        billing: {
          title: 'How charges work',
          body: 'Charges follow the work the replicas measured, not elapsed time. Each task has one charge cap, max_cpt, which covers the whole task. The default three replicas each receive the same whole-number execution budget, taken from the part of the cap that remains after the platform fee.',
          formula: 'max_cpt = whole-task charge cap, platform fee included\nreplica_budget = largest whole number B where R × B + floor(R × B / 10) ≤ max_cpt (R = replicas, 3 by default)\nfee = floor(10% of valid usage)\ncharged_cpt = valid usage + fee\ncharged_cpt ≤ max_cpt\nrefund_cpt = held_cpt − charged_cpt',
          rows: [
            { name: 'Charge cap', value: 'max_cpt, platform fee included, for the whole task' },
            { name: 'Usage', value: '1 usage unit = 1 CPT of replica usage' },
            { name: 'Platform fee', value: '10% of valid usage, rounded down' },
            { name: 'Replica budget', value: 'Equal whole-number shares. Any remainder stays with the owner.' },
            { name: 'Automatic retries', value: 'Disabled. Retry work is never charged.' },
          ],
          cases: [
            {
              id: 'cap-100',
              title: 'Cap of 100 CPT',
              lines: [
                'Replica budget: 30 CPT each, 90 CPT in total.',
                'Worst-case hold: 90 CPT of usage plus a 9 CPT fee, 99 CPT in total.',
                'The remaining 1 CPT stays with the owner and is not assigned to any replica.',
              ],
            },
            {
              id: 'cap-1000',
              title: 'Cap of 1000 CPT, all three replicas succeed',
              lines: [
                'Replica budget: 303 CPT each, 909 CPT in total.',
                'Worst-case hold: 909 CPT plus a 90 CPT fee, 999 CPT in total.',
                'Each replica records 25 usage units. Usage is 75, and the fee is floor(75 / 10) = 7 CPT.',
                'Charged: 82 CPT. Released: 999 − 82 = 917 CPT.',
              ],
            },
          ],
          functionRows: [
            {
              id: 'len',
              name: 'len(value)',
              price: '6 usage units + argument usage',
              note: 'The call adds 5 usage units of function overhead and 1 for the call expression. Evaluating the value is metered separately. len adds nothing else.',
            },
            {
              id: 'get',
              name: 'get(target, key)',
              price: '6 usage units + argument usage',
              note: 'The call adds 5 usage units of function overhead and 1 for the call expression. Evaluating the target and key is metered separately.',
            },
            {
              id: 'contains',
              name: 'contains(target, value)',
              price: '6 usage units + argument usage',
              note: 'The call adds 5 usage units of function overhead and 1 for the call expression. Evaluating the target and value is metered separately.',
            },
            {
              id: 'user-function',
              name: 'fn name(args) { ... }',
              price: '6 usage units + arguments + body usage',
              note: 'Each call adds 5 overhead units and 1 for the call expression. It then records the evaluated arguments and the metered work its body runs.',
            },
            {
              id: 'print',
              name: 'print(value)',
              price: '6 usage units + argument usage',
              note: 'print adds 5 usage units of output overhead and 1 for the statement. Evaluating the value is metered separately, and the output still counts toward the output limit.',
            },
          ],
          examples: [
            {
              id: 'len-receipt',
              title: 'len([1, 2, 3])',
              program: 'len([1, 2, 3]);',
              receiptUsageUnits: 11,
              breakdown: 'One replica records 6 fixed call units and 5 units to build the list, for 11 usage units.',
            },
            {
              id: 'get-receipt',
              title: 'get({"hits": 4}, "hits")',
              program: 'get({"hits": 4}, "hits");',
              receiptUsageUnits: 10,
              breakdown: 'One replica records 6 fixed call units and 4 units for the map and key arguments, for 10 usage units.',
            },
            {
              id: 'user-function-receipt',
              title: 'A user function call',
              program: 'fn double(n) { return n * 2; } double(4);',
              receiptUsageUnits: 11,
              breakdown: 'One replica records 6 fixed function-call units and 5 units for the argument, the body, the return, and the statement, for 11 usage units.',
            },
            {
              id: 'print-receipt',
              title: 'print("ok")',
              program: 'print("ok");',
              receiptUsageUnits: 7,
              breakdown: 'One replica records 6 fixed print units and 1 unit for the string, for 7 usage units.',
            },
            {
              id: 'worked-receipt',
              title: 'The complete example above',
              program: MANAGED_EXAMPLE,
              receiptUsageUnits: 80,
              breakdown: 'Each valid replica of this input records 80 usage units. The settled total adds the valid usage of all replicas, applies the 10% fee, and returns unused held credits.',
            },
          ],
          platforms: [
            {
              id: 'linux',
              name: 'Linux',
              status: 'Not included',
              check: 'Not part of the first public version. No Linux package is available.',
            },
            {
              id: 'macos',
              name: 'macOS',
              status: 'Not included',
              check: 'Not part of the first public version. No macOS package is available.',
            },
            {
              id: 'wsl',
              name: 'WSL',
              status: 'Not included',
              check: 'Not part of the first public version. The Linux worker path is not packaged for WSL.',
            },
            {
              id: 'windows-native',
              name: 'Native Windows',
              status: 'Supported',
              check: 'Included in the first public version. Windows Master and Worker are offered on the download page once a release is published.',
            },
          ],
          notes: [
            'max_cpt is the whole-task maximum charge, including the 10% fee and all replicas. It is not a per-replica limit.',
            'Automatic paid retries are disabled. Retry work is never charged to the task owner.',
            'With a quorum, valid replica usage plus the fee is charged, never more than max_cpt, and the unused hold is released. Without a quorum, the task is not charged.',
            'Holds created under earlier billing rules keep those rules and are not retroactively recalculated or refunded.',
          ],
        },
        settlement: {
          title: 'Quorum and settlement',
          body: 'Settlement happens only after a quorum agrees. It totals the valid usage of the replicas, adds the platform fee, and stays within the task max_cpt. The unused hold is released. A task without a quorum is not charged.',
        },
        limits: [
          { id: 'taskSource', name: 'Program size', value: LIMITS.taskSourceBytes, note: 'A larger program is rejected at submission.' },
          { id: 'jsonInput', name: 'JSON input size', value: LIMITS.jsonInputBytes, note: 'A larger input is rejected at submission.' },
          { id: 'callDepth', name: 'Call depth', value: LIMITS.maxCallDepth, note: 'Deeper calls stop the task with call_depth_exceeded.' },
          { id: 'output', name: 'Printed output', value: LIMITS.maxOutputBytes, note: 'More output stops the task with output_limit_exceeded.' },
          { id: 'valueBytes', name: 'One value', value: LIMITS.maxValueBytes, note: 'A single value larger than this stops the task with value_limit_exceeded.' },
          { id: 'items', name: 'Elements in one list or dict', value: LIMITS.maxCollectionItems, note: 'More elements stop the task with value_limit_exceeded.' },
          { id: 'valueDepth', name: 'Nesting depth of a value', value: LIMITS.maxValueDepth, note: 'Deeper nesting stops the task with value_limit_exceeded.' },
          { id: 'materialization', name: 'Values created in one run', value: LIMITS.maxValueMaterializationBytes, note: 'Creating more than this stops the task with value_limit_exceeded.' },
          { id: 'taskId', name: 'Task ID length', value: LIMITS.taskIdBytes, note: 'A longer ID is rejected.' },
          { id: 'submitRate', name: 'Submissions per minute', value: `${LIMITS.submitPerMinute} per account by default`, note: 'Requests over the limit receive HTTP 429. Operators can change the limit.' },
          { id: 'deadline', name: 'Round deadline', value: '120 seconds by default', note: 'Replicas that do not report within this time count as timed out. Operators can change the deadline.' },
        ],
        failures: [
          { code: 'parse_error', note: 'The program is not valid hcode. The message describes the problem.' },
          { code: 'type_error', note: 'A value has the wrong type for an operation or builtin, such as a for loop over a dict, or an if condition that is not a boolean.' },
          { code: 'name_error', note: 'A variable or function is not defined. Math functions such as abs are not available in CPU hcode, so they also report name_error.' },
          { code: 'arity_error', note: 'A function or builtin received the wrong number of arguments.' },
          { code: 'key_error', note: 'A dict index such as d["key"] named a key the dict does not contain. get returns null for a missing key instead.' },
          { code: 'index_error', note: 'A list or text index is outside the valid range.' },
          { code: 'input_error', note: 'The JSON input could not be parsed.' },
          { code: 'budget_exhausted', note: 'The replica used its whole execution budget before it finished. Reduce the work or raise max_cpt.' },
          { code: 'integer_arithmetic_overflow', note: 'An integer result left the signed 64-bit range.' },
          { code: 'runtime_error', note: 'Division by zero, or another evaluation error not listed here.' },
          { code: 'call_depth_exceeded', note: 'Calls nested deeper than 64 levels.' },
          { code: 'output_limit_exceeded', note: 'print wrote more than 1 MiB of output.' },
          { code: 'value_limit_exceeded', note: 'A value grew past its size, element-count, or nesting limit.' },
          { code: 'cancelled', note: 'The task was stopped with the stop route.' },
        ],
        api: {
          intro: 'Two sets of routes are documented here. The account routes are served by this website and by the Master API. The task routes are served by the Master API while Master is running. Send the bearer token from POST /api/login as Authorization: Bearer <token>.',
          groups: [
            {
              id: 'account',
              title: 'Account routes',
              note: 'Served by this website. The Master API serves the same paths.',
              rows: [
                { method: 'POST', path: ACCOUNT_API[0], note: 'Create an account. Body: username and password.' },
                { method: 'POST', path: ACCOUNT_API[1], note: 'Sign in. Returns a bearer token.' },
                { method: 'GET', path: ACCOUNT_API[2], note: 'Return the CPT balance of the signed-in account.' },
              ],
            },
            {
              id: 'tasks',
              title: 'Task routes (Master API)',
              note: 'Served by the Master API while Master is running. Every route except /health needs a bearer token.',
              rows: [
                { method: 'POST', path: '/api/tasks/quote', note: 'Estimate the charge before you submit. The response includes quoted_cpt and a breakdown by component.' },
                { method: 'POST', path: '/api/tasks', note: 'Submit a task. Rejected when max_cpt is below the quote.' },
                { method: 'GET', path: '/api/tasks', note: 'List your tasks with their status and charges.' },
                { method: 'GET', path: '/api/tasks/{task_id}/log', note: 'Read the log of a task, including its printed output.' },
                { method: 'GET', path: '/api/tasks/{task_id}/result', note: 'Read the final result of a finished task.' },
                { method: 'POST', path: '/api/tasks/{task_id}/stop', note: 'Stop a task. It ends with the cancelled error code.' },
                { method: 'GET', path: '/api/workers', note: 'List the computers available to your account.' },
                { method: 'GET', path: '/health', note: 'Health check. No authentication required.' },
              ],
            },
          ],
          footnote: 'The Master API also has routes for VPN enrollment, provider settings, and administration. The desktop apps use them. They are not documented here.',
          submitExample: SUBMIT_EXAMPLE,
          submitNote: 'Replace $TOKEN with the token from POST /api/login. This request runs a program that returns the number of rows, which is 3.',
        },
        versions: {
          platformsBody: 'The first public version supports native Windows only.',
        },
      },
      terms: {
        summary: 'Hivemind is in testing. This page describes the current rules for credits, shared computers, and stored data, and the things the service does not promise.',
        groups: [
          {
            title: 'What credits are',
            items: [
              'CPT is an internal unit for budgeting and paying for tasks.',
              'CPT is not money, and there is no conversion or withdrawal.',
              'Credits stay in your Hivemind account and can only be used for tasks.',
            ],
          },
          {
            title: 'Current service model',
            items: [
              'The first public version runs on a limited group of participating computers.',
              'Open participation, bidding, and public rewards are not available.',
              'Computer owners agree to follow the published rules.',
              'The account that submits a task and the account that shares a computer can differ. A task may run on another user\'s computer, and it is not guaranteed to run on your own.',
              'Each owner reports the capacity of their computer. The figures may not be exact.',
            ],
          },
          {
            title: 'What you can run',
            items: [
              'Only the hcode task language is available.',
              'Tasks must stay within the published size, operation, and credit limits.',
              'Tasks cannot access files, the network, other processes, or the computer that runs them.',
            ],
          },
          {
            title: 'What is not allowed',
            items: [
              'Trying to escape task limits or reach the computer that runs a task.',
              'Falsifying the capacity or activity of a shared computer.',
              'Interfering with task assignment, other accounts, or other computers.',
              'Using another person\'s credentials, or sharing a token issued to you.',
            ],
          },
          {
            title: 'What is not promised',
            items: [
              'No uptime guarantee and no service level agreement.',
              'No dispute resolution for charges or results.',
              'No durability guarantee for task inputs, outputs, or results.',
              'Availability is best effort. Tasks can be rejected or fail.',
            ],
          },
          {
            title: 'What is stored',
            items: [
              'Account records: username and a hashed password. Passwords are never stored in readable form.',
              'Task records: the instructions, the input, the activity record, and the result.',
              'Charge records: the network\'s check result and the amount charged.',
              'Anyone who runs Hivemind may see task instructions and input. Do not submit secrets.',
            ],
          },
          {
            title: 'If you share a computer',
            items: [
              'Set up and maintain the computer you share.',
              'You are responsible for the network access and safety settings you configure.',
              'If the network cannot confirm a task, the task fails and is not charged.',
              'This website does not control the computers that are shared.',
            ],
          },
        ],
      },
    },
  },
  zh: {
    brand: {
      name: 'Hivemind',
      strap: '在共享電腦上執行批次工作',
      tagline: '在閒置的電腦上執行運算工作。',
    },
    routes: baseRoutes.zh,
    nav: [
      { id: 'pricing', label: '價格', route: 'pricing' },
      { id: 'share', label: '分享你的電腦', route: 'share' },
      { id: 'security', label: '信任與安全', route: 'security' },
      { id: 'faq', label: '常見問題', route: 'faq' },
      { id: 'docs', label: '文件', route: 'docs' },
      { id: 'download', label: '下載', route: 'download' },
    ],
    footer: [
      {
        title: '產品',
        links: [
          { label: '價格', route: 'pricing' },
          { label: '分享你的電腦', route: 'share' },
          { label: '下載', route: 'download' },
          { label: '目前狀態', route: 'faq' },
        ],
      },
      {
        title: '開發者',
        links: [
          { label: '文件', route: 'docs' },
          { label: '工作語言', route: 'docs', slug: 'language' },
          { label: 'API 參考', route: 'docs', slug: 'api-reference' },
        ],
      },
      {
        title: '信任',
        links: [
          { label: '安全機制', route: 'security' },
          { label: '使用規範', route: 'terms' },
          { label: '用戶條款', route: 'terms-of-service' },
          { label: '隱私權政策', route: 'privacy' },
          { label: '常見問題', route: 'faq' },
        ],
      },
      {
        title: '帳號',
        links: [
          { label: '登入', route: 'login' },
          { label: '建立帳號', route: 'register' },
          { label: '帳號中心', route: 'account' },
        ],
      },
    ],
    hero: {
      badge: '首次公開版本 · Windows',
      title: '在閒置的電腦上執行運算工作',
      body: '描述工作，並設定你願意支付的最高費用。Hivemind 會把工作交給可用的電腦執行，只為多台電腦確認過的工作收費。',
      primaryCta: '建立帳號',
      secondaryCta: '分享你的電腦',
      bullets: [
        '只為確認的工作付費',
        '不需要建立映像檔、容器或安裝包',
        '未使用的額度會退回帳號',
      ],
      mockTitle: '範例工作',
      mockNote: '此為示意',
      mockRows: [
        { label: '設定的最高費用', value: '100 CPT' },
        { label: '執行的電腦數', value: '3' },
        { label: '結果', value: '已確認' },
        { label: '實際扣款', value: '55 CPT', emphasis: true },
        { label: '退回', value: '保留額度的剩餘部分' },
      ],
    },
    sections: {
      stats: [
        { value: '10%', label: '有效用量的平台費' },
        { value: '3', label: '每份工作預設使用的電腦數' },
        { value: '0', label: '需要建立的映像檔或容器' },
      ],
      hub: {
        title: '從這裡開始',
        items: [
          {
            id: 'run',
            title: '執行工作',
            body: '建立帳號、安裝 Master，然後在 Master 視窗送出第一份工作。',
            cta: '建立帳號',
            route: 'register',
          },
          {
            id: 'share',
            title: '分享你的電腦',
            body: '安裝 Worker 並登入。需要接收工作時，保持 Worker 執行即可。',
            cta: '安裝步驟',
            route: 'download',
          },
          {
            id: 'build',
            title: '用 hcode 撰寫',
            body: '閱讀工作語言與 API 參考。開發者也可以透過 Master API 送出工作。',
            cta: '閱讀文件',
            route: 'docs',
          },
        ],
      },
      workflow: [
        {
          step: '01',
          title: '描述工作',
          body: '寫好工作說明與輸入資料，或用 hcode 撰寫自訂邏輯。你不需要建立映像檔或容器。',
        },
        {
          step: '02',
          title: '設定最高費用',
          body: '最高費用包含 10% 平台費，並涵蓋所有執行這份工作的電腦。',
        },
        {
          step: '03',
          title: '只為確認的工作付費',
          body: '幾台電腦執行同一份工作，網路比對它們的結果，只為能確認的工作收費。',
        },
      ],
      workflowTitle: '從工作到確認的結果',
      workflowBody: '三個步驟。網路確認工作之前，不會扣任何費用。',
      pricing: {
        title: '你設定上限，確認的工作決定扣款。',
        body: 'Hivemind 的扣款不會超過你設定的最高費用，而且只依照網路確認過的工作計算。',
        points: [
          '最高費用已包含 10% 平台費。',
          '扣款依照結果達成一致的電腦所量測的工作計算。',
          '沒有確認結果的工作不會扣款。',
          '未使用的額度會退回你的帳號。',
        ],
        example: {
          title: '範例：最高 100 CPT 的工作',
          rows: [
            { label: '設定的最高費用', value: '100 CPT' },
            { label: '確認的工作量', value: '50 CPT' },
            { label: '平台費（10%）', value: '5 CPT' },
            { label: '實際扣款', value: '55 CPT', emphasis: true },
            { label: '退回', value: '保留額度的剩餘部分' },
          ],
          note: '此為示意，實際金額依量測結果而定。',
        },
        creditNote: 'CPT 是 Hivemind 內部用於工作的單位。它不是貨幣，也不能提領。',
      },
      featuresTitle: '這樣設計的原因',
      features: [
        {
          title: '只為網路能確認的工作扣款',
          body: '每台電腦都會記錄它執行的工作。網路比對這些紀錄，只結算能確認的部分。未能確認的工作所保留的額度，會退回帳號。',
        },
        {
          title: '不需要打包',
          body: '寫好工作內容與輸入資料即可送出，不需要建立映像檔，也不需要設定容器。',
        },
        {
          title: '上限涵蓋整份工作',
          body: '最高費用適用於整份工作，而不是每台電腦。自動付費重試會停用，重試不會增加費用。',
        },
        {
          title: '工作交給合適的電腦',
          body: 'Hivemind 會選擇符合條件的可用電腦，可能是你的電腦，也可能是其他使用者分享的電腦，不保證一定使用你自己的電腦。',
        },
      ],
      share: {
        title: '分享你的電腦的閒置運算資源。',
        body: 'Worker 在 64 位元 Windows 上執行。登入並保持執行時，網路可以把工作交給這台電腦。',
        steps: [
          { step: '01', title: '下載 Worker', body: '從下載頁取得 Windows 套件。' },
          { step: '02', title: '登入', body: '第一次登入會把這台電腦連到網路。' },
          { step: '03', title: '保持執行', body: '需要接收工作時，保持 Worker 執行。隨時都可以停止分享。' },
        ],
        panelTitle: '在你的電腦上執行的內容',
        panelItems: [
          '工作以 hcode 撰寫，無法存取檔案、網路或其他程式。',
          '每份工作只能在送出者設定的上限內執行。',
          '你可以隨時從 Worker 視窗停止分享。',
          '分享工作得到的額度，是你自己工作使用的內部餘額，不是貨幣。',
        ],
      },
      trust: {
        title: '扣款前，先確認結果。',
        cards: [
          { title: '多台電腦', body: '每份工作由多台電腦執行，網路比對它們回傳的結果。' },
          { title: '結果一致', body: '要有足夠的相符結果，結果才算數。' },
          { title: '無法確認就不扣款', body: '網路無法確認的工作，不會扣款。' },
          { title: '封閉的工作語言', body: 'hcode 無法存取電腦上的檔案、網路或其他程式。' },
        ],
        linkLabel: '閱讀查核方式',
      },
      status: {
        title: '目前提供的內容',
        body: '這是首次公開版本，部分功能尚未提供。',
        rows: [
          { label: '用戶端平台', value: 'Windows，64 位元' },
          { label: '工作語言', value: 'hcode' },
          { label: 'Linux、macOS 與 Android 用戶端', value: '尚未提供' },
          { label: '下載', value: '正式版本發布後會列在下載頁' },
        ],
      },
      faq: {
        pageTitle: '目前狀態與常見問題',
        title: '常見問題',
        items: [
          {
            question: '執行工作前需要安裝什麼嗎？',
            answer: '需要。請安裝 Master（Windows 版）。你不需要建立映像檔或容器。',
          },
          {
            question: 'hcode 是什麼？',
            answer: '為這個網路設計的小型工作語言。用 hcode 寫的程式無法存取檔案、網路或其他程式。',
          },
          {
            question: '我自己的電腦能執行我的工作嗎？',
            answer: '有可能。Hivemind 會選擇符合條件的電腦，其中有些可能屬於其他使用者。不保證一定使用你自己的電腦。',
          },
          {
            question: 'CPT 是真正的貨幣嗎？',
            answer: '不是。CPT 是工作用的內部單位，不能兌換或提領。',
          },
          {
            question: '工作失敗時會怎樣？',
            answer: '沒有確認結果的工作不會扣款。未使用的額度會退回你的帳號。',
          },
          {
            question: '哪些電腦可以分享？',
            answer: '目前只有 Windows 電腦。Linux、macOS 與 Android 尚未提供。',
          },
        ],
      },
      finalCta: {
        title: '建立帳號，開始執行工作。',
        body: '你的帳號保存額度餘額。Master 與 Worker 都使用同一個帳號登入。',
        primaryCta: '建立帳號',
        secondaryCta: '閱讀文件',
      },
      security: {
        items: [
          '每份工作都由多台電腦執行，網路在扣款前會比對它們記錄的內容。',
          '足夠的結果一致時，結果才算數。沒有共識的工作沒有最終結果，也不會扣款。',
          '瀏覽器只會連到這個網站，不會連到執行工作的電腦。',
          '登入帳號、執行工作與分享電腦是各自獨立的事。',
        ],
        pipelineTitle: '網路如何確認扣款',
        pipeline: [
          {
            step: '01',
            title: '每個副本有固定額度',
            body: '扣除平台費後的執行額度，會平均分成預設三個副本的整數額度。副本用完額度就停止。工作不能開啟檔案、連接網路或啟動其他程式。',
          },
          {
            step: '02',
            title: '每個副本保留紀錄',
            body: '工作紀錄包含完成的步驟、執行的動作、重複步驟數與輸出大小。',
          },
          {
            step: '03',
            title: '比對副本結果',
            body: '幾台電腦執行同一份工作，網路比對它們的結果，確認是否達成共識。',
          },
          {
            step: '04',
            title: '只為確認的工作扣款',
            body: '達成共識時，網路會依有效副本用量加上 10% 平台費扣款，不超過 max_cpt，並退回剩餘的保留額度。沒有達成共識時，不扣任何費用。',
          },
        ],
        caveatsTitle: '這套機制不涵蓋的範圍',
        caveats: [
          '電腦的速度與容量由各自的擁有者回報，Hivemind 不會驗證硬體。',
          '副本結果一致，代表它們產生了相同的結果，不代表結果一定正確。',
          '首次公開版本只提供 Windows 用戶端，不包含 Linux、macOS 與 Android 用戶端。',
          '網路會查核工作是否遵守公開規則，不會審查工作本身的目的。',
          '服務以盡力而為提供，工作可能被拒絕或失敗。失敗的工作不會扣款。',
        ],
      },
      account: {
        summary: '你的帳號保存額度餘額。額度用於支付你送出的工作。',
        panels: [
          {
            title: '額度',
            body: '下一份工作可以使用的餘額。',
          },
          {
            title: '關於額度',
            body: 'CPT 是工作用的內部單位，不是貨幣，也不能提領。',
          },
          {
            title: '下一步',
            body: '從 Master 視窗送出工作，或從 Worker 視窗分享電腦。',
          },
        ],
      },
      download: {
        title: '下載與安裝',
        intro: 'Hivemind 提供兩個 Windows 應用程式：Master 用來送出工作，Worker 用來分享你的電腦。兩者都需要 64 位元的 Windows。',
        requirementsTitle: '系統需求',
        requirements: [
          '64 位元的 Windows',
          '網際網路連線',
          '建議安裝 Microsoft Edge WebView2 Runtime。未安裝時，應用程式會改用預設瀏覽器開啟。',
        ],
        packages: [
          {
            id: 'master',
            name: 'Hivemind Master',
            summary: '送出工作，並查看進度與結果。啟動後會開啟 Master 視窗。',
          },
          {
            id: 'worker',
            name: 'Hivemind Worker',
            summary: '把這台電腦的閒置運算資源分享給網路。需要接收工作時，請保持 Worker 執行。',
          },
        ],
        unavailable: '這個版本尚未發布。發布後，下載連結會顯示在這裡。',
        versionLabel: '版本',
        hashLabel: 'SHA-256',
        downloadLabel: '下載',
        masterStepsTitle: '安裝 Master',
        masterSteps: [
          '下載套件，解壓縮到你可以寫入的資料夾。',
          '核對 SHA-256 雜湊值（見下方）。',
          '執行 hivemind-master.exe，會開啟 Master 視窗。',
          '使用你的 Hivemind 帳號登入。',
          '在 Master 視窗輸入工作，並按下「傳送任務」。',
        ],
        workerStepsTitle: '安裝 Worker',
        workerSteps: [
          '下載套件，解壓縮到你可以寫入的資料夾。',
          '核對 SHA-256 雜湊值（見下方）。',
          '執行 hivemind-worker.exe，會開啟 Worker 視窗。',
          '使用你的 Hivemind 帳號登入，按下「登入並連線」。第一次登入時，這台電腦會自動加入網路並完成註冊。',
          '需要接收工作時，請保持 Worker 執行。關閉視窗時，可以選擇取消、在背景繼續執行，或結束。',
        ],
        verifyTitle: '核對 SHA-256 雜湊值',
        verifyBody: '在檔案所在的資料夾開啟 PowerShell，執行下方指令。輸出結果必須與本頁列出的該套件 SHA-256 值相同。',
        verifyCommand: 'Get-FileHash .\\<檔案名稱> -Algorithm SHA256',
        warningTitle: '如果 Windows 顯示警告',
        warningBody: 'Windows 可能會對它不認識的應用程式發出警告。執行前，請先確認下載網址與 SHA-256 值和本頁相符。',
        advancedTitle: '進階設定',
        advancedBody: 'Master 會讀取放在 hivemind-master.exe 旁邊的 .env 檔案。Worker 可以使用套件內附的 .env.worker.example 進行設定。大多數人不需要這些設定。一般登入流程不會保存你的密碼或可重複使用的 VPN 金鑰。',
      },
      docs: {
        groups: docsGroupsZh,
        pages: docsPagesZh,
        title: '撰寫並執行 hcode 工作',
        intro: 'hcode 是 Hivemind 工作使用的語言。你寫一段短程式，提供 JSON 輸入，並設定整份工作最多可以收取的費用。多台電腦會執行同一段程式，Nodepool 比對它們的結果，只為確認過的工作扣款。',
        flow: [
          { step: '1', title: '送出', body: '從 Master 送出程式、JSON 輸入與收費上限（max_cpt）。收費上限是整份工作最多可以收取的費用。' },
          { step: '2', title: '執行', body: '預設由 Nodepool 把工作交給三台不同的 Worker。每台 Worker 執行同一段程式，並回報一份 receipt，記錄它實際量到的用量。' },
          { step: '3', title: '比對', body: '結果需要達到 quorum 才會被接受。三個副本時，兩個結果一致即可。沒有任何單一 Worker 能決定結果。' },
          { step: '4', title: '結算', body: 'Nodepool 依副本記錄的有效用量扣款，加上 10% 平台費，並釋放保留額度中未使用的部分。未達 quorum 的工作不會扣款。' },
        ],
        terms: [
          { term: '工作（Task）', body: '一次送出的程式，包含輸入資料與設定。' },
          { term: '副本（Replica）', body: '同一份工作在某一台 Worker 上的一次執行。預設每份工作有三個副本。' },
          { term: 'Quorum（共識門檻）', body: '接受結果所需的一致副本數。三個副本時，門檻是兩個。' },
          { term: '收費上限（max_cpt）', body: '整份工作最多可以收取的費用，包含平台費與所有副本。每份工作各自設定。' },
          { term: '用量單位（usage unit）', body: '一步被量測到的工作量。1 個 usage unit 等於 1 CPT 的副本用量。' },
          { term: 'CPT', body: '用來支付工作的額度單位，不是貨幣，也沒有兌換或提領管道。' },
          { term: '保留額度（hold）', body: '工作開始前，依最壞情況先保留的額度。結算後，未使用的保留額度會釋放。' },
          { term: 'Receipt', body: '副本對一次執行的記錄，包含狀態、用量與計數器。Nodepool 會先驗證 receipt，才會用它影響扣款。' },
          { term: 'Master', body: '你登入、送出工作並查看日誌的桌面程式。' },
          { term: 'Worker', body: '在你的電腦上執行副本，並分享運算能力的桌面程式。' },
          { term: 'Nodepool', body: '負責排程工作、比對結果並結算費用的服務。' },
        ],
        quickstart: [
          { step: '01', title: '建立帳號', body: '選擇使用者名稱與至少 8 個字元的密碼。登入後可以查看 CPT 餘額。' },
          { step: '02', title: '安裝 Master', body: '從下載頁下載 Master，並執行 hivemind-master.exe。Master 視窗會開啟，使用你建立的帳號登入。' },
          { step: '03', title: '撰寫程式與輸入', body: '使用下一節的程式。它從輸入中讀取一個 list，把數字加總後回傳。' },
          { step: '04', title: '設定收費上限並送出', body: '在送出表單貼上程式與輸入，把「任務收費上限（CPT，含平台費）」設為 1000，然後按下提交。副本可能在其他 Hivemind 使用者分享的電腦上執行，不保證一定使用你自己的電腦。' },
          { step: '05', title: '查看結果', body: '在日誌中查看最終值與印出的文字。若三個副本都成功，且各記錄 25 個 usage unit，工作會收取 82 CPT：75 個用量單位加上 7 CPT 平台費。保留的 999 CPT 中，未使用的 917 CPT 會釋放。' },
        ],
        program: {
          intro: '程式是一連串陳述式。最後一個運算式陳述式的值就是結果，除非更早的 return 先結束程式。',
          points: [
            '名稱 input 代表你送出的 JSON，已經解析完成。用 get(input, "key") 讀取欄位。',
            '中文等非 ASCII 文字請放在 JSON 輸入中。程式原始碼中的字串常值無法可靠保留非 ASCII 文字。',
            'hcode 不是 JavaScript 或 Python。請寫 get(input, "values")，不要寫 input.values。',
            '不支援原始碼註解。不要使用 //、# 或 /* */。',
            'print(value); 會把文字加入日誌。印出的文字與最終值是分開的。',
            '最終值以精簡的 JSON 顯示。頂層字串則以純文字顯示。',
          ],
          example: MANAGED_EXAMPLE,
          exampleInput: MANAGED_EXAMPLE_INPUT,
          exampleResult: '36',
          exampleNote: '使用這份輸入時，每個副本都回傳 36、印出 rows scanned，並記錄 80 個 usage unit。迴圈中的 let total = total + score(row); 會更新迴圈前宣告的 total。單獨寫 total = total + score(row); 會是 parse_error。',
        },
        language: {
          intro: 'hcode 的陳述式與運算式不多，內建函式只有三個。解析時遇到不支援的語法會以 parse_error 失敗；找不到的名稱會以 name_error 失敗。',
          statements: [
            { form: 'let name = expression;', note: '建立變數，或重新綁定既有的變數。在 for 迴圈內，let total = total + value; 會更新迴圈前宣告的 total。' },
            { form: 'target[index] = expression;', note: '取代 list 或 dict 中的一個元素。target 必須是變數名稱。' },
            { form: 'fn name(a, b) { return expression; }', note: '在頂層定義函式。呼叫可以巢狀到 64 層。' },
            { form: 'for item in list { ... }', note: '對 list 的每個元素執行一次區塊。其他型別會以 type_error 失敗。' },
            { form: 'print(expression);', note: '把文字加入日誌。印出的輸出上限為 1 MiB。' },
            { form: 'return expression;', note: '以一個值結束程式或函式。' },
            { form: 'expression;', note: '程式中最後一個這類陳述式的值就是結果。' },
          ],
          expressions: [
            { form: '42   -7', note: '有號 64 位元整數。結果超出範圍會以 integer_arithmetic_overflow 失敗。' },
            { form: 'true   false   null', note: '布林值與 null。' },
            { form: '"文字"', note: '使用雙引號。跳脫字元：\\" \\\\ \\n \\r \\t。不支援 \\uXXXX。' },
            { form: '[1, 2, 3]   {"key": value}', note: 'list 與 dict。dict 的鍵是字串。' },
            { form: '+   -   *   /', note: '整數運算。除法向零捨去，例如 -5 / 2 為 -2。除以零會以 runtime_error 失敗。' },
            { form: '==   !=   <   <=   >   >=', note: '比較運算。' },
            { form: 'and   or   not', note: '布林運算。' },
            { form: 'if 條件 { a } else { b }', note: '運算式。必須有 else 分支，每個分支只能放一個運算式。' },
            { form: 'list[i]   text[i]   dict["key"]', note: '索引。list 與字串的負索引從尾端計算。dict 沒有該鍵時會以 key_error 失敗；索引超出範圍則以 index_error 失敗。' },
          ],
          builtins: [
            { sig: 'len(value)', note: 'list 或 dict 的元素數；字串則是 UTF-8 位元組數。' },
            { sig: 'get(target, key)', note: '以字串鍵讀取 dict，或以非負整數索引讀取 list。鍵不存在或索引超出範圍時回傳 null。字串目標會以 type_error 失敗。' },
            { sig: 'contains(target, value)', note: 'dict 含有該鍵、字串包含該文字，或 list 含有相等的元素時為 true。' },
          ],
          unavailable: [
            '浮點數，以及 abs、min、max、sqrt 等數學函式。CPU hcode 只使用整數。',
            'while、break、continue、% 運算子，以及 input.values 這類成員存取。',
            '原始碼註解、import、檔案與網路存取、環境變數、子行程、動態 eval、反射與任意宿主函式。',
            '無界迴圈與無界遞迴。for 只迭代 list，遞迴受呼叫深度上限限制。',
          ],
          rules: [
            '識別字只能使用 ASCII 字母、數字與 _，且不可以數字開頭。',
            '沒有裸寫的賦值。要更新變數請用 let。',
            'JSON 輸入與輸出使用 UTF-8。原始碼字串中請只使用上面列出的跳脫字元。',
          ],
        },
        settings: {
          intro: '以下欄位組成 POST /api/tasks 的請求內容。',
          fields: [
            { name: 'task_id', type: '文字', required: '必填', note: '工作的唯一名稱。只能使用 ASCII 字母、數字、連字號、底線與點。不可含連續兩個點，最長 255 bytes。' },
            { name: 'runtime', type: '文字', required: '必填', note: '填入 hcode。已退役的 runtime 名稱會被拒絕。' },
            { name: 'task_source', type: '文字', required: '必填', note: '程式原始碼，不可為空，最多 64 KiB。' },
            { name: 'torrent', type: '文字', required: '必填', note: 'JSON 輸入，以字串傳入。欄位名稱沿用舊名，內容不是 torrent。程式不需要輸入時，填入字串 null。最多 1 MiB。' },
            { name: 'max_cpt', type: '整數', required: '必填', note: '正整數，代表整份工作最多可以收取的費用，包含 10% 平台費與所有副本。請見「費用如何計算」。' },
            { name: 'cpu_score', type: '整數', required: '選填', note: '電腦接收工作所需的最低 CPU 分數，不可為負。' },
            { name: 'gpu_score', type: '整數', required: '選填', note: '電腦所需的最低 GPU 分數，不可為負。' },
            { name: 'memory_gb', type: '整數', required: '選填', note: '電腦所需的最低記憶體（GB），不可為負。' },
            { name: 'gpu_memory_gb', type: '整數', required: '選填', note: '電腦所需的最低 GPU 記憶體（GB），不可為負。' },
            { name: 'storage_gb', type: '整數', required: '選填', note: '電腦所需的最低儲存空間（GB），不可為負。' },
            { name: 'host_count', type: '整數', required: '選填', note: '工作要放到的電腦數量，至少為 1，預設 1。' },
            { name: 'location', type: '文字', required: '選填', note: '電腦的偏好位置標籤。' },
          ],
        },
        billing: {
          title: '費用如何計算',
          body: '費用依副本實際量到的工作量計算，不是依經過的時間。每份工作都有一個收費上限 max_cpt，涵蓋整份工作。預設三個副本各自取得相同的整數執行額度，來源是收費上限扣除平台費後的部分。',
          formula: 'max_cpt = 整份工作的最高收費，含平台費\nreplica_budget = 最大整數 B，使 R × B + floor(R × B / 10) ≤ max_cpt（R 為副本數，預設 3）\nfee = floor(有效用量的 10%)\ncharged_cpt = 有效用量 + 平台費\ncharged_cpt ≤ max_cpt\nrefund_cpt = held_cpt − charged_cpt',
          rows: [
            { name: '收費上限', value: 'max_cpt，含平台費，涵蓋整份工作' },
            { name: '用量', value: '1 個 usage unit = 1 CPT 的副本用量' },
            { name: '平台費', value: '有效用量的 10%，無條件捨去' },
            { name: '副本額度', value: '相同的整數分配；餘數留在任務擁有者帳戶。' },
            { name: '自動重試', value: '已停用，重試用量不會收費。' },
          ],
          cases: [
            {
              id: 'cap-100',
              title: '收費上限 100 CPT',
              lines: [
                '副本額度：每個副本 30 CPT，合計 90 CPT。',
                '最壞情況保留：用量 90 CPT 加上平台費 9 CPT，合計 99 CPT。',
                '剩下的 1 CPT 留在任務擁有者帳戶，不分配給任何副本。',
              ],
            },
            {
              id: 'cap-1000',
              title: '收費上限 1000 CPT，三個副本都成功',
              lines: [
                '副本額度：每個副本 303 CPT，合計 909 CPT。',
                '最壞情況保留：909 CPT 加上平台費 90 CPT，合計 999 CPT。',
                '每個副本記錄 25 個 usage unit。用量為 75，平台費為 floor(75 / 10) = 7 CPT。',
                '收費 82 CPT。釋放 999 − 82 = 917 CPT。',
              ],
            },
          ],
          functionRows: [
            {
              id: 'len',
              name: 'len(value)',
              price: '6 個 usage unit + 引數用量',
              note: '呼叫固定加 5 個 usage unit，呼叫運算式再加 1 個。value 的計算另外按實際運算計量，len 沒有其他固定用量。',
            },
            {
              id: 'get',
              name: 'get(target, key)',
              price: '6 個 usage unit + 引數用量',
              note: '呼叫固定加 5 個 usage unit，呼叫運算式再加 1 個。target 與 key 的計算另外按實際運算計量。',
            },
            {
              id: 'contains',
              name: 'contains(target, value)',
              price: '6 個 usage unit + 引數用量',
              note: '呼叫固定加 5 個 usage unit，呼叫運算式再加 1 個。target 與 value 的計算另外按實際運算計量。',
            },
            {
              id: 'user-function',
              name: 'fn name(args) { ... }',
              price: '6 個 usage unit + 引數 + 本體用量',
              note: '每次呼叫固定加 5 個 usage unit，呼叫運算式再加 1 個。之後記錄引數，以及函式本體實際執行的計量工作。',
            },
            {
              id: 'print',
              name: 'print(value)',
              price: '6 個 usage unit + 引數用量',
              note: 'print 固定加 5 個 usage unit，陳述式本身再加 1 個。value 的計算另外計量，輸出仍受大小上限限制。',
            },
          ],
          examples: [
            {
              id: 'len-receipt',
              title: 'len([1, 2, 3])',
              program: 'len([1, 2, 3]);',
              receiptUsageUnits: 11,
              breakdown: '單一副本記錄 6 個固定呼叫用量，加上建立 list 的 5 個用量，共 11 個 usage unit。',
            },
            {
              id: 'get-receipt',
              title: 'get({"hits": 4}, "hits")',
              program: 'get({"hits": 4}, "hits");',
              receiptUsageUnits: 10,
              breakdown: '單一副本記錄 6 個固定呼叫用量，加上 map 與 key 引數的 4 個用量，共 10 個 usage unit。',
            },
            {
              id: 'user-function-receipt',
              title: '使用者函式呼叫',
              program: 'fn double(n) { return n * 2; } double(4);',
              receiptUsageUnits: 11,
              breakdown: '單一副本記錄 6 個固定函式呼叫用量，加上引數、本體運算、return 與陳述式的 5 個用量。',
            },
            {
              id: 'print-receipt',
              title: 'print("ok")',
              program: 'print("ok");',
              receiptUsageUnits: 7,
              breakdown: '單一副本記錄 6 個固定 print 用量，加上字串值的 1 個用量。',
            },
            {
              id: 'worked-receipt',
              title: '上面的完整範例',
              program: MANAGED_EXAMPLE,
              receiptUsageUnits: 80,
              breakdown: '這份輸入的每個有效副本記錄 80 個 usage unit。最終結算會合計所有有效副本的用量，加上 10% 費用，並退回未使用的保留額度。',
            },
          ],
          platforms: [
            {
              id: 'linux',
              name: 'Linux',
              status: '未包含',
              check: '不在首次公開版本內，目前沒有 Linux 套件。',
            },
            {
              id: 'macos',
              name: 'macOS',
              status: '未包含',
              check: '不在首次公開版本內，目前沒有 macOS 套件。',
            },
            {
              id: 'wsl',
              name: 'WSL',
              status: '未包含',
              check: '不在首次公開版本內。Linux Worker 路徑沒有為 WSL 打包。',
            },
            {
              id: 'windows-native',
              name: '原生 Windows',
              status: '支援',
              check: '包含在首次公開版本內。Windows 版 Master 與 Worker 會在正式發布後，於下載頁提供。',
            },
          ],
          notes: [
            'max_cpt 是整份任務的最高總收費，包含 10% 平台費與所有副本，不是單一副本的額度。',
            '自動付費重試會停用，重試用量不會向任務擁有者收費。',
            '達成 quorum 時，依有效副本用量加上平台費扣款，不超過 max_cpt，未使用的保留額度會釋放。未達 quorum 時，工作不會扣款。',
            '依舊有計費規則建立的保留額度，會維持原有規則，不會追溯重算或退款。',
          ],
        },
        settlement: {
          title: '達到 quorum 後結算',
          body: '只有達到 quorum 後才會結算。結算會合計副本的有效用量，加上平台費，並不超過工作的 max_cpt。未使用的保留額度會釋放。未達 quorum 的工作不會扣款。',
        },
        limits: [
          { id: 'taskSource', name: '程式大小', value: LIMITS.taskSourceBytes, note: '送出時超過即拒絕。' },
          { id: 'jsonInput', name: 'JSON 輸入大小', value: LIMITS.jsonInputBytes, note: '送出時超過即拒絕。' },
          { id: 'callDepth', name: '呼叫深度', value: LIMITS.maxCallDepth, note: '超過時以 call_depth_exceeded 中止工作。' },
          { id: 'output', name: '印出的輸出', value: LIMITS.maxOutputBytes, note: '超過時以 output_limit_exceeded 中止工作。' },
          { id: 'valueBytes', name: '單一值', value: LIMITS.maxValueBytes, note: '單一值超過時以 value_limit_exceeded 中止工作。' },
          { id: 'items', name: '單一 list 或 dict 的元素數', value: LIMITS.maxCollectionItems, note: '超過時以 value_limit_exceeded 中止工作。' },
          { id: 'valueDepth', name: '值的巢狀深度', value: LIMITS.maxValueDepth, note: '超過時以 value_limit_exceeded 中止工作。' },
          { id: 'materialization', name: '一次執行中建立的值總量', value: LIMITS.maxValueMaterializationBytes, note: '超過時以 value_limit_exceeded 中止工作。' },
          { id: 'taskId', name: 'task_id 長度', value: LIMITS.taskIdBytes, note: '超過時會被拒絕。' },
          { id: 'submitRate', name: '每分鐘送出次數', value: `每帳號預設 ${LIMITS.submitPerMinute} 次`, note: '超過時回傳 HTTP 429。營運者可以調整上限。' },
          { id: 'deadline', name: '回合時限', value: '預設 120 秒', note: '副本在時限內沒有回報，會視為逾時。營運者可以調整時限。' },
        ],
        failures: [
          { code: 'parse_error', note: '程式不是有效的 hcode，訊息會說明問題所在。' },
          { code: 'type_error', note: '運算或內建函式收到型別不符的值，例如對 dict 執行 for，或 if 的條件不是布林值。' },
          { code: 'name_error', note: '變數或函式沒有定義。CPU hcode 沒有 abs 等數學函式，因此呼叫它們也會回報 name_error。' },
          { code: 'arity_error', note: '函式或內建函式收到的引數數量不符。' },
          { code: 'key_error', note: '使用 d["key"] 這類 dict 索引時，指定的鍵不存在。get 遇到缺少的鍵則回傳 null。' },
          { code: 'index_error', note: 'list 或文字的索引超出有效範圍。' },
          { code: 'input_error', note: 'JSON 輸入無法解析。' },
          { code: 'budget_exhausted', note: '副本在完成前就用完了整份執行額度。請減少工作量，或提高 max_cpt。' },
          { code: 'integer_arithmetic_overflow', note: '整數結果超出有號 64 位元的範圍。' },
          { code: 'runtime_error', note: '除以零，或其他未列於此的執行錯誤。' },
          { code: 'call_depth_exceeded', note: '呼叫巢狀超過 64 層。' },
          { code: 'output_limit_exceeded', note: 'print 輸出超過 1 MiB。' },
          { code: 'value_limit_exceeded', note: '值超過大小、元素數或巢狀深度的上限。' },
          { code: 'cancelled', note: '工作透過 stop 路由被停止。' },
        ],
        api: {
          intro: '本頁列出兩組路由。帳號路由由本網站提供，Master API 也提供相同的路徑。工作路由在 Master 執行時由 Master API 提供。以 POST /api/login 取得的 bearer token，請放在 Authorization: Bearer <token> 標頭中。',
          groups: [
            {
              id: 'account',
              title: '帳號路由',
              note: '由本網站提供，Master API 也提供相同的路徑。',
              rows: [
                { method: 'POST', path: ACCOUNT_API[0], note: '建立帳號。Body：username 與 password。' },
                { method: 'POST', path: ACCOUNT_API[1], note: '登入，回傳 bearer token。' },
                { method: 'GET', path: ACCOUNT_API[2], note: '讀取已登入帳號的 CPT 餘額。' },
              ],
            },
            {
              id: 'tasks',
              title: '工作路由（Master API）',
              note: 'Master 執行時由 Master API 提供。除了 /health 以外，每個路由都需要 bearer token。',
              rows: [
                { method: 'POST', path: '/api/tasks/quote', note: '送出前先估價。回傳 quoted_cpt 與各項目的明細。' },
                { method: 'POST', path: '/api/tasks', note: '送出工作。max_cpt 低於估價時會被拒絕。' },
                { method: 'GET', path: '/api/tasks', note: '列出你的工作，含狀態與費用。' },
                { method: 'GET', path: '/api/tasks/{task_id}/log', note: '讀取工作的日誌，包含印出的輸出。' },
                { method: 'GET', path: '/api/tasks/{task_id}/result', note: '讀取已完成工作的最終結果。' },
                { method: 'POST', path: '/api/tasks/{task_id}/stop', note: '停止工作。工作會以 cancelled 錯誤代碼結束。' },
                { method: 'GET', path: '/api/workers', note: '列出你的帳號可以使用的電腦。' },
                { method: 'GET', path: '/health', note: '存活檢查，不需要驗證。' },
              ],
            },
          ],
          footnote: 'Master API 另外提供 VPN 加入、供應者設定與管理相關的路由。這些路由由桌面程式使用，本頁不說明。',
          submitExample: SUBMIT_EXAMPLE,
          submitNote: '把 $TOKEN 換成 POST /api/login 回傳的 token。這個請求執行的程式會回傳資料列數，結果為 3。',
        },
        versions: {
          platformsBody: '首次公開版本只支援原生 Windows。',
        },
      },
      terms: {
        summary: 'Hivemind 目前仍在測試。本頁說明目前的額度、共享電腦與資料保存規則，以及服務不提供的保證。',
        groups: [
          {
            title: '額度是什麼',
            items: [
              'CPT 是內部單位，用於工作的預算與支付。',
              'CPT 不是貨幣，也沒有兌換或提領管道。',
              '額度會保留在你的 Hivemind 帳號中，只用於工作。',
            ],
          },
          {
            title: '目前的服務方式',
            items: [
              '首次公開版本使用一組有限的參與電腦。',
              '公開加入、競價與公開獎勵目前都不提供。',
              '電腦擁有者需要遵守公布的規則。',
              '送出工作的帳號與分享電腦的帳號可以不同。工作可能在其他使用者的電腦上執行，不保證一定在你自己的電腦上執行。',
              '電腦能力由各自的擁有者回報，數字可能不完全準確。',
            ],
          },
          {
            title: '你可以執行什麼',
            items: [
              '目前只提供 hcode 工作語言。',
              '工作必須在公布的大小、運算與額度上限內。',
              '工作無法存取檔案、網路、其他行程，或執行它的電腦。',
            ],
          },
          {
            title: '不被允許的行為',
            items: [
              '試圖突破工作限制，或接觸執行工作的電腦。',
              '偽造共享電腦的容量或活動紀錄。',
              '干擾工作分派、其他帳號或其他電腦。',
              '使用他人的帳號憑證，或分享發給你的 token。',
            ],
          },
          {
            title: '不提供的保證',
            items: [
              '不保證可用性與連線時間，也沒有服務等級協議。',
              '對扣款或工作結果，沒有爭議處理程序。',
              '不保證工作的輸入、輸出與結果長期保存。',
              '服務以盡力而為提供，工作可能被拒絕或失敗。',
            ],
          },
          {
            title: '會保存的資料',
            items: [
              '帳號紀錄：使用者名稱與雜湊後的密碼。密碼不會以可讀形式保存。',
              '工作紀錄：工作內容、輸入資料、活動紀錄與結果。',
              '扣款紀錄：網路查核結果與實際扣款金額。',
              '執行 Hivemind 的人可能看得到工作內容與輸入資料，請不要送出機密資訊。',
            ],
          },
          {
            title: '分享電腦時',
            items: [
              '自行設定並維護你分享的電腦。',
              '你需要為自己設定的網路存取與安全選項負責。',
              '網路無法確認的工作會失敗，不會扣款。',
              '本網站不控制你分享的電腦。',
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
