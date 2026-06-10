use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn rust_imports_stay_at_top_level_import_blocks() {
    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("monty crate has crates parent");
    let mut files = Vec::new();
    collect_rs_files(crates_dir, &mut files);

    let mut errors = Vec::new();
    for file in files {
        errors.extend(check_file(&file));
    }

    assert!(
        errors.is_empty(),
        "found `use` statements outside the top-level import block:\n{}",
        errors.join("\n")
    );
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read crate directory") {
        let path = entry.expect("read directory entry").path();
        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

fn check_file(path: &Path) -> Vec<String> {
    let content = fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let mut errors = Vec::new();
    let mut past_imports = false;
    let mut brace_depth = 0usize;

    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        let stripped = line.trim();

        if brace_depth == 0 {
            if !past_imports && is_allowed_import_block_line(stripped) {
                update_brace_depth(stripped, &mut brace_depth);
                continue;
            }

            if !past_imports && !stripped.is_empty() {
                past_imports = true;
            }

            if past_imports && is_top_level_use(stripped) {
                errors.push(format!("{}:{line_number}: {stripped}", path.display()));
            }
        }

        update_brace_depth(stripped, &mut brace_depth);
    }

    errors
}

fn is_allowed_import_block_line(stripped: &str) -> bool {
    stripped.is_empty()
        || stripped.starts_with("//")
        || stripped.starts_with("/*")
        || stripped.starts_with('*')
        || stripped.starts_with("#[")
        || stripped.starts_with("#![")
        || stripped == "}"
        || is_top_level_use(stripped)
        || is_module_declaration(stripped)
}

fn is_top_level_use(stripped: &str) -> bool {
    stripped.starts_with("use ")
        || stripped.starts_with("pub use ")
        || (stripped.starts_with("pub(") && stripped.contains(") use "))
}

fn is_module_declaration(stripped: &str) -> bool {
    let declaration = stripped
        .split_once("//")
        .map_or(stripped, |(code, _comment)| code.trim());
    let declaration = declaration.strip_prefix("pub ").unwrap_or(declaration);
    declaration.starts_with("mod ") && declaration.ends_with(';')
}

fn update_brace_depth(stripped: &str, brace_depth: &mut usize) {
    *brace_depth += stripped.matches('{').count();
    *brace_depth = brace_depth.saturating_sub(stripped.matches('}').count());
}
