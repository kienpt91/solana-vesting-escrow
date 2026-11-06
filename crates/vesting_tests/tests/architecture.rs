//! Mechanical architecture rules. They read the manifests, the lock file and the sources, so a change that breaks a
//! rule fails the build instead of waiting for a review:
//!
//! * `vesting_core` is pure: no dependencies at all, no Solana or SPL names, no unsafe code.
//! * The program does no money arithmetic of its own and reads time only from the clock sysvar.
//! * The crates depend on each other in one direction only.
//! * Test keypairs are generated at runtime and never written into sources.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// The dependency names of a manifest: the keys of `[dependencies]`, `[dev-dependencies]` and
/// `[build-dependencies]` (including target-specific tables).
fn dependency_names(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_deps = line.contains("dependencies") && !line.contains("workspace.");
            // `[dependencies.foo]` style tables name the dependency in the header
            if let Some(rest) = line.strip_prefix("[dependencies.") {
                names.push(rest.trim_end_matches(']').to_string());
                in_deps = false;
            }
            continue;
        }
        if in_deps && !line.is_empty() && !line.starts_with('#') {
            if let Some((key, _)) = line.split_once('=') {
                names.push(key.trim().to_string());
            }
        }
    }
    names
}

fn crate_manifest(name: &str) -> String {
    let dir = match name {
        "vesting_escrow" => "programs/vesting_escrow",
        other => return read(&root().join("crates").join(other).join("Cargo.toml")),
    };
    read(&root().join(dir).join("Cargo.toml"))
}

#[test]
fn the_pure_crate_has_no_dependencies_at_all() {
    let manifest = crate_manifest("vesting_core");
    assert_eq!(dependency_names(&manifest), Vec::<String>::new(), "vesting_core must not depend on anything");
    // the lock file agrees: the package entry lists no dependencies
    let lock = read(&root().join("Cargo.lock"));
    let block = lock.split("[[package]]").find(|b| b.contains("name = \"vesting_core\"")).expect("vesting_core is in the lock file");
    assert!(!block.contains("dependencies"), "vesting_core has dependencies in Cargo.lock:\n{block}");
}

#[test]
fn the_pure_crate_names_no_solana_type_and_forbids_unsafe_code() {
    let forbidden = ["solana", "spl_token", "spl-token", "Pubkey", "AccountInfo", "ProgramError", "sysvar", "Clock"];
    let files = rust_files(&root().join("crates/vesting_core/src"));
    assert!(files.len() >= 4, "the sources of vesting_core were not found");
    for file in &files {
        let text = read(file);
        for word in forbidden {
            assert!(!text.contains(word), "{} mentions `{word}`: the domain crate must stay free of Solana types", file.display());
        }
        assert!(!text.contains("unsafe "), "{} contains unsafe code", file.display());
    }
    let lib = read(&root().join("crates/vesting_core/src/lib.rs"));
    assert!(lib.contains("#![forbid(unsafe_code)]"), "vesting_core must forbid unsafe code");
}

#[test]
fn the_program_uses_the_pure_crate_and_does_no_money_arithmetic_of_its_own() {
    let manifest = crate_manifest("vesting_escrow");
    assert!(dependency_names(&manifest).contains(&"vesting_core".to_string()), "the program must depend on vesting_core");
    let processor = read(&root().join("programs/vesting_escrow/src/processor.rs"));
    assert!(processor.contains("vesting_core::Schedule"), "the processor must take its amounts from vesting_core");

    // wide integers and the operations that implement the schedule maths live in vesting_core only
    let banned = ["u128", "i128", "checked_mul", "checked_div", "saturating_mul", "wrapping_", "overflowing_", "as u128", "pow("];
    for file in rust_files(&root().join("programs/vesting_escrow/src")) {
        let text = read(&file);
        for word in banned {
            assert!(!text.contains(word), "{} contains `{word}`: schedule arithmetic belongs in vesting_core", file.display());
        }
    }
    // and the workspace lint that rejects unchecked arithmetic is on for the program
    let workspace = read(&root().join("Cargo.toml"));
    assert!(workspace.contains("arithmetic_side_effects = \"deny\""));
    assert!(manifest.contains("[lints]") && manifest.contains("workspace = true"), "the program must inherit the workspace lints");
}

