use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    error::Error,
    fs,
    panic::{self, AssertUnwindSafe},
    path::Path,
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::Duration,
};

use ahash::AHashMap;
use monty::{
    ExcType, ExtFunctionResult, LimitedTracker, MontyException, MontyObject, MontyRun, NameLookupResult, OsFunction,
    PrintWriter, ResourceLimits, RunProgress, dir_stat, file_stat,
};
use similar::TextDiff;

/// Recursion limit for test execution.
///
/// NOTE this value is chosen to avoid both:
/// * fixture recursion errors if it's too low
/// * stack overflows in debug Rust if it's too high
const TEST_RECURSION_LIMIT: usize = 50;

/// Test configuration parsed from directive comments.
///
/// Parsed from an optional first-line comment like `# xfail=monty` or `# call-external`.
/// If not present, defaults to running Monty in standard mode.
///
/// ## Xfail Semantics (Strict)
/// - `xfail=monty` - Test is expected to fail on Monty; if it passes, that's an error
#[derive(Debug, Clone, Default)]
#[expect(clippy::struct_excessive_bools)]
struct TestConfig {
    /// When true, test is expected to fail on Monty (strict xfail).
    xfail_monty: bool,
    /// When true, use MontyRun with external function support instead of MontyRun.
    iter_mode: bool,
}

/// Represents the expected outcome of a test fixture
#[derive(Debug, Clone)]
enum Expectation {
    /// Expect exception (parse-time or runtime) with specific message
    Raise(String),
    /// Expect successful execution, check py_str() output
    ReturnStr(String),
    /// Expect successful execution, check py_repr() output
    Return(String),
    /// Expect successful execution, check py_type() output
    ReturnType(String),
    /// Expect successful execution, check ref counts of named variables.
    /// Only used when `ref-count-return` feature is enabled; skipped otherwise.
    RefCounts(#[cfg_attr(not(feature = "ref-count-return"), expect(dead_code))] AHashMap<String, usize>),
    /// Expect exception with full traceback comparison.
    /// The expected traceback string should match exactly with Monty's output.
    Traceback(String),
    /// Expect successful execution without raising an exception (no return value check).
    /// Used for tests that rely on asserts or just verify code runs.
    NoException,
}

/// Parse a Python fixture file into code, expected outcome, and test configuration.
///
/// The file may optionally contain a `# xfail=monty` comment to specify that
/// Monty is expected to fail. If not present, the fixture is expected to pass.
///
/// The file may have an expectation comment as the LAST line:
/// - `# Raise=ExceptionType('message')` - Exception (parse-time or runtime)
/// - `# Return.str=value` - Check py_str() output
/// - `# Return=value` - Check py_repr() output
/// - `# Return.type=typename` - Check py_type() output
/// - `# ref-counts={'var': count, ...}` - Check ref counts of named heap variables
///
/// Or a traceback expectation as a triple-quoted string at the end (uses actual test filename):
/// ```text
/// """TRACEBACK:
/// Traceback (most recent call last):
///   File "my_test.monty", line 4, in <module>
///     foo()
/// ValueError: message
/// """
/// ```
///
/// If no expectation comment is present, the test just verifies the code runs without exception.
fn parse_fixture(content: &str) -> (String, Expectation, TestConfig) {
    let normalized_content = content.replace("\r\n", "\n");
    let content = normalized_content.as_str();
    let lines: Vec<&str> = content.lines().collect();

    assert!(!lines.is_empty(), "Empty fixture file");

    // comment lines with leading # and spaces stripped
    let comment_lines = lines
        .iter()
        .filter(|line| line.starts_with('#'))
        .map(|line| line.trim_start_matches('#').trim())
        .collect::<Vec<_>>();

    let mut config = TestConfig {
        iter_mode: comment_lines.iter().any(|line| line.starts_with("call-external")),
        ..Default::default()
    };
    // Check for "xfail=" directive
    if let Some(&xfail_line) = comment_lines.iter().find(|line| line.starts_with("xfail=")) {
        // Parse until whitespace or end of line
        let xfail_end = xfail_line.find(|c: char| c.is_whitespace()).unwrap_or(xfail_line.len());
        let xfail_str = &xfail_line[..xfail_end];
        config.xfail_monty = xfail_str.contains("monty");
    }

    // Check for TRACEBACK expectation (triple-quoted string at end of file)
    // Format: """TRACEBACK:\n...\n"""
    if let Some((code, traceback)) = parse_traceback_expectation(content) {
        return (code, Expectation::Traceback(traceback), config);
    }

    // Get the last line and check if it's an expectation comment
    let last_line = lines.last().unwrap();

    // Parse expectation from comment line if present
    // Note: Check more specific patterns first (Return.str, Return.type, ref-counts) before general Return
    let (expectation, code_lines) = if let Some(expected) = last_line.strip_prefix("# ref-counts=") {
        (
            Expectation::RefCounts(parse_ref_counts(expected)),
            &lines[..lines.len() - 1],
        )
    } else if let Some(expected) = last_line.strip_prefix("# Return.str=") {
        (Expectation::ReturnStr(expected.to_string()), &lines[..lines.len() - 1])
    } else if let Some(expected) = last_line.strip_prefix("# Return.type=") {
        (Expectation::ReturnType(expected.to_string()), &lines[..lines.len() - 1])
    } else if let Some(expected) = last_line.strip_prefix("# Return=") {
        (Expectation::Return(expected.to_string()), &lines[..lines.len() - 1])
    } else if let Some(expected) = last_line.strip_prefix("# Raise=") {
        (Expectation::Raise(expected.to_string()), &lines[..lines.len() - 1])
    } else {
        // No expectation comment - just run and check it doesn't raise
        (Expectation::NoException, &lines[..])
    };

    // Code is everything except the directive comment (and expectation comment if present)
    let code = code_lines.join("\n");

    (code, expectation, config)
}

/// Parses a TRACEBACK expectation from the end of a fixture file.
///
/// Looks for a triple-quoted string starting with `"""TRACEBACK:` at the end of the file.
/// Returns `Some((code, expected_traceback))` if found, `None` otherwise.
///
/// The traceback string should contain the full expected output including the
/// "Traceback (most recent call last):" header and the exception line.
fn parse_traceback_expectation(content: &str) -> Option<(String, String)> {
    // Format: """\nTRACEBACK:\n...\n"""
    const MARKER: &str = "\"\"\"\nTRACEBACK:\n";

    // Find the TRACEBACK marker
    let marker_pos = content.find(MARKER)?;

    // Extract the code before the marker
    let code_part = &content[..marker_pos];
    let lines: Vec<&str> = code_part.lines().collect();
    let code = lines.join("\n").trim_end().to_string();

    // Extract the traceback content between the markers
    let after_marker = &content[marker_pos + MARKER.len()..];

    // Find the closing triple quotes (preceded by newline)
    let end_pos = after_marker.find("\n\"\"\"")?;
    let traceback_content = &after_marker[..end_pos];

    Some((code, traceback_content.to_string()))
}

/// Parses the ref-counts format: {'var': count, 'var2': count2}
///
/// Supports both single and double quotes for variable names.
/// Example: {'x': 2, 'y': 1} or {"x": 2, "y": 1}
fn parse_ref_counts(s: &str) -> AHashMap<String, usize> {
    let mut counts = AHashMap::new();
    let trimmed = s.trim().trim_start_matches('{').trim_end_matches('}');
    for pair in trimmed.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let parts: Vec<&str> = pair.split(':').collect();
        assert!(
            parts.len() == 2,
            "Invalid ref-counts pair format: {pair}. Expected 'name': count"
        );
        let name = parts[0].trim().trim_matches('\'').trim_matches('"');
        let count: usize = parts[1]
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("Invalid ref count value: {}", parts[1]));
        counts.insert(name.to_string(), count);
    }
    counts
}

