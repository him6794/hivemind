use managed_function_transpiler::{SourceLanguage, TranspileError, transpile_cpp, transpile_python};

#[test]
fn translates_python_function_with_assignments_conditionals_and_return() {
    let source = r"
def score(x, limit):
    total = x + 1
    if total > limit:
        return limit
    else:
        return total
";

    let managed = transpile_python(source).expect("python subset should transpile");

    assert_eq!(
        managed,
        "fn score(x, limit) {\n    let total = x + 1;\n    return if total > limit { limit } else { total };\n}"
    );
}

#[test]
fn translates_cpp_function_with_local_declarations_conditionals_and_return() {
    let source = r"
int score(int x, int limit) {
    int total = x + 1;
    if (total > limit) {
        return limit;
    } else {
        return total;
    }
}
";

    let managed = transpile_cpp(source).expect("c++ subset should transpile");

    assert_eq!(
        managed,
        "fn score(x, limit) {\n    let total = x + 1;\n    return if total > limit { limit } else { total };\n}"
    );
}

#[test]
fn rejects_python_constructs_outside_initial_subset() {
    let err = transpile_python(
        r"
def score(values):
    for value in values:
        return value
",
    )
    .expect_err("loops are intentionally unsupported by the converter subset");

    assert_eq!(err.language(), SourceLanguage::Python);
    assert!(matches!(err, TranspileError::Unsupported { .. }));
    assert!(err.to_string().contains("for"));
}

#[test]
fn rejects_cpp_constructs_outside_initial_subset() {
    let err = transpile_cpp(
        r"
int score(int x) {
    while (x > 0) {
        x = x - 1;
    }
    return x;
}
",
    )
    .expect_err("loops are intentionally unsupported by the converter subset");

    assert_eq!(err.language(), SourceLanguage::Cpp);
    assert!(matches!(err, TranspileError::Unsupported { .. }));
    assert!(err.to_string().contains("while"));
}