#[test]
fn time_comes_from_the_clock_sysvar_only() {
    let dir = root().join("programs/vesting_escrow/src");
    let mut uses = 0;
    for file in rust_files(&dir) {
        let text = read(&file);
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        uses += text.matches("Clock::get()").count();
        // no other way of reading a clock, in any file
        for banned in ["Clock::from_account_info", "sysvar::clock", "SystemTime", "Instant::now"] {
            assert!(!text.contains(banned), "{name} reads time through `{banned}`");
        }
    }
    assert_eq!(uses, 1, "the clock must be read in exactly one place");
    // no instruction carries "now" or a clock
    let instruction = read(&dir.join("instruction.rs"));
    assert!(!instruction.contains("Clock") && !instruction.contains("unix_timestamp") && !instruction.contains("now:"));
}

#[test]
fn unsafe_code_exists_only_in_the_entrypoint() {
    for file in rust_files(&root().join("programs/vesting_escrow/src")) {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        let text = read(&file);
        let uses_unsafe =
            text.lines().filter(|l| !l.trim_start().starts_with("//")).any(|l| l.contains("unsafe ") && !l.contains("unsafe_code"));
        assert!(!uses_unsafe, "{name} contains unsafe code");
    }
}

#[test]
fn crates_depend_on_each_other_in_one_direction() {
    // allowed workspace dependencies per crate
    let allowed: [(&str, &[&str]); 4] = [
        ("vesting_core", &[]),
        ("vesting_escrow", &["vesting_core"]),
        ("vesting_client", &["vesting_core", "vesting_escrow"]),
        ("vesting_tests", &["vesting_core", "vesting_escrow", "vesting_client"]),
    ];
    for (name, allowed) in allowed {
        for dep in dependency_names(&crate_manifest(name)).into_iter().filter(|d| d.starts_with("vesting_")) {
            assert!(allowed.contains(&dep.as_str()), "{name} must not depend on {dep}");
        }
    }
}