/// Result from dispatching an external function call.
///
/// Distinguishes between synchronous calls (return immediately) and
/// asynchronous calls (return a future that needs later resolution).
enum DispatchResult {
    /// Synchronous result - pass directly to `state.run()`.
    Sync(ExtFunctionResult),
    /// Asynchronous call - use `state.run_pending()` and resolve later.
    /// Contains the value to resolve the future with.
    Async(MontyObject),
}

/// Dispatches an external function call to the appropriate test implementation.
///
/// Returns `DispatchResult::Sync` for synchronous calls or `DispatchResult::Async`
/// for coroutine calls that should use `run_pending()`.
///
/// # Panics
/// Panics if the function name is unknown or arguments are invalid types.
fn dispatch_external_call(name: &str, args: Vec<MontyObject>) -> DispatchResult {
    match name {
        "add_ints" => {
            assert!(args.len() == 2, "add_ints requires 2 arguments");
            let a = i64::try_from(&args[0]).expect("add_ints: first arg must be int");
            let b = i64::try_from(&args[1]).expect("add_ints: second arg must be int");
            DispatchResult::Sync(MontyObject::Int(a + b).into())
        }
        "concat_strings" => {
            assert!(args.len() == 2, "concat_strings requires 2 arguments");
            let a = String::try_from(&args[0]).expect("concat_strings: first arg must be str");
            let b = String::try_from(&args[1]).expect("concat_strings: second arg must be str");
            DispatchResult::Sync(MontyObject::String(a + &b).into())
        }
        "return_value" => {
            assert!(args.len() == 1, "return_value requires 1 argument");
            DispatchResult::Sync(args.into_iter().next().unwrap().into())
        }
        "get_list" => {
            assert!(args.is_empty(), "get_list requires no arguments");
            DispatchResult::Sync(
                MontyObject::List(vec![MontyObject::Int(1), MontyObject::Int(2), MontyObject::Int(3)]).into(),
            )
        }
        "raise_error" => {
            // raise_error(exc_type: str, message: str) -> raises exception
            assert!(args.len() == 2, "raise_error requires 2 arguments");
            let exc_type_str = String::try_from(&args[0]).expect("raise_error: first arg must be str");
            let message = String::try_from(&args[1]).expect("raise_error: second arg must be str");
            let exc_type = match exc_type_str.as_str() {
                "ValueError" => ExcType::ValueError,
                "TypeError" => ExcType::TypeError,
                "KeyError" => ExcType::KeyError,
                "RuntimeError" => ExcType::RuntimeError,
                _ => panic!("raise_error: unsupported exception type: {exc_type_str}"),
            };
            DispatchResult::Sync(MontyException::new(exc_type, Some(message)).into())
        }
        "make_point" => {
            assert!(args.is_empty(), "make_point requires no arguments");
            // Return an immutable Point(x=1, y=2) dataclass
            DispatchResult::Sync(
                MontyObject::Dataclass {
                    name: "Point".to_string(),
                    type_id: 0, // Test fixture has no real Python type
                    field_names: vec!["x".to_string(), "y".to_string()],
                    attrs: vec![
                        (MontyObject::String("x".to_string()), MontyObject::Int(1)),
                        (MontyObject::String("y".to_string()), MontyObject::Int(2)),
                    ]
                    .into(),

                    frozen: true,
                }
                .into(),
            )
        }
        "make_mutable_point" => {
            assert!(args.is_empty(), "make_mutable_point requires no arguments");
            // Return a mutable Point(x=1, y=2) dataclass
            DispatchResult::Sync(
                MontyObject::Dataclass {
                    name: "MutablePoint".to_string(),
                    type_id: 0, // Test fixture has no real Python type
                    field_names: vec!["x".to_string(), "y".to_string()],
                    attrs: vec![
                        (MontyObject::String("x".to_string()), MontyObject::Int(1)),
                        (MontyObject::String("y".to_string()), MontyObject::Int(2)),
                    ]
                    .into(),

                    frozen: false,
                }
                .into(),
            )
        }
        "make_user" => {
            assert!(args.len() == 1, "make_user requires 1 argument");
            let name = String::try_from(&args[0]).expect("make_user: first arg must be str");
            // Return an immutable User(name=name, active=True) dataclass
            DispatchResult::Sync(
                MontyObject::Dataclass {
                    name: "User".to_string(),
                    type_id: 0, // Test fixture has no real Python type
                    field_names: vec!["name".to_string(), "active".to_string()],
                    attrs: vec![
                        (MontyObject::String("name".to_string()), MontyObject::String(name)),
                        (MontyObject::String("active".to_string()), MontyObject::Bool(true)),
                    ]
                    .into(),

                    frozen: true,
                }
                .into(),
            )
        }
        "make_empty" => {
            assert!(args.is_empty(), "make_empty requires no arguments");
            // Return an immutable empty dataclass with no fields
            DispatchResult::Sync(
                MontyObject::Dataclass {
                    name: "Empty".to_string(),
                    type_id: 0, // Test fixture has no real Python type
                    field_names: vec![],
                    attrs: vec![].into(),

                    frozen: true,
                }
                .into(),
            )
        }
        "async_call" => {
            // async_call(x) -> coroutine that returns x
            // This is an async function - use run_pending() and resolve later
            assert!(args.len() == 1, "async_call requires 1 argument");
            DispatchResult::Async(args.into_iter().next().unwrap())
        }
        _ => panic!("Unknown external function: {name}"),
    }
}

/// Dispatches a dataclass method call to the appropriate test implementation.
///
/// The first argument is always the dataclass instance (`self`). Known methods
/// are implemented to mirror the dataclass methods used by the fixture suite.
/// Unknown methods return `AttributeError`.
fn dispatch_method_call(
    method_name: &str,
    args: &[MontyObject],
    kwargs: &[(MontyObject, MontyObject)],
) -> ExtFunctionResult {
    let class_name = match args.first() {
        Some(MontyObject::Dataclass { name, .. }) => name.as_str(),
        _ => "<unknown>",
    };

    match (class_name, method_name) {
        // Point.sum(self) -> int
        ("Point" | "MutablePoint", "sum") => {
            let (x, y) = extract_point_fields(&args[0]);
            MontyObject::Int(x + y).into()
        }
        // Point.add(self, dx, dy) -> Point
        ("Point", "add") => {
            assert!(args.len() == 3, "Point.add requires self, dx, dy");
            let (x, y) = extract_point_fields(&args[0]);
            let dx = i64::try_from(&args[1]).expect("dx must be int");
            let dy = i64::try_from(&args[2]).expect("dy must be int");
            MontyObject::Dataclass {
                name: "Point".to_string(),
                type_id: 0,
                field_names: vec!["x".to_string(), "y".to_string()],
                attrs: vec![
                    (MontyObject::String("x".to_string()), MontyObject::Int(x + dx)),
                    (MontyObject::String("y".to_string()), MontyObject::Int(y + dy)),
                ]
                .into(),
                frozen: true,
            }
            .into()
        }
        // Point.scale(self, factor) -> Point
        ("Point", "scale") => {
            assert!(args.len() == 2, "Point.scale requires self, factor");
            let (x, y) = extract_point_fields(&args[0]);
            let factor = i64::try_from(&args[1]).expect("factor must be int");
            MontyObject::Dataclass {
                name: "Point".to_string(),
                type_id: 0,
                field_names: vec!["x".to_string(), "y".to_string()],
                attrs: vec![
                    (MontyObject::String("x".to_string()), MontyObject::Int(x * factor)),
                    (MontyObject::String("y".to_string()), MontyObject::Int(y * factor)),
                ]
                .into(),
                frozen: true,
            }
            .into()
        }
        // Point.describe(self, label='point') -> str
        ("Point", "describe") => {
            let (x, y) = extract_point_fields(&args[0]);
            // Check positional arg first, then kwargs, then default
            let label = if args.len() > 1 {
                String::try_from(&args[1]).expect("label must be str")
            } else if let Some(kw_label) = get_kwarg_str(kwargs, "label") {
                kw_label
            } else {
                "point".to_string()
            };
            MontyObject::String(format!("{label}({x}, {y})")).into()
        }
        // MutablePoint.shift(self, dx, dy) -> None (mutates in-place via host)
        // Note: In the test runner, we can't actually mutate the dataclass in-place
        // since the host doesn't have direct heap access. Return None as the method
        // would in Python (the mutation happens inside Python's method body).
        // For test coverage purposes, we just return None.
        ("MutablePoint", "shift") => MontyObject::None.into(),
        // User.greeting(self) -> str
        ("User", "greeting") => {
            let name = extract_user_name(&args[0]);
            MontyObject::String(format!("Hello, {name}!")).into()
        }
        // Unknown method — return AttributeError
        _ => {
            let message = format!("'{class_name}' object has no attribute '{method_name}'");
            MontyException::new(ExcType::AttributeError, Some(message)).into()
        }
    }
}

/// Extracts (x, y) fields from a Point or MutablePoint `MontyObject::Dataclass`.
fn extract_point_fields(obj: &MontyObject) -> (i64, i64) {
    match obj {
        MontyObject::Dataclass { attrs, .. } => {
            let mut x = 0i64;
            let mut y = 0i64;
            for (key, value) in attrs {
                if let MontyObject::String(k) = key {
                    match k.as_str() {
                        "x" => x = i64::try_from(value).expect("x must be int"),
                        "y" => y = i64::try_from(value).expect("y must be int"),
                        _ => {}
                    }
                }
            }
            (x, y)
        }
        other => panic!("Expected Dataclass, got {other:?}"),
    }
}

/// Extracts a string kwarg value by key name.
fn get_kwarg_str(kwargs: &[(MontyObject, MontyObject)], name: &str) -> Option<String> {
    for (key, value) in kwargs {
        if let MontyObject::String(key_str) = key
            && key_str == name
        {
            return Some(String::try_from(value).expect("kwarg value must be str"));
        }
    }
    None
}

/// Extracts the `name` field from a User `MontyObject::Dataclass`.
fn extract_user_name(obj: &MontyObject) -> String {
    match obj {
        MontyObject::Dataclass { attrs, .. } => {
            for (key, value) in attrs {
                if let MontyObject::String(k) = key
                    && k == "name"
                {
                    return String::try_from(value).expect("name must be str");
                }
            }
            panic!("User dataclass has no 'name' field");
        }
        other => panic!("Expected Dataclass, got {other:?}"),
    }
}

// =============================================================================
// Virtual Filesystem for OS Call Tests
// =============================================================================

/// Virtual file entry for OS call tests (static VFS).
struct StaticVirtualFile {
    content: &'static [u8],
    mode: i64,
}

/// Virtual file entry (owned, for unified VFS lookups).
struct VirtualFile {
    content: Vec<u8>,
    mode: i64,
}

/// Virtual filesystem modification time (arbitrary fixed timestamp).
const VFS_MTIME: f64 = 1_700_000_000.0;

/// Virtual filesystem for testing Path methods.
///
/// Structure:
/// ```text
/// /virtual/
/// ├── file.txt           (file, 644, "hello world\n")
/// ├── data.bin           (file, 644, b"\x00\x01\x02\x03")
/// ├── empty.txt          (file, 644, "")
/// ├── subdir/
/// │   ├── nested.txt     (file, 644, "nested content")
/// │   └── deep/
/// │       └── file.txt   (file, 644, "deep")
/// └── readonly.txt       (file, 444, "readonly")
///
/// /nonexistent           (does not exist)
/// ```
fn get_static_virtual_file(path: &str) -> Option<StaticVirtualFile> {
    match path {
        "/virtual/file.txt" => Some(StaticVirtualFile {
            content: b"hello world\n",
            mode: 0o644,
        }),
        "/virtual/data.bin" => Some(StaticVirtualFile {
            content: b"\x00\x01\x02\x03",
            mode: 0o644,
        }),
        "/virtual/empty.txt" => Some(StaticVirtualFile {
            content: b"",
            mode: 0o644,
        }),
        "/virtual/subdir/nested.txt" => Some(StaticVirtualFile {
            content: b"nested content",
            mode: 0o644,
        }),
        "/virtual/subdir/deep/file.txt" => Some(StaticVirtualFile {
            content: b"deep",
            mode: 0o644,
        }),
        "/virtual/readonly.txt" => Some(StaticVirtualFile {
            content: b"readonly",
            mode: 0o444,
        }),
        _ => None,
    }
}