#[test]
fn no_keypair_is_written_into_a_source_file() {
    for dir in ["crates", "programs"] {
        for file in rust_files(&root().join(dir)) {
            if file.ends_with("tests/architecture.rs") {
                continue;
            }
            let text = read(&file);
            for banned in ["Keypair::from_bytes", "from_base58_string", "keypair_from_seed", "read_keypair"] {
                assert!(
                    !text.contains(banned),
                    "{} builds a keypair from stored material (`{banned}`): test keys must be generated at runtime",
                    file.display()
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// documentation that names tests must name tests that exist
// ---------------------------------------------------------------------------------------------------------

/// The backticked words of a markdown line.
fn backticked(line: &str) -> Vec<String> {
    line.split('`').skip(1).step_by(2).map(str::to_string).collect()
}

/// Every test name in the "Test" column of the tables of the given documents.
fn documented_tests(doc: &Path) -> Vec<String> {
    let text = read(doc);
    let lines: Vec<&str> = text.lines().collect();
    let mut names = Vec::new();
    let mut i = 0;
    while i + 1 < lines.len() {
        let is_table_head = lines[i].starts_with('|') && lines[i + 1].starts_with("| ---");
        if is_table_head && lines[i].contains("Test") {
            let mut row = i + 2;
            while row < lines.len() && lines[row].starts_with('|') {
                if let Some(cell) = lines[row].trim_end_matches('|').rsplit('|').next() {
                    names.extend(backticked(cell));
                }
                row += 1;
            }
            i = row;
        } else {
            i += 1;
        }
    }
    names
}

#[test]
fn every_test_named_in_the_documentation_exists() {
    let sources: Vec<(PathBuf, String)> = ["crates", "programs"]
        .iter()
        .flat_map(|d| rust_files(&root().join(d)))
        .map(|p| {
            let text = read(&p);
            (p, text)
        })
        .collect();
    let docs = ["README.md", "SECURITY.md", "docs/threat-model.md"];
    let mut checked = 0;
    for doc in docs {
        for token in documented_tests(&root().join(doc)) {
            // `file.rs::test_name`, `path/to/file.rs::test_name`, `file.rs`, `path/to/file.rs` or `test_name`
            let (file, name) = match token.split_once("::") {
                Some((file, name)) => (Some(file.to_string()), Some(name.to_string())),
                None if token.ends_with(".rs") => (Some(token.clone()), None),
                None => (None, Some(token.clone())),
            };
            if let Some(file) = &file {
                let found = sources.iter().any(|(p, _)| p.ends_with(file) || p.to_string_lossy().ends_with(file.as_str()));
                assert!(found, "{doc} names the file `{file}`, which does not exist");
            }
            if let Some(name) = &name {
                let needle = format!("fn {name}(");
                let found = sources
                    .iter()
                    .any(|(p, text)| text.contains(&needle) && file.as_ref().is_none_or(|f| p.to_string_lossy().ends_with(f.as_str())));
                assert!(
                    found,
                    "{doc} names the test `{name}`, which does not exist{}",
                    file.map(|f| format!(" in {f}")).unwrap_or_default()
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 80, "the documents should name many tests; found only {checked}");
}

// ---------------------------------------------------------------------------------------------------------
// what can be checked about SBF without the SBF toolchain (docs/sbf-checklist.md lists what cannot)
// ---------------------------------------------------------------------------------------------------------

/// The code of a source file without comments and without everything after the first `#[cfg(test)]`.
fn code_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        let code = line.split("//").next().unwrap_or("").to_string();
        if !code.trim().is_empty() {
            out.push((i + 1, code));
        }
    }
    out
}

const PROGRAM_LOGIC_FILES: [&str; 5] = ["processor.rs", "state.rs", "validation.rs", "token.rs", "codec.rs"];

#[test]
fn the_account_handling_code_does_not_allocate() {
    // SBF gives a program a 32 KiB bump heap that is never freed: an allocation per instruction is a budget to account for.
    // The instruction handlers, the state codec, the validation layer and the token helpers allocate nothing (the client-side
    // `VestingInstruction::pack` allocates and lives in instruction.rs, which no handler calls).
    let banned = [
        "vec!",
        "Vec<",
        "Vec::",
        "Box::",
        "Box<",
        "String",
        "format!",
        ".to_string()",
        ".to_owned()",
        ".collect(",
        "to_vec()",
        "HashMap",
        "BTreeMap",
    ];
    for name in PROGRAM_LOGIC_FILES {
        let path = root().join("programs/vesting_escrow/src").join(name);
        for (line, code) in code_lines(&read(&path)) {
            for word in banned {
                assert!(!code.contains(word), "{name}:{line} allocates (`{word}`): {}", code.trim());
            }
            if code.contains("msg!(") {
                assert!(
                    !code.contains('{') || code.contains("msg!(\"") && !code.contains("{}"),
                    "{name}:{line}: msg! with formatting arguments costs compute units and heap: {}",
                    code.trim()
                );
            }
        }
    }
}

#[test]
fn no_stack_array_is_larger_than_a_small_bound() {
    // SBF stack frames are 4 KiB per function. A large array on the stack is the usual way to blow one. The program has no
    // array with a numeric length above this bound (the largest value is the 272 byte schedule record, which is a struct).
    const BOUND: u64 = 128;
    for file in rust_files(&root().join("programs/vesting_escrow/src")) {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        for (line, code) in code_lines(&read(&file)) {
            let mut rest = code.as_str();
            while let Some(at) = rest.find(';') {
                let after = &rest[at + 1..];
                if let Some(end) = after.find(']') {
                    let length = after[..end].trim();
                    if let Ok(n) = length.parse::<u64>() {
                        assert!(
                            n <= BOUND,
                            "{name}:{line}: an array of {n} elements; keep stack arrays at {BOUND} or below: {}",
                            code.trim()
                        );
                    }
                }
                rest = after;
            }
        }
    }
}

#[test]
fn the_crate_type_lets_cargo_build_sbf_link_the_program() {
    // `cargo build-sbf` needs a `cdylib` to produce the shared object; the tests and the client need the `lib`
    let manifest = crate_manifest("vesting_escrow");
    let compact: String = manifest.split_whitespace().collect();
    assert!(
        compact.contains("[lib]crate-type=[\"cdylib\",\"lib\"]"),
        "programs/vesting_escrow/Cargo.toml must declare crate-type cdylib and lib"
    );
}

#[test]
fn every_cpi_signs_with_seeds_built_from_the_stored_state() {
    // CPI signer seeds come from one place, built from the stored creator, mint, id and bump: no handler assembles its own
    let processor = read(&root().join("programs/vesting_escrow/src/processor.rs"));
    assert!(processor.contains("SignerSeeds::new(") && processor.contains("seeds.slices()"));
    let token = read(&root().join("programs/vesting_escrow/src/token.rs"));
    assert_eq!(token.matches("invoke_signed(").count(), 2, "exactly the vault transfer and the vault close sign with seeds");
    assert_eq!(token.matches("invoke(").count(), 1, "the funding transfer is signed by the creator's own signature");
    assert_eq!(processor.matches("find_program_address").count(), 0, "the processor derives through pda.rs only");
}

// ---------------------------------------------------------------------------------------------------------
// the number of hostile transactions is counted, not written by hand
// ---------------------------------------------------------------------------------------------------------

/// The number in "N hostile transactions" at the end of the recorded demo output, after checking that it agrees with the rows.
fn recorded_hostile_transactions() -> usize {
    let output = read(&root().join("docs/demo-output.txt"));
    let rows = output
        .lines()
        .filter(|l| l.split_whitespace().nth(1) == Some("PASS") && l.split_whitespace().next().is_some_and(|n| n.parse::<usize>().is_ok()))
        .count();
    let summary = output
        .lines()
        .find(|l| l.contains(" hostile transactions, "))
        .unwrap_or_else(|| panic!("docs/demo-output.txt has no summary line"));
    let words: Vec<&str> = summary.split_whitespace().collect();
    let total: usize = words[0].parse().unwrap();
    let rejected: usize = words[3].parse().unwrap();
    let not_rejected: usize = words[words.len() - 2].parse().unwrap();
    assert_eq!(total, rows, "the summary of docs/demo-output.txt counts {total} transactions but the file has {rows} rows");
    assert_eq!(rejected, total, "every recorded transaction must be rejected exactly as expected");
    assert_eq!(not_rejected, 0);
    // row numbers are 1..=N in order
    let numbers: Vec<usize> = output
        .lines()
        .filter(|l| l.split_whitespace().nth(1) == Some("PASS"))
        .filter_map(|l| l.split_whitespace().next()?.parse().ok())
        .collect();
    assert_eq!(numbers, (1..=total).collect::<Vec<_>>(), "the rows of the recorded output are not numbered 1..N");
    total
}

#[test]
fn the_documented_number_of_hostile_transactions_is_the_recorded_one() {
    let total = recorded_hostile_transactions();
    // every attack test sends at least one hostile transaction
    let attacks = read(&root().join("crates/vesting_tests/tests/attacks.rs"));
    let tests = attacks.matches("#[tokio::test]").count();
    assert!(total >= tests, "{total} recorded transactions for {tests} attack tests: the recording undercounts");
    for doc in ["README.md", "SECURITY.md", "docs/threat-model.md", "CHANGELOG.md"] {
        let text = read(&root().join(doc));
        for (at, _) in text.match_indices(" hostile transactions") {
            let digits: String = text[..at].chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
            if let Ok(n) = digits.parse::<usize>() {
                assert_eq!(n, total, "{doc} says {n} hostile transactions; docs/demo-output.txt records {total}");
            }
        }
    }
    let readme = read(&root().join("README.md"));
    assert!(
        readme.contains(&format!("{total} hostile transactions")),
        "the README must state the recorded number of hostile transactions ({total})"
    );
    assert!(
        read(&root().join("SECURITY.md")).contains(&format!("{total} hostile transactions")),
        "SECURITY.md must state the recorded number ({total})"
    );
}