/// Gets a virtual file, checking the mutable layer first, then falling back to static.
fn get_virtual_file(path: &str) -> Option<VirtualFile> {
    // Check mutable layer first
    let mutable_result = MUTABLE_VFS.with(|vfs| {
        let vfs = vfs.borrow();
        // Check if deleted
        if vfs.deleted_files.contains(path) {
            return Some(None);
        }
        // Check if exists in mutable layer
        if let Some((content, mode)) = vfs.files.get(path) {
            return Some(Some(VirtualFile {
                content: content.clone(),
                mode: *mode,
            }));
        }
        None
    });

    match mutable_result {
        Some(Some(file)) => Some(file),
        Some(None) => None, // File was deleted
        None => {
            // Fall back to static VFS
            get_static_virtual_file(path).map(|f| VirtualFile {
                content: f.content.to_vec(),
                mode: f.mode,
            })
        }
    }
}

// =============================================================================
// Mutable VFS Layer (Thread-Local Storage for Write Operations)
// =============================================================================

/// Mutable state for the virtual filesystem, supporting write operations.
///
/// This layer sits on top of the static VFS and allows tests to create, modify, and
/// delete files and directories. The state is thread-local so tests don't interfere
/// with each other.
#[derive(Default)]
struct MutableVfs {
    /// Files created or modified during test execution.
    files: HashMap<String, (Vec<u8>, i64)>, // path -> (content, mode)
    /// Directories created during test execution.
    dirs: HashSet<String>,
    /// Files deleted during test execution (shadows static VFS entries).
    deleted_files: HashSet<String>,
    /// Directories deleted during test execution.
    deleted_dirs: HashSet<String>,
}

thread_local! {
    /// Thread-local mutable VFS state.
    static MUTABLE_VFS: RefCell<MutableVfs> = RefCell::new(MutableVfs::default());
}

/// Resets the mutable VFS state for a new test.
fn reset_mutable_vfs() {
    MUTABLE_VFS.with(|vfs| {
        *vfs.borrow_mut() = MutableVfs::default();
    });
}

/// Check if the given path is a directory in the virtual filesystem.
fn is_virtual_dir(path: &str) -> bool {
    // Check mutable layer first
    let result = MUTABLE_VFS.with(|vfs| {
        let vfs = vfs.borrow();
        if vfs.deleted_dirs.contains(path) {
            return Some(false);
        }
        if vfs.dirs.contains(path) {
            return Some(true);
        }
        None
    });
    if let Some(is_dir) = result {
        return is_dir;
    }
    // Fall back to static VFS
    matches!(path, "/virtual" | "/virtual/subdir" | "/virtual/subdir/deep")
}

/// Get directory entries for a virtual directory.
fn get_virtual_dir_entries(path: &str) -> Option<Vec<String>> {
    // First check if the directory exists
    if !is_virtual_dir(path) {
        return None;
    }

    // Get static entries (if any)
    let static_entries: Vec<&'static str> = match path {
        "/virtual" => vec![
            "/virtual/file.txt",
            "/virtual/data.bin",
            "/virtual/empty.txt",
            "/virtual/subdir",
            "/virtual/readonly.txt",
        ],
        "/virtual/subdir" => vec!["/virtual/subdir/nested.txt", "/virtual/subdir/deep"],
        "/virtual/subdir/deep" => vec!["/virtual/subdir/deep/file.txt"],
        _ => vec![],
    };

    // Combine with mutable layer
    MUTABLE_VFS.with(|vfs| {
        let vfs = vfs.borrow();
        let mut entries: HashSet<String> = static_entries
            .iter()
            .filter(|e| {
                let s: &str = e;
                !vfs.deleted_files.contains(s) && !vfs.deleted_dirs.contains(s)
            })
            .map(|e| (*e).to_owned())
            .collect();

        // Add mutable files and dirs in this directory
        let prefix = if path.ends_with('/') {
            path.to_owned()
        } else {
            format!("{path}/")
        };
        for file_path in vfs.files.keys() {
            if file_path.starts_with(&prefix) {
                // Only include direct children (not nested)
                let rest = &file_path[prefix.len()..];
                if !rest.contains('/') {
                    entries.insert(file_path.clone());
                }
            }
        }
        for dir_path in &vfs.dirs {
            if dir_path.starts_with(&prefix) {
                let rest = &dir_path[prefix.len()..];
                if !rest.contains('/') {
                    entries.insert(dir_path.clone());
                }
            }
        }

        Some(entries.into_iter().collect())
    })
}

/// Helper to get a boolean kwarg by name.
fn get_kwarg_bool(kwargs: &[(MontyObject, MontyObject)], name: &str) -> bool {
    for (key, value) in kwargs {
        if let MontyObject::String(key_str) = key
            && key_str == name
        {
            return matches!(value, MontyObject::Bool(true));
        }
    }
    false
}

/// Dispatches an OS function call using the virtual filesystem.
///
/// Returns an `ExternalResult` to pass back to the Monty interpreter.
/// Raises `FileNotFoundError` for missing files/directories.
#[expect(clippy::cast_possible_wrap)] // Virtual file sizes are tiny, no wrap possible
fn dispatch_os_call(
    function: OsFunction,
    args: &[MontyObject],
    kwargs: &[(MontyObject, MontyObject)],
) -> ExtFunctionResult {
    // Handle GetEnviron first as it takes no path argument
    if function == OsFunction::GetEnviron {
        // Return the virtual environment as a dict
        let env_dict = vec![
            (
                MontyObject::String("VIRTUAL_HOME".to_owned()),
                MontyObject::String("/virtual/home".to_owned()),
            ),
            (
                MontyObject::String("VIRTUAL_USER".to_owned()),
                MontyObject::String("testuser".to_owned()),
            ),
            (
                MontyObject::String("VIRTUAL_EMPTY".to_owned()),
                MontyObject::String(String::new()),
            ),
        ];
        return MontyObject::Dict(env_dict.into()).into();
    }

    // Extract path from MontyObject::Path (or String for backwards compatibility)
    let path = match &args[0] {
        MontyObject::Path(p) => p.clone(),
        MontyObject::String(s) => s.clone(),
        other => panic!("OS call: first arg must be path, got {other:?}"),
    };

    match function {
        OsFunction::GetEnviron => unreachable!("handled above"),
        OsFunction::Exists => {
            let exists = get_virtual_file(&path).is_some() || is_virtual_dir(&path);
            MontyObject::Bool(exists).into()
        }
        OsFunction::IsFile => {
            let is_file = get_virtual_file(&path).is_some();
            MontyObject::Bool(is_file).into()
        }
        OsFunction::IsDir => {
            let is_dir = is_virtual_dir(&path);
            MontyObject::Bool(is_dir).into()
        }
        OsFunction::IsSymlink => {
            // Virtual filesystem doesn't have symlinks
            MontyObject::Bool(false).into()
        }
        OsFunction::ReadText => {
            if let Some(file) = get_virtual_file(&path) {
                match std::str::from_utf8(&file.content) {
                    Ok(text) => MontyObject::String(text.to_owned()).into(),
                    Err(_) => MontyException::new(
                        ExcType::UnicodeDecodeError,
                        Some("'utf-8' codec can't decode bytes".to_owned()),
                    )
                    .into(),
                }
            } else {
                MontyException::new(
                    ExcType::FileNotFoundError,
                    Some(format!("[Errno 2] No such file or directory: '{path}'")),
                )
                .into()
            }
        }
        OsFunction::ReadBytes => {
            if let Some(file) = get_virtual_file(&path) {
                MontyObject::Bytes(file.content).into()
            } else {
                MontyException::new(
                    ExcType::FileNotFoundError,
                    Some(format!("[Errno 2] No such file or directory: '{path}'")),
                )
                .into()
            }
        }
        OsFunction::Stat => {
            if let Some(file) = get_virtual_file(&path) {
                file_stat(file.mode, file.content.len() as i64, VFS_MTIME).into()
            } else if is_virtual_dir(&path) {
                dir_stat(0o755, VFS_MTIME).into()
            } else {
                MontyException::new(
                    ExcType::FileNotFoundError,
                    Some(format!("[Errno 2] No such file or directory: '{path}'")),
                )
                .into()
            }
        }
        OsFunction::Iterdir => {
            if let Some(entries) = get_virtual_dir_entries(&path) {
                // Return Path objects, not strings
                let list: Vec<MontyObject> = entries.into_iter().map(MontyObject::Path).collect();
                MontyObject::List(list).into()
            } else {
                MontyException::new(
                    ExcType::FileNotFoundError,
                    Some(format!("[Errno 2] No such file or directory: '{path}'")),
                )
                .into()
            }
        }
        OsFunction::Resolve | OsFunction::Absolute => {
            // For virtual paths, return as-is (they're already absolute)
            MontyObject::String(path).into()
        }
        OsFunction::Getenv => {
            // Virtual environment for testing os.getenv()
            // args[0] is key, args[1] is default (may be None)
            let key = String::try_from(&args[0]).expect("getenv: first arg must be key string");
            let default = &args[1];

            // Provide a few test environment variables
            let value = match key.as_str() {
                "VIRTUAL_HOME" => Some("/virtual/home"),
                "VIRTUAL_USER" => Some("testuser"),
                "VIRTUAL_EMPTY" => Some(""),
                _ => None,
            };

            if let Some(v) = value {
                MontyObject::String(v.to_owned()).into()
            } else if matches!(default, MontyObject::None) {
                MontyObject::None.into()
            } else {
                // Return the default value
                default.clone().into()
            }
        }
        OsFunction::WriteText => {
            // args[0] is path, args[1] is text content
            let text = String::try_from(&args[1]).expect("write_text: second arg must be string");
            MUTABLE_VFS.with(|vfs| {
                let mut vfs = vfs.borrow_mut();
                vfs.files.insert(path.clone(), (text.into_bytes(), 0o644));
                vfs.deleted_files.remove(&path);
            });
            // write_text returns the number of bytes written
            let byte_count = MUTABLE_VFS.with(|vfs| vfs.borrow().files.get(&path).map_or(0, |(c, _)| c.len()));
            MontyObject::Int(byte_count as i64).into()
        }
        OsFunction::WriteBytes => {
            // args[0] is path, args[1] is bytes content
            let bytes = match &args[1] {
                MontyObject::Bytes(b) => b.clone(),
                other => panic!("write_bytes: second arg must be bytes, got {other:?}"),
            };
            let byte_count = bytes.len();
            MUTABLE_VFS.with(|vfs| {
                let mut vfs = vfs.borrow_mut();
                vfs.files.insert(path.clone(), (bytes, 0o644));
                vfs.deleted_files.remove(&path);
            });
            // write_bytes returns the number of bytes written
            MontyObject::Int(byte_count as i64).into()
        }
        OsFunction::Mkdir => {
            // Check for parents and exist_ok in kwargs (e.g., mkdir(parents=True, exist_ok=True))
            let parents = get_kwarg_bool(kwargs, "parents");
            let exist_ok = get_kwarg_bool(kwargs, "exist_ok");

            // Check if already exists
            if is_virtual_dir(&path) {
                if exist_ok {
                    return MontyObject::None.into();
                }
                return MontyException::new(ExcType::OSError, Some(format!("[Errno 17] File exists: '{path}'"))).into();
            }

            // Check parent directory
            let parent = std::path::Path::new(&path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            if !parent.is_empty() && !is_virtual_dir(&parent) {
                if parents {
                    // Create parent directories recursively
                    create_parent_dirs(&parent);
                } else {
                    return MontyException::new(
                        ExcType::FileNotFoundError,
                        Some(format!("[Errno 2] No such file or directory: '{path}'")),
                    )
                    .into();
                }
            }

            MUTABLE_VFS.with(|vfs| {
                let mut vfs = vfs.borrow_mut();
                vfs.deleted_dirs.remove(&path);
                vfs.dirs.insert(path);
            });
            MontyObject::None.into()
        }
        OsFunction::Unlink => {
            // args[0] is path
            if get_virtual_file(&path).is_some() {
                MUTABLE_VFS.with(|vfs| {
                    let mut vfs = vfs.borrow_mut();
                    vfs.files.remove(&path);
                    vfs.deleted_files.insert(path);
                });
                MontyObject::None.into()
            } else {
                MontyException::new(
                    ExcType::FileNotFoundError,
                    Some(format!("[Errno 2] No such file or directory: '{path}'")),
                )
                .into()
            }
        }
        OsFunction::Rmdir => {
            // args[0] is path
            if is_virtual_dir(&path) {
                MUTABLE_VFS.with(|vfs| {
                    let mut vfs = vfs.borrow_mut();
                    vfs.dirs.remove(&path);
                    vfs.deleted_dirs.insert(path);
                });
                MontyObject::None.into()
            } else {
                MontyException::new(
                    ExcType::FileNotFoundError,
                    Some(format!("[Errno 2] No such file or directory: '{path}'")),
                )
                .into()
            }
        }
        OsFunction::Rename => {
            // args[0] is src path, args[1] is dest path
            let dest = match &args[1] {
                MontyObject::Path(p) => p.clone(),
                MontyObject::String(s) => s.clone(),
                other => panic!("rename: second arg must be path, got {other:?}"),
            };

            if let Some(file) = get_virtual_file(&path) {
                MUTABLE_VFS.with(|vfs| {
                    let mut vfs = vfs.borrow_mut();
                    // Remove from old location
                    vfs.files.remove(&path);
                    vfs.deleted_files.insert(path);
                    // Add to new location
                    vfs.files.insert(dest, (file.content, file.mode));
                });
                MontyObject::None.into()
            } else if is_virtual_dir(&path) {
                MUTABLE_VFS.with(|vfs| {
                    let mut vfs = vfs.borrow_mut();
                    vfs.dirs.remove(&path);
                    vfs.deleted_dirs.insert(path);
                    vfs.dirs.insert(dest);
                });
                MontyObject::None.into()
            } else {
                MontyException::new(
                    ExcType::FileNotFoundError,
                    Some(format!("[Errno 2] No such file or directory: '{path}'")),
                )
                .into()
            }
        }
    }
}

/// Helper to create parent directories recursively.
fn create_parent_dirs(path: &str) {
    if is_virtual_dir(path) {
        return;
    }
    // Create parent first
    if let Some(parent) = std::path::Path::new(path).parent() {
        let parent_str = parent.to_string_lossy().to_string();
        if !parent_str.is_empty() {
            create_parent_dirs(&parent_str);
        }
    }
    // Create this directory
    MUTABLE_VFS.with(|vfs| {
        let mut vfs = vfs.borrow_mut();
        vfs.dirs.insert(path.to_owned());
    });
}

/// Represents a test failure with details about expected vs actual values.
#[derive(Debug)]
struct TestFailure {
    test_name: String,
    kind: String,
    expected: String,
    actual: String,
}

impl std::fmt::Display for TestFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "[{}] {} mismatch\ngot {:?}\ndiff:",
            self.test_name, self.kind, self.actual
        )?;

        for change in TextDiff::from_lines(&self.expected, &self.actual).iter_all_changes() {
            write!(f, "{}{}", change.tag(), change)?;
        }
        Ok(())
    }
}

/// Try to run a test, returning Ok(()) on success or Err with failure details.
///
/// This function executes Python code via the MontyRun and validates the result
/// against the expected outcome specified in the fixture.
fn try_run_test(path: &Path, code: &str, expectation: &Expectation) -> Result<(), TestFailure> {
    let test_name = path.strip_prefix("test_cases/").unwrap_or(path).display().to_string();

    // Reset the mutable VFS for each test
    reset_mutable_vfs();

    // Handle ref-count-return tests separately since they need run_ref_counts()
    #[cfg(feature = "ref-count-return")]
    if let Expectation::RefCounts(expected) = expectation {
        match MontyRun::new(code.to_owned(), &test_name, vec![]) {
            Ok(ex) => {
                let result = ex.run_ref_counts(vec![]);
                match result {
                    Ok(monty::RefCountOutput {
                        counts,
                        unique_refs,
                        heap_count,
                        ..
                    }) => {
                        // Strict matching: verify all heap objects are accounted for by variables
                        if unique_refs != heap_count {
                            return Err(TestFailure {
                                test_name,
                                kind: "Strict matching".to_string(),
                                expected: format!("{heap_count} heap objects"),
                                actual: format!("{unique_refs} referenced by variables, counts: {counts:?}"),
                            });
                        }
                        if &counts != expected {
                            return Err(TestFailure {
                                test_name,
                                kind: "ref-counts".to_string(),
                                expected: format!("{expected:?}"),
                                actual: format!("{counts:?}"),
                            });
                        }
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(TestFailure {
                            test_name,
                            kind: "Runtime".to_string(),
                            expected: "success".to_string(),
                            actual: e.to_string(),
                        });
                    }
                }
            }
            Err(parse_err) => {
                return Err(TestFailure {
                    test_name,
                    kind: "Parse".to_string(),
                    expected: "success".to_string(),
                    actual: parse_err.to_string(),
                });
            }
        }
    }

    match MontyRun::new(code.to_owned(), &test_name, vec![]) {
        Ok(ex) => {
            let limits = ResourceLimits::new().max_recursion_depth(Some(TEST_RECURSION_LIMIT));
            let result = ex.run(vec![], LimitedTracker::new(limits), &mut PrintWriter::Stdout);
            match result {
                Ok(obj) => match expectation {
                    Expectation::ReturnStr(expected) => {
                        let output = obj.to_string();
                        if output != *expected {
                            return Err(TestFailure {
                                test_name,
                                kind: "str()".to_string(),
                                expected: expected.clone(),
                                actual: output,
                            });
                        }
                    }
                    Expectation::Return(expected) => {
                        let output = obj.py_repr();
                        if output != *expected {
                            return Err(TestFailure {
                                test_name,
                                kind: "py_repr()".to_string(),
                                expected: expected.clone(),
                                actual: output,
                            });
                        }
                    }
                    Expectation::ReturnType(expected) => {
                        let output = obj.type_name();
                        if output != expected {
                            return Err(TestFailure {
                                test_name,
                                kind: "type_name()".to_string(),
                                expected: expected.clone(),
                                actual: output.to_string(),
                            });
                        }
                    }
                    #[cfg(not(feature = "ref-count-return"))]
                    Expectation::RefCounts(_) => {
                        // Skip ref-count tests when feature is disabled
                    }
                    Expectation::NoException => {
                        // Success - code ran without exception as expected
                    }
                    Expectation::Raise(expected) | Expectation::Traceback(expected) => {
                        return Err(TestFailure {
                            test_name,
                            kind: "Exception".to_string(),
                            expected: expected.clone(),
                            actual: "no exception raised".to_string(),
                        });
                    }
                    #[cfg(feature = "ref-count-return")]
                    Expectation::RefCounts(_) => unreachable!(),
                },
                Err(e) => {
                    if let Expectation::Raise(expected) = expectation {
                        let output = e.py_repr();
                        if output != *expected {
                            return Err(TestFailure {
                                test_name,
                                kind: "Exception".to_string(),
                                expected: expected.clone(),
                                actual: output,
                            });
                        }
                    } else if let Expectation::Traceback(expected) = expectation {
                        let output = e.to_string();
                        if output != *expected {
                            return Err(TestFailure {
                                test_name,
                                kind: "Traceback".to_string(),
                                expected: expected.clone(),
                                actual: output,
                            });
                        }
                    } else {
                        return Err(TestFailure {
                            test_name,
                            kind: "Unexpected error".to_string(),
                            expected: "success".to_string(),
                            actual: e.to_string(),
                        });
                    }
                }
            }
        }
        Err(parse_err) => {
            if let Expectation::Raise(expected) = expectation {
                let output = parse_err.py_repr();
                if output != *expected {
                    return Err(TestFailure {
                        test_name,
                        kind: "Parse error".to_string(),
                        expected: expected.clone(),
                        actual: output,
                    });
                }
            } else if let Expectation::Traceback(expected) = expectation {
                let output = parse_err.to_string();
                if output != *expected {
                    return Err(TestFailure {
                        test_name,
                        kind: "Traceback".to_string(),
                        expected: expected.clone(),
                        actual: output,
                    });
                }
            } else {
                return Err(TestFailure {
                    test_name,
                    kind: "Unexpected parse error".to_string(),
                    expected: "success".to_string(),
                    actual: parse_err.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// Try to run a test using MontyRun with external function support.
///
/// This function handles tests marked with `# call-external` directive by using the
/// iterative executor API and providing implementations for predefined external functions.
fn try_run_iter_test(path: &Path, code: &str, expectation: &Expectation) -> Result<(), TestFailure> {
    let test_name = path.strip_prefix("test_cases/").unwrap_or(path).display().to_string();

    // Reset the mutable VFS for each test
    reset_mutable_vfs();

    // Ref-counting tests not supported in iter mode
    #[cfg(feature = "ref-count-return")]
    if matches!(expectation, Expectation::RefCounts(_)) {
        return Err(TestFailure {
            test_name,
            kind: "Configuration".to_string(),
            expected: "non-refcount test".to_string(),
            actual: "ref-counts tests are not supported in iter mode".to_string(),
        });
    }

    let exec = match MontyRun::new(code.to_owned(), &test_name, vec![]) {
        Ok(e) => e,
        Err(parse_err) => {
            if let Expectation::Raise(expected) = expectation {
                let output = parse_err.py_repr();
                if output != *expected {
                    return Err(TestFailure {
                        test_name,
                        kind: "Parse error".to_string(),
                        expected: expected.clone(),
                        actual: output,
                    });
                }
                return Ok(());
            } else if let Expectation::Traceback(expected) = expectation {
                let output = parse_err.to_string();
                if output != *expected {
                    return Err(TestFailure {
                        test_name,
                        kind: "Traceback".to_string(),
                        expected: expected.clone(),
                        actual: output,
                    });
                }
                return Ok(());
            }
            return Err(TestFailure {
                test_name,
                kind: "Unexpected parse error".to_string(),
                expected: "success".to_string(),
                actual: parse_err.to_string(),
            });
        }
    };

    // Run execution loop, handling external function calls until complete
    let result = run_iter_loop(exec);

    match result {
        Ok(obj) => match expectation {
            Expectation::ReturnStr(expected) => {
                let output = obj.to_string();
                if output != *expected {
                    return Err(TestFailure {
                        test_name,
                        kind: "str()".to_string(),
                        expected: expected.clone(),
                        actual: output,
                    });
                }
            }
            Expectation::Return(expected) => {
                let output = obj.py_repr();
                if output != *expected {
                    return Err(TestFailure {
                        test_name,
                        kind: "py_repr()".to_string(),
                        expected: expected.clone(),
                        actual: output,
                    });
                }
            }
            Expectation::ReturnType(expected) => {
                let output = obj.type_name();
                if output != expected {
                    return Err(TestFailure {
                        test_name,
                        kind: "type_name()".to_string(),
                        expected: expected.clone(),
                        actual: output.to_string(),
                    });
                }
            }
            #[cfg(not(feature = "ref-count-return"))]
            Expectation::RefCounts(_) => {}
            Expectation::NoException => {}
            Expectation::Raise(expected) | Expectation::Traceback(expected) => {
                return Err(TestFailure {
                    test_name,
                    kind: "Exception".to_string(),
                    expected: expected.clone(),
                    actual: "no exception raised".to_string(),
                });
            }
            #[cfg(feature = "ref-count-return")]
            Expectation::RefCounts(_) => unreachable!(),
        },
        Err(e) => {
            if let Expectation::Raise(expected) = expectation {
                let output = e.py_repr();
                if output != *expected {
                    return Err(TestFailure {
                        test_name,
                        kind: "Exception".to_string(),
                        expected: expected.clone(),
                        actual: output,
                    });
                }
            } else if let Expectation::Traceback(expected) = expectation {
                let output = e.to_string();
                if output != *expected {
                    return Err(TestFailure {
                        test_name,
                        kind: "Traceback".to_string(),
                        expected: expected.clone(),
                        actual: output,
                    });
                }
            } else {
                return Err(TestFailure {
                    test_name,
                    kind: "Unexpected error".to_string(),
                    expected: "success".to_string(),
                    actual: e.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// Execute the iter loop, dispatching external function calls until complete.
///
/// When `ref-count-panic` feature is NOT enabled, this function also tests
/// serialization round-trips by dumping and loading the execution state at
/// each external function call boundary.
///
/// Supports both synchronous and asynchronous external functions:
/// - Sync functions: result is passed immediately via `state.run()`
/// - Async functions: `state.run_pending()` creates a future, resolved via `ResolveFutures`
fn run_iter_loop(exec: MontyRun) -> Result<MontyObject, MontyException> {
    let limits = ResourceLimits::new().max_recursion_depth(Some(TEST_RECURSION_LIMIT));
    let mut progress = exec.start(vec![], LimitedTracker::new(limits), &mut PrintWriter::Stdout)?;

    // Track pending async calls: (call_id, result_value)
    let mut pending_results: Vec<(u32, MontyObject)> = Vec::new();

    loop {
        // Test serialization round-trip at each step (skip when ref-count-panic is enabled
        // since the old RunProgress would panic on drop without proper cleanup)
        #[cfg(not(feature = "ref-count-panic"))]
        {
            let bytes = progress.dump().expect("failed to dump RunProgress");
            progress = RunProgress::load(&bytes).expect("failed to load RunProgress");
        }

        match progress {
            RunProgress::Complete(result) => return Ok(result),
            RunProgress::FunctionCall(call) => {
                // Method calls on dataclasses are dispatched to the host.
                // Dispatch known methods; return AttributeError for unknown ones.
                if call.method_call {
                    let result = dispatch_method_call(&call.function_name, &call.args, &call.kwargs);
                    progress = call.resume(result, &mut PrintWriter::Stdout)?;
                    continue;
                }
                let dispatch_result = dispatch_external_call(&call.function_name, call.args.clone());
                match dispatch_result {
                    DispatchResult::Sync(return_value) => {
                        progress = call.resume(return_value, &mut PrintWriter::Stdout)?;
                    }
                    DispatchResult::Async(result_value) => {
                        // Store the result for later resolution
                        pending_results.push((call.call_id, result_value));
                        // Continue execution with a pending future
                        progress = call.resume_pending(&mut PrintWriter::Stdout)?;
                    }
                }
            }
            RunProgress::ResolveFutures(state) => {
                // Resolve all pending futures that we have results for
                let results: Vec<(u32, ExtFunctionResult)> = state
                    .pending_call_ids()
                    .iter()
                    .filter_map(|p| {
                        pending_results.iter().position(|(id, _)| id == p).map(|idx| {
                            let (call_id, value) = pending_results.remove(idx);
                            (call_id, ExtFunctionResult::Return(value))
                        })
                    })
                    .collect();

                assert!(
                    !results.is_empty(),
                    "ResolveFutures: no results available for pending calls: {:?}",
                    state.pending_call_ids().iter().collect::<Vec<_>>()
                );

                progress = state.resume(results, &mut PrintWriter::Stdout)?;
            }
            RunProgress::NameLookup(lookup) => {
                let result = match lookup.name.as_str() {
                    // External functions — resolved as callable Function objects
                    "add_ints" | "concat_strings" | "return_value" | "get_list" | "raise_error" | "make_point"
                    | "make_mutable_point" | "make_user" | "make_empty" | "async_call" => {
                        NameLookupResult::Value(MontyObject::Function {
                            name: lookup.name.clone(),
                            docstring: None,
                        })
                    }
                    // Non-function constants — resolved as plain values
                    "CONST_INT" => NameLookupResult::Value(MontyObject::Int(42)),
                    "CONST_STR" => NameLookupResult::Value(MontyObject::String("hello".to_string())),
                    #[expect(clippy::approx_constant, reason = "3.14 is the intended test value")]
                    "CONST_FLOAT" => NameLookupResult::Value(MontyObject::Float(3.14)),
                    "CONST_BOOL" => NameLookupResult::Value(MontyObject::Bool(true)),
                    "CONST_LIST" => NameLookupResult::Value(MontyObject::List(vec![
                        MontyObject::Int(1),
                        MontyObject::Int(2),
                        MontyObject::Int(3),
                    ])),
                    "CONST_NONE" => NameLookupResult::Value(MontyObject::None),
                    // Unknown names → NameError
                    _ => NameLookupResult::Undefined,
                };
                progress = lookup.resume(result, &mut PrintWriter::Stdout)?;
            }
            RunProgress::OsCall(call) => {
                let result = dispatch_os_call(call.function, &call.args, &call.kwargs);
                progress = call.resume(result, &mut PrintWriter::Stdout)?;
            }
        }
    }
}

/// Timeout duration for Monty tests.
///
/// Tests that exceed this duration are considered to be hanging (infinite loop)
/// and will fail with a timeout error.
const TEST_TIMEOUT: Duration = Duration::from_secs(2);

/// Result from running a test with a timeout.
enum TimeoutResult<T> {
    /// The closure completed successfully.
    Ok(T),
    /// The closure panicked with the given message.
    Panicked(String),
    /// The timeout was exceeded.
    TimedOut,
}

/// Runs a closure with a timeout, returning an error if it exceeds the duration or panics.
///
/// Spawns the closure in a separate thread and waits for the result with a timeout.
/// Distinguishes between three cases:
/// - Success: the closure returned normally
/// - Panic: the closure panicked (detected via channel disconnect + catch_unwind)
/// - Timeout: the timeout was exceeded (possible infinite loop)
///
/// Note that if a timeout occurs, the spawned thread will continue running in the
/// background (Rust doesn't support killing threads), but the test will fail immediately.
fn run_with_timeout<F, T>(timeout: Duration, f: F) -> TimeoutResult<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        // Catch panics so we can report them properly instead of as timeouts
        let result = panic::catch_unwind(AssertUnwindSafe(f));
        match result {
            Ok(value) => {
                let _ = tx.send(Ok(value));
            }
            Err(panic_payload) => {
                // Extract panic message from the payload
                let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    (*s).to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                let _ = tx.send(Err(msg));
            }
        }
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(value)) => TimeoutResult::Ok(value),
        Ok(Err(panic_msg)) => TimeoutResult::Panicked(panic_msg),
        Err(RecvTimeoutError::Timeout) => TimeoutResult::TimedOut,
        // Disconnected without sending means something went very wrong
        Err(RecvTimeoutError::Disconnected) => {
            TimeoutResult::Panicked("thread terminated without sending result".to_string())
        }
    }
}

/// Test function that runs each fixture through Monty.
///
/// Handles xfail with strict semantics: if a test is marked `xfail=monty`, it must fail.
/// If an xfail test passes unexpectedly, that's an error.
fn run_test_cases_monty(path: &Path) -> Result<(), Box<dyn Error>> {
    let content = fs::read_to_string(path)?;
    let (code, expectation, config) = parse_fixture(&content);
    let test_name = path.strip_prefix("test_cases/").unwrap_or(path).display().to_string();

    // Move data into the closure since it needs 'static lifetime
    let path_owned = path.to_owned();
    let iter_mode = config.iter_mode;

    let result = run_with_timeout(TEST_TIMEOUT, move || {
        if iter_mode {
            try_run_iter_test(&path_owned, &code, &expectation)
        } else {
            try_run_test(&path_owned, &code, &expectation)
        }
    });

    // Handle timeout/panic errors from the test thread
    let result = match result {
        TimeoutResult::Ok(inner_result) => inner_result,
        TimeoutResult::Panicked(panic_msg) => Err(TestFailure {
            test_name: test_name.clone(),
            kind: "Panic".to_string(),
            expected: "no panic".to_string(),
            actual: format!("test panicked: {panic_msg}"),
        }),
        TimeoutResult::TimedOut => Err(TestFailure {
            test_name: test_name.clone(),
            kind: "Timeout".to_string(),
            expected: format!("completion within {TEST_TIMEOUT:?}"),
            actual: format!("test timed out after {TEST_TIMEOUT:?} (possible infinite loop)"),
        }),
    };

    if config.xfail_monty {
        // Strict xfail: test must fail; if it passed, xfail should be removed
        assert!(
            result.is_err(),
            "[{test_name}] Test marked xfail=monty passed unexpectedly. Remove xfail if the test is now fixed."
        );
    } else if let Err(failure) = result {
        panic!("{failure}");
    }
    Ok(())
}

// Generate tests for all fixture files using datatest-stable harness macro
datatest_stable::harness!(run_test_cases_monty, "test_cases", r"^.*\.monty$",);
