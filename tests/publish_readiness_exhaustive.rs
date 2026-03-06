#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use toml::Value;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn parse_toml(path: &Path) -> Value {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
    content
        .parse::<Value>()
        .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()))
}

fn root_cargo_toml() -> Value {
    parse_toml(&workspace_root().join("Cargo.toml"))
}

fn workspace_package(root: &Value) -> &toml::value::Table {
    root.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.as_table())
        .expect("workspace.package should be a table")
}

fn workspace_members_list(root: &Value) -> Vec<String> {
    root.get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .expect("workspace.members should be an array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn workspace_deps_table(root: &Value) -> &toml::value::Table {
    root.get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|d| d.as_table())
        .expect("workspace.dependencies should be a table")
}

struct CrateInfo {
    name: String,
    member_path: String,
    cargo_toml_path: PathBuf,
    toml: Value,
}

impl CrateInfo {
    fn package(&self) -> &toml::value::Table {
        self.toml
            .get("package")
            .and_then(|p| p.as_table())
            .expect("package should be a table")
    }

    fn deps_table(&self, section: &str) -> Option<&toml::value::Table> {
        self.toml.get(section).and_then(|d| d.as_table())
    }
}

fn all_workspace_crates() -> Vec<CrateInfo> {
    let root = root_cargo_toml();
    let members = workspace_members_list(&root);
    let ws_root = workspace_root();

    members
        .into_iter()
        .map(|member| {
            let cargo_toml_path = ws_root.join(&member).join("Cargo.toml");
            let toml = parse_toml(&cargo_toml_path);
            let name = toml
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();
            CrateInfo {
                name,
                member_path: member,
                cargo_toml_path,
                toml,
            }
        })
        .collect()
}

/// Crates from `crates/` directory only (excludes xtask, root).
fn publishable_crates() -> Vec<CrateInfo> {
    all_workspace_crates()
        .into_iter()
        .filter(|c| c.member_path.starts_with("crates/"))
        .collect()
}

/// Check if a package field uses `field.workspace = true`.
fn is_workspace_inherited(pkg: &toml::value::Table, field: &str) -> bool {
    pkg.get(field)
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("workspace"))
        .and_then(|w| w.as_bool())
        .unwrap_or(false)
}

/// Check if a package field has a direct string value.
fn has_string_value(pkg: &toml::value::Table, field: &str) -> bool {
    pkg.get(field).and_then(|v| v.as_str()).is_some()
}

/// Get the version string from a workspace dependency.
fn dep_version(dep: &Value) -> Option<&str> {
    match dep {
        Value::String(s) => Some(s.as_str()),
        Value::Table(t) => t.get("version").and_then(|v| v.as_str()),
        _ => None,
    }
}

/// Check if a dependency references a path.
fn dep_has_path(dep: &Value) -> bool {
    match dep {
        Value::Table(t) => t.get("path").is_some(),
        _ => false,
    }
}

/// Check if a dependency uses workspace = true.
fn dep_is_workspace(dep: &Value) -> bool {
    match dep {
        Value::Table(t) => t
            .get("workspace")
            .and_then(|w| w.as_bool())
            .unwrap_or(false),
        _ => false,
    }
}

/// All crate names in the workspace.
fn all_crate_names() -> HashSet<String> {
    all_workspace_crates()
        .iter()
        .map(|c| c.name.clone())
        .collect()
}

// ===========================================================================
// Group 1: Workspace structure
// ===========================================================================

#[test]
fn test_workspace_has_resolver() {
    let root = root_cargo_toml();
    let resolver = root
        .get("workspace")
        .and_then(|w| w.get("resolver"))
        .and_then(|r| r.as_str());
    assert!(resolver.is_some(), "workspace should declare a resolver");
}

#[test]
fn test_workspace_resolver_is_two() {
    let root = root_cargo_toml();
    let resolver = root["workspace"]["resolver"].as_str().unwrap();
    assert_eq!(resolver, "2", "workspace resolver should be 2");
}

#[test]
fn test_workspace_has_members() {
    let root = root_cargo_toml();
    let members = workspace_members_list(&root);
    assert!(!members.is_empty(), "workspace should have members");
}

#[test]
fn test_workspace_member_count() {
    let root = root_cargo_toml();
    let members = workspace_members_list(&root);
    assert!(
        members.len() >= 47,
        "workspace should have at least 47 members, found {}",
        members.len()
    );
}

#[test]
fn test_workspace_members_match_crates_dir() {
    let root = root_cargo_toml();
    let members = workspace_members_list(&root);
    let crate_members: HashSet<String> = members
        .iter()
        .filter(|m| m.starts_with("crates/"))
        .map(|m| m.strip_prefix("crates/").unwrap().to_string())
        .collect();

    let ws_root = workspace_root();
    let crates_dir = ws_root.join("crates");
    let on_disk: HashSet<String> = fs::read_dir(&crates_dir)
        .expect("crates/ should be readable")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|ft| ft.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    let missing_from_workspace: Vec<_> = on_disk.difference(&crate_members).collect();
    let missing_from_disk: Vec<_> = crate_members.difference(&on_disk).collect();

    assert!(
        missing_from_workspace.is_empty(),
        "Directories in crates/ not listed as workspace members: {missing_from_workspace:?}"
    );
    assert!(
        missing_from_disk.is_empty(),
        "Workspace members not found in crates/: {missing_from_disk:?}"
    );
}

#[test]
fn test_no_orphan_workspace_members() {
    let root = root_cargo_toml();
    let ws_root = workspace_root();
    for member in workspace_members_list(&root) {
        let dir = ws_root.join(&member);
        assert!(dir.is_dir(), "workspace member {member} directory missing");
    }
}

#[test]
fn test_xtask_is_workspace_member() {
    let root = root_cargo_toml();
    let members = workspace_members_list(&root);
    assert!(
        members.contains(&"xtask".to_string()),
        "xtask should be a workspace member"
    );
}

#[test]
fn test_all_member_cargo_tomls_exist() {
    let root = root_cargo_toml();
    let ws_root = workspace_root();
    for member in workspace_members_list(&root) {
        let toml_path = ws_root.join(&member).join("Cargo.toml");
        assert!(
            toml_path.is_file(),
            "Cargo.toml missing for member {member}"
        );
    }
}

#[test]
fn test_workspace_excludes_fuzz() {
    let root = root_cargo_toml();
    let exclude = root
        .get("workspace")
        .and_then(|w| w.get("exclude"))
        .and_then(|e| e.as_array())
        .expect("workspace.exclude should be an array");
    let excluded: Vec<&str> = exclude.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        excluded.contains(&"fuzz"),
        "workspace should exclude fuzz directory"
    );
}

// ===========================================================================
// Group 2: Workspace package metadata
// ===========================================================================

#[test]
fn test_workspace_package_has_version() {
    let root = root_cargo_toml();
    let pkg = workspace_package(&root);
    assert!(
        has_string_value(pkg, "version"),
        "workspace.package should have a version"
    );
}

#[test]
fn test_workspace_version_is_semver() {
    let root = root_cargo_toml();
    let pkg = workspace_package(&root);
    let version = pkg["version"].as_str().unwrap();
    let parts: Vec<&str> = version.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "version should be semver (major.minor.patch)"
    );
    for part in &parts {
        assert!(
            part.parse::<u32>().is_ok(),
            "version component '{part}' should be numeric"
        );
    }
}

#[test]
fn test_workspace_package_has_edition() {
    let root = root_cargo_toml();
    let pkg = workspace_package(&root);
    assert!(
        has_string_value(pkg, "edition"),
        "workspace.package should have an edition"
    );
}

#[test]
fn test_workspace_edition_is_2024() {
    let root = root_cargo_toml();
    let pkg = workspace_package(&root);
    assert_eq!(pkg["edition"].as_str().unwrap(), "2024");
}

#[test]
fn test_workspace_package_has_rust_version() {
    let root = root_cargo_toml();
    let pkg = workspace_package(&root);
    assert!(
        has_string_value(pkg, "rust-version"),
        "workspace.package should declare rust-version"
    );
}

#[test]
fn test_workspace_package_has_license() {
    let root = root_cargo_toml();
    let pkg = workspace_package(&root);
    assert!(
        has_string_value(pkg, "license"),
        "workspace.package should have a license"
    );
}

#[test]
fn test_workspace_license_is_dual() {
    let root = root_cargo_toml();
    let pkg = workspace_package(&root);
    let license = pkg["license"].as_str().unwrap();
    assert_eq!(license, "MIT OR Apache-2.0");
}

#[test]
fn test_workspace_package_has_description() {
    let root = root_cargo_toml();
    let pkg = workspace_package(&root);
    assert!(
        has_string_value(pkg, "description"),
        "workspace.package should have a description"
    );
}

#[test]
fn test_workspace_package_has_authors() {
    let root = root_cargo_toml();
    let pkg = workspace_package(&root);
    let authors = pkg.get("authors").and_then(|a| a.as_array());
    assert!(authors.is_some(), "workspace.package should have authors");
    assert!(
        !authors.unwrap().is_empty(),
        "workspace.package.authors should not be empty"
    );
}

#[test]
fn test_workspace_package_has_repository() {
    let root = root_cargo_toml();
    let pkg = workspace_package(&root);
    assert!(
        has_string_value(pkg, "repository"),
        "workspace.package should have a repository URL"
    );
}

// ===========================================================================
// Group 3: License files
// ===========================================================================

#[test]
fn test_license_mit_exists() {
    let path = workspace_root().join("LICENSE-MIT");
    assert!(path.is_file(), "LICENSE-MIT should exist at repo root");
}

#[test]
fn test_license_apache_exists() {
    let path = workspace_root().join("LICENSE-APACHE");
    assert!(path.is_file(), "LICENSE-APACHE should exist at repo root");
}

#[test]
fn test_license_mit_not_empty() {
    let path = workspace_root().join("LICENSE-MIT");
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.len() > 100,
        "LICENSE-MIT should contain substantive text"
    );
}

#[test]
fn test_license_apache_not_empty() {
    let path = workspace_root().join("LICENSE-APACHE");
    let content = fs::read_to_string(&path).unwrap();
    assert!(
        content.len() > 100,
        "LICENSE-APACHE should contain substantive text"
    );
}

#[test]
fn test_license_string_is_valid_spdx_dual() {
    let root = root_cargo_toml();
    let pkg = workspace_package(&root);
    let license = pkg["license"].as_str().unwrap();
    assert!(
        license.contains("MIT") && license.contains("Apache-2.0"),
        "license should reference both MIT and Apache-2.0"
    );
    assert!(
        license.contains("OR"),
        "dual license should use OR combinator"
    );
}

// ===========================================================================
// Group 4: Per-crate metadata validation
// ===========================================================================

#[test]
fn test_all_crates_have_name() {
    for c in all_workspace_crates() {
        let pkg = c.package();
        assert!(
            has_string_value(pkg, "name"),
            "crate at {} should have a name",
            c.member_path
        );
    }
}

#[test]
fn test_all_crates_have_version_field() {
    for c in all_workspace_crates() {
        let pkg = c.package();
        let has = has_string_value(pkg, "version") || is_workspace_inherited(pkg, "version");
        assert!(
            has,
            "{} should have version or version.workspace = true",
            c.name
        );
    }
}

#[test]
fn test_all_crates_have_license_field() {
    for c in all_workspace_crates() {
        let pkg = c.package();
        let has = has_string_value(pkg, "license") || is_workspace_inherited(pkg, "license");
        assert!(
            has,
            "{} should have license or license.workspace = true",
            c.name
        );
    }
}

#[test]
fn test_all_crates_have_description_field() {
    for c in all_workspace_crates() {
        let pkg = c.package();
        let has =
            has_string_value(pkg, "description") || is_workspace_inherited(pkg, "description");
        assert!(has, "{} should have a description", c.name);
    }
}

#[test]
fn test_all_crates_have_edition_field() {
    for c in all_workspace_crates() {
        let pkg = c.package();
        let has = has_string_value(pkg, "edition") || is_workspace_inherited(pkg, "edition");
        assert!(
            has,
            "{} should have edition or edition.workspace = true",
            c.name
        );
    }
}

#[test]
fn test_all_publishable_crates_use_workspace_version() {
    for c in publishable_crates() {
        let pkg = c.package();
        assert!(
            is_workspace_inherited(pkg, "version"),
            "{} should use version.workspace = true",
            c.name
        );
    }
}

#[test]
fn test_all_publishable_crates_use_workspace_edition() {
    for c in publishable_crates() {
        let pkg = c.package();
        assert!(
            is_workspace_inherited(pkg, "edition"),
            "{} should use edition.workspace = true",
            c.name
        );
    }
}

#[test]
fn test_all_publishable_crates_use_workspace_license() {
    for c in publishable_crates() {
        let pkg = c.package();
        assert!(
            is_workspace_inherited(pkg, "license"),
            "{} should use license.workspace = true",
            c.name
        );
    }
}

#[test]
fn test_all_publishable_crates_use_workspace_authors() {
    for c in publishable_crates() {
        let pkg = c.package();
        assert!(
            is_workspace_inherited(pkg, "authors"),
            "{} should use authors.workspace = true",
            c.name
        );
    }
}

#[test]
fn test_all_publishable_crates_use_workspace_repository() {
    for c in publishable_crates() {
        let pkg = c.package();
        assert!(
            is_workspace_inherited(pkg, "repository"),
            "{} should use repository.workspace = true",
            c.name
        );
    }
}

#[test]
fn test_all_publishable_crates_use_workspace_rust_version() {
    for c in publishable_crates() {
        let pkg = c.package();
        assert!(
            is_workspace_inherited(pkg, "rust-version"),
            "{} should use rust-version.workspace = true",
            c.name
        );
    }
}

#[test]
fn test_all_descriptions_are_nonempty() {
    for c in publishable_crates() {
        let pkg = c.package();
        if let Some(desc) = pkg.get("description").and_then(|d| d.as_str()) {
            assert!(
                !desc.trim().is_empty(),
                "{} has an empty description",
                c.name
            );
            assert!(
                desc.len() >= 10,
                "{} description is too short: '{desc}'",
                c.name
            );
        }
    }
}

#[test]
fn test_all_crate_descriptions_are_unique() {
    let crates = publishable_crates();
    let mut seen: HashMap<String, String> = HashMap::new();
    for c in &crates {
        let pkg = c.package();
        if let Some(desc) = pkg.get("description").and_then(|d| d.as_str()) {
            if let Some(prev) = seen.get(desc) {
                panic!(
                    "duplicate description between {} and {}: '{desc}'",
                    prev, c.name
                );
            }
            seen.insert(desc.to_string(), c.name.clone());
        }
    }
}

#[test]
fn test_all_crate_names_are_lowercase() {
    for c in all_workspace_crates() {
        assert_eq!(
            c.name,
            c.name.to_lowercase(),
            "crate name '{}' should be lowercase",
            c.name
        );
    }
}

#[test]
fn test_all_crate_names_use_kebab_case() {
    for c in all_workspace_crates() {
        assert!(
            !c.name.contains('_'),
            "crate name '{}' should use kebab-case, not underscores",
            c.name
        );
    }
}

#[test]
fn test_no_duplicate_crate_names() {
    let crates = all_workspace_crates();
    let mut seen: HashSet<String> = HashSet::new();
    for c in &crates {
        assert!(
            seen.insert(c.name.clone()),
            "duplicate crate name: {}",
            c.name
        );
    }
}

#[test]
fn test_all_publishable_crates_have_readme() {
    for c in publishable_crates() {
        let pkg = c.package();
        assert!(
            has_string_value(pkg, "readme"),
            "{} should have a readme field",
            c.name
        );
    }
}

#[test]
fn test_all_publishable_crates_have_keywords() {
    for c in publishable_crates() {
        let pkg = c.package();
        let kw = pkg.get("keywords").and_then(|k| k.as_array());
        assert!(
            kw.is_some() && !kw.unwrap().is_empty(),
            "{} should have keywords",
            c.name
        );
    }
}

#[test]
fn test_all_publishable_crates_have_categories() {
    for c in publishable_crates() {
        let pkg = c.package();
        let cats = pkg.get("categories").and_then(|k| k.as_array());
        assert!(
            cats.is_some() && !cats.unwrap().is_empty(),
            "{} should have categories",
            c.name
        );
    }
}

#[test]
fn test_all_publishable_crates_have_src() {
    let ws_root = workspace_root();
    for c in publishable_crates() {
        let src_lib = ws_root
            .join(&c.member_path)
            .join("src")
            .join("lib.rs");
        let src_main = ws_root
            .join(&c.member_path)
            .join("src")
            .join("main.rs");
        assert!(
            src_lib.is_file() || src_main.is_file(),
            "{} should have src/lib.rs or src/main.rs",
            c.name
        );
    }
}

#[test]
fn test_description_length_under_limit() {
    // crates.io limits descriptions to ~1000 chars
    for c in publishable_crates() {
        let pkg = c.package();
        if let Some(desc) = pkg.get("description").and_then(|d| d.as_str()) {
            assert!(
                desc.len() <= 1000,
                "{} description is too long ({} chars)",
                c.name,
                desc.len()
            );
        }
    }
}

// ===========================================================================
// Group 5: Publish flags
// ===========================================================================

#[test]
fn test_root_crate_is_publish_false() {
    let root = root_cargo_toml();
    let publish = root
        .get("package")
        .and_then(|p| p.get("publish"))
        .and_then(|p| p.as_bool());
    assert_eq!(
        publish,
        Some(false),
        "root crate should have publish = false"
    );
}

#[test]
fn test_xtask_is_publish_false() {
    let ws_root = workspace_root();
    let toml = parse_toml(&ws_root.join("xtask").join("Cargo.toml"));
    let publish = toml
        .get("package")
        .and_then(|p| p.get("publish"))
        .and_then(|p| p.as_bool());
    assert_eq!(publish, Some(false), "xtask should have publish = false");
}

#[test]
fn test_no_publishable_crate_has_publish_false() {
    for c in publishable_crates() {
        let pkg = c.package();
        let publish = pkg.get("publish").and_then(|p| p.as_bool());
        assert!(
            publish != Some(false),
            "{} should not have publish = false",
            c.name
        );
    }
}

#[test]
fn test_publishable_crate_count() {
    let count = publishable_crates().len();
    assert!(
        count >= 46,
        "expected at least 46 publishable crates, found {count}"
    );
}

#[test]
fn test_intentional_publish_false_only_on_known_crates() {
    let allowed_publish_false: HashSet<&str> =
        ["agent-backplane", "xtask"].iter().copied().collect();
    for c in all_workspace_crates() {
        let pkg = c.package();
        if let Some(false) = pkg.get("publish").and_then(|p| p.as_bool()) {
            assert!(
                allowed_publish_false.contains(c.name.as_str()),
                "{} has publish = false but is not in the allowed list",
                c.name
            );
        }
    }
}

// ===========================================================================
// Group 6: Documentation
// ===========================================================================

#[test]
fn test_all_crate_libs_warn_missing_docs() {
    let ws_root = workspace_root();
    for c in publishable_crates() {
        let lib_rs = ws_root
            .join(&c.member_path)
            .join("src")
            .join("lib.rs");
        if lib_rs.is_file() {
            let content = fs::read_to_string(&lib_rs).unwrap();
            assert!(
                content.contains("#![warn(missing_docs)]"),
                "{} src/lib.rs should contain #![warn(missing_docs)]",
                c.name
            );
        }
    }
}

#[test]
fn test_root_lib_warns_missing_docs() {
    let lib_rs = workspace_root().join("src").join("lib.rs");
    if lib_rs.is_file() {
        let content = fs::read_to_string(&lib_rs).unwrap();
        assert!(
            content.contains("#![warn(missing_docs)]"),
            "root src/lib.rs should contain #![warn(missing_docs)]"
        );
    }
}

#[test]
fn test_all_crate_libs_exist() {
    let ws_root = workspace_root();
    for c in publishable_crates() {
        let lib_rs = ws_root
            .join(&c.member_path)
            .join("src")
            .join("lib.rs");
        let main_rs = ws_root
            .join(&c.member_path)
            .join("src")
            .join("main.rs");
        assert!(
            lib_rs.is_file() || main_rs.is_file(),
            "{} should have src/lib.rs or src/main.rs",
            c.name
        );
    }
}

// ===========================================================================
// Group 7: Dependency pinning
// ===========================================================================

#[test]
fn test_workspace_deps_have_versions() {
    let root = root_cargo_toml();
    let deps = workspace_deps_table(&root);
    for (name, val) in deps {
        let version = dep_version(val);
        assert!(
            version.is_some(),
            "workspace dep '{name}' should have an explicit version"
        );
    }
}

#[test]
fn test_no_wildcard_versions_in_workspace_deps() {
    let root = root_cargo_toml();
    let deps = workspace_deps_table(&root);
    for (name, val) in deps {
        if let Some(v) = dep_version(val) {
            assert_ne!(
                v, "*",
                "workspace dep '{name}' should not use wildcard version"
            );
        }
    }
}

#[test]
fn test_no_wildcard_versions_in_crate_deps() {
    for c in all_workspace_crates() {
        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(deps) = c.deps_table(section) {
                for (dep_name, val) in deps {
                    if let Some(v) = dep_version(val) {
                        assert_ne!(
                            v, "*",
                            "{}: dep '{dep_name}' should not use wildcard version",
                            c.name
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn test_internal_path_deps_use_path_and_version() {
    let names = all_crate_names();
    for c in publishable_crates() {
        if let Some(deps) = c.deps_table("dependencies") {
            for (dep_name, val) in deps {
                if dep_has_path(val) && names.contains(dep_name.as_str()) {
                    let has_version = dep_version(val).is_some();
                    assert!(
                        has_version,
                        "{}: internal path dep '{dep_name}' should also have a version for crates.io",
                        c.name
                    );
                }
            }
        }
    }
}

#[test]
fn test_internal_dep_paths_exist() {
    let ws_root = workspace_root();
    for c in all_workspace_crates() {
        for section in ["dependencies", "dev-dependencies"] {
            if let Some(deps) = c.deps_table(section) {
                for (dep_name, val) in deps {
                    if let Value::Table(t) = val {
                        if let Some(path_val) = t.get("path").and_then(|p| p.as_str()) {
                            let base = ws_root.join(&c.member_path);
                            let dep_dir = base.join(path_val);
                            assert!(
                                dep_dir.is_dir(),
                                "{}: path dep '{dep_name}' -> '{path_val}' does not exist",
                                c.name
                            );
                        }
                    }
                }
            }
        }
    }
}

macro_rules! workspace_dep_pinned_test {
    ($test_name:ident, $dep_name:literal) => {
        #[test]
        fn $test_name() {
            let root = root_cargo_toml();
            let deps = workspace_deps_table(&root);
            let dep = deps
                .get($dep_name)
                .unwrap_or_else(|| panic!("workspace should have '{}' as a dependency", $dep_name));
            let version = dep_version(dep)
                .unwrap_or_else(|| panic!("workspace dep '{}' should have a version", $dep_name));
            assert!(
                !version.is_empty(),
                "workspace dep '{}' version should not be empty",
                $dep_name
            );
            assert_ne!(
                version, "*",
                "workspace dep '{}' should not use wildcard",
                $dep_name
            );
        }
    };
}

workspace_dep_pinned_test!(test_serde_version_pinned, "serde");
workspace_dep_pinned_test!(test_tokio_version_pinned, "tokio");
workspace_dep_pinned_test!(test_anyhow_version_pinned, "anyhow");
workspace_dep_pinned_test!(test_chrono_version_pinned, "chrono");
workspace_dep_pinned_test!(test_uuid_version_pinned, "uuid");
workspace_dep_pinned_test!(test_serde_json_version_pinned, "serde_json");
workspace_dep_pinned_test!(test_thiserror_version_pinned, "thiserror");
workspace_dep_pinned_test!(test_tracing_version_pinned, "tracing");
workspace_dep_pinned_test!(test_clap_version_pinned, "clap");
workspace_dep_pinned_test!(test_sha2_version_pinned, "sha2");
workspace_dep_pinned_test!(test_globset_version_pinned, "globset");
workspace_dep_pinned_test!(test_tempfile_version_pinned, "tempfile");
workspace_dep_pinned_test!(test_walkdir_version_pinned, "walkdir");
workspace_dep_pinned_test!(test_axum_version_pinned, "axum");
workspace_dep_pinned_test!(test_reqwest_version_pinned, "reqwest");
workspace_dep_pinned_test!(test_futures_version_pinned, "futures");

// ===========================================================================
// Group 8: Naming conventions
// ===========================================================================

/// Known exceptions to the abp- prefix convention.
const NAMING_EXCEPTIONS: &[&str] = &[
    "claude-bridge",
    "codex-bridge",
    "copilot-bridge",
    "gemini-bridge",
    "kimi-bridge",
    "openai-bridge",
    "sidecar-kit",
    "xtask",
    "agent-backplane",
];

#[test]
fn test_crate_names_follow_abp_prefix_convention() {
    for c in publishable_crates() {
        if !NAMING_EXCEPTIONS.contains(&c.name.as_str()) {
            assert!(
                c.name.starts_with("abp-"),
                "crate '{}' should follow the abp- prefix convention",
                c.name
            );
        }
    }
}

#[test]
fn test_known_naming_exceptions_exist() {
    let names = all_crate_names();
    for exc in &[
        "claude-bridge",
        "codex-bridge",
        "copilot-bridge",
        "gemini-bridge",
        "kimi-bridge",
        "openai-bridge",
        "sidecar-kit",
    ] {
        assert!(
            names.contains(*exc),
            "expected naming exception '{exc}' should be a workspace member"
        );
    }
}

#[test]
fn test_naming_exceptions_are_publishable() {
    let crates = publishable_crates();
    let names: HashSet<&str> = crates.iter().map(|c| c.name.as_str()).collect();
    for exc in &[
        "claude-bridge",
        "codex-bridge",
        "copilot-bridge",
        "gemini-bridge",
        "kimi-bridge",
        "openai-bridge",
        "sidecar-kit",
    ] {
        assert!(
            names.contains(*exc),
            "naming exception '{exc}' should be publishable (in crates/)"
        );
    }
}

#[test]
fn test_no_unexpected_naming_exceptions() {
    let publishable_exceptions: Vec<String> = publishable_crates()
        .into_iter()
        .filter(|c| !c.name.starts_with("abp-"))
        .map(|c| c.name)
        .collect();
    let known: HashSet<&str> = NAMING_EXCEPTIONS.iter().copied().collect();
    for name in &publishable_exceptions {
        assert!(
            known.contains(name.as_str()),
            "crate '{name}' does not follow abp- prefix and is not a known exception"
        );
    }
}

// ===========================================================================
// Group 9: Inter-crate dependency consistency
// ===========================================================================

fn crate_depends_on(crate_info: &CrateInfo, dep_name: &str) -> bool {
    crate_info
        .deps_table("dependencies")
        .map(|deps| deps.contains_key(dep_name))
        .unwrap_or(false)
}

fn find_crate<'a>(crates: &'a [CrateInfo], name: &str) -> &'a CrateInfo {
    crates
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("crate '{name}' not found"))
}

#[test]
fn test_all_internal_deps_are_workspace_members() {
    let names = all_crate_names();
    for c in all_workspace_crates() {
        for section in ["dependencies", "dev-dependencies"] {
            if let Some(deps) = c.deps_table(section) {
                for (dep_name, val) in deps {
                    if dep_has_path(val) {
                        assert!(
                            names.contains(dep_name.as_str()),
                            "{}: path dep '{dep_name}' is not a workspace member",
                            c.name
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn test_abp_core_has_minimal_internal_deps() {
    let crates = publishable_crates();
    let core = find_crate(&crates, "abp-core");
    let deps = core.deps_table("dependencies").unwrap();
    let internal_deps: Vec<&String> = deps.keys().filter(|k| k.starts_with("abp-")).collect();
    // abp-core should only depend on abp-error at most
    assert!(
        internal_deps.len() <= 1,
        "abp-core should have minimal internal deps, found: {internal_deps:?}"
    );
}

#[test]
fn test_protocol_depends_on_core() {
    let crates = publishable_crates();
    let proto = find_crate(&crates, "abp-protocol");
    assert!(
        crate_depends_on(proto, "abp-core"),
        "abp-protocol should depend on abp-core"
    );
}

#[test]
fn test_host_depends_on_protocol() {
    let crates = publishable_crates();
    let host = find_crate(&crates, "abp-host");
    assert!(
        crate_depends_on(host, "abp-protocol"),
        "abp-host should depend on abp-protocol"
    );
}

#[test]
fn test_integrations_depends_on_core() {
    let crates = publishable_crates();
    let integ = find_crate(&crates, "abp-integrations");
    assert!(
        crate_depends_on(integ, "abp-core"),
        "abp-integrations should depend on abp-core"
    );
}

#[test]
fn test_runtime_depends_on_integrations() {
    let crates = publishable_crates();
    let rt = find_crate(&crates, "abp-runtime");
    assert!(
        crate_depends_on(rt, "abp-integrations"),
        "abp-runtime should depend on abp-integrations"
    );
}

#[test]
fn test_cli_depends_on_runtime() {
    let crates = publishable_crates();
    let cli = find_crate(&crates, "abp-cli");
    assert!(
        crate_depends_on(cli, "abp-runtime"),
        "abp-cli should depend on abp-runtime"
    );
}

#[test]
fn test_policy_depends_on_glob() {
    let crates = publishable_crates();
    let policy = find_crate(&crates, "abp-policy");
    assert!(
        crate_depends_on(policy, "abp-glob"),
        "abp-policy should depend on abp-glob"
    );
}

#[test]
fn test_workspace_depends_on_glob() {
    let crates = publishable_crates();
    let ws = find_crate(&crates, "abp-workspace");
    assert!(
        crate_depends_on(ws, "abp-glob"),
        "abp-workspace should depend on abp-glob"
    );
}

#[test]
fn test_receipt_depends_on_core() {
    let crates = publishable_crates();
    let receipt = find_crate(&crates, "abp-receipt");
    assert!(
        crate_depends_on(receipt, "abp-core"),
        "abp-receipt should depend on abp-core"
    );
}

#[test]
fn test_no_publishable_crate_depends_on_cli() {
    let crates = publishable_crates();
    for c in &crates {
        if c.name == "abp-cli" {
            continue;
        }
        assert!(
            !crate_depends_on(c, "abp-cli"),
            "{} should not depend on abp-cli (leaf crate)",
            c.name
        );
    }
}

#[test]
fn test_no_publishable_crate_depends_on_daemon() {
    let crates = publishable_crates();
    for c in &crates {
        if c.name == "abp-daemon" {
            continue;
        }
        assert!(
            !crate_depends_on(c, "abp-daemon"),
            "{} should not depend on abp-daemon (leaf crate)",
            c.name
        );
    }
}

#[test]
fn test_backend_mock_depends_on_backend_core() {
    let crates = publishable_crates();
    let mock = find_crate(&crates, "abp-backend-mock");
    assert!(
        crate_depends_on(mock, "abp-backend-core"),
        "abp-backend-mock should depend on abp-backend-core"
    );
}

#[test]
fn test_backend_sidecar_depends_on_backend_core() {
    let crates = publishable_crates();
    let sidecar = find_crate(&crates, "abp-backend-sidecar");
    assert!(
        crate_depends_on(sidecar, "abp-backend-core"),
        "abp-backend-sidecar should depend on abp-backend-core"
    );
}

#[test]
fn test_sidecar_sdk_depends_on_host() {
    let crates = publishable_crates();
    let sdk = find_crate(&crates, "abp-sidecar-sdk");
    assert!(
        crate_depends_on(sdk, "abp-host"),
        "abp-sidecar-sdk should depend on abp-host"
    );
}

#[test]
fn test_projection_depends_on_capability() {
    let crates = publishable_crates();
    let proj = find_crate(&crates, "abp-projection");
    assert!(
        crate_depends_on(proj, "abp-capability"),
        "abp-projection should depend on abp-capability"
    );
}

#[test]
fn test_internal_dep_versions_match_workspace() {
    let root = root_cargo_toml();
    let ws_version = workspace_package(&root)["version"].as_str().unwrap();
    for c in publishable_crates() {
        for section in ["dependencies", "dev-dependencies"] {
            if let Some(deps) = c.deps_table(section) {
                for (dep_name, val) in deps {
                    if dep_has_path(val) {
                        if let Some(v) = dep_version(val) {
                            assert_eq!(
                                v, ws_version,
                                "{}: internal dep '{dep_name}' version '{v}' != workspace '{ws_version}'",
                                c.name
                            );
                        }
                    }
                }
            }
        }
    }
}

// ===========================================================================
// Group 10: Version consistency
// ===========================================================================

#[test]
fn test_all_versions_consistent() {
    let root = root_cargo_toml();
    let ws_version = workspace_package(&root)["version"].as_str().unwrap();
    for c in publishable_crates() {
        let pkg = c.package();
        if let Some(v) = pkg.get("version").and_then(|v| v.as_str()) {
            assert_eq!(
                v, ws_version,
                "{} version '{v}' should match workspace version '{ws_version}'",
                c.name
            );
        }
        // If workspace-inherited, it's fine
    }
}

#[test]
fn test_workspace_version_is_zero_one_zero() {
    let root = root_cargo_toml();
    let ws_version = workspace_package(&root)["version"].as_str().unwrap();
    assert_eq!(
        ws_version, "0.1.0",
        "current workspace version should be 0.1.0"
    );
}

// ===========================================================================
// Group 11: Additional robustness checks
// ===========================================================================

#[test]
fn test_crate_count_matches_expected() {
    let crates = publishable_crates();
    let names: Vec<&str> = crates.iter().map(|c| c.name.as_str()).collect();
    // Verify specific known crates exist
    let expected = [
        "abp-core",
        "abp-protocol",
        "abp-host",
        "abp-glob",
        "abp-policy",
        "abp-workspace",
        "abp-runtime",
        "abp-cli",
        "abp-integrations",
        "abp-receipt",
        "abp-backend-core",
        "abp-backend-mock",
        "abp-stream",
        "sidecar-kit",
        "claude-bridge",
    ];
    for name in &expected {
        assert!(
            names.contains(name),
            "expected crate '{name}' not found in publishable crates"
        );
    }
}

#[test]
fn test_no_build_script_in_leaf_crates() {
    let ws_root = workspace_root();
    let leaf_crates = ["abp-glob", "abp-dialect", "abp-telemetry", "abp-config"];
    for name in &leaf_crates {
        let crates = publishable_crates();
        let c = find_crate(&crates, name);
        let build_rs = ws_root
            .join(&c.member_path)
            .join("build.rs");
        assert!(
            !build_rs.is_file(),
            "{name} should not have a build.rs script"
        );
    }
}

#[test]
fn test_workspace_deps_use_minor_pinning() {
    let root = root_cargo_toml();
    let deps = workspace_deps_table(&root);
    for (name, val) in deps {
        if let Some(v) = dep_version(val) {
            // Should not use exact = versions or < constraints for normal deps
            assert!(
                !v.starts_with('='),
                "workspace dep '{name}' uses exact version pinning '={v}', prefer caret ranges"
            );
        }
    }
}

#[test]
fn test_all_publishable_crates_complete_metadata() {
    for c in publishable_crates() {
        let pkg = c.package();
        let fields = ["name", "description", "readme", "keywords", "categories"];
        for field in &fields {
            let has_direct = pkg.get(*field).is_some();
            let has_ws = is_workspace_inherited(pkg, field);
            assert!(
                has_direct || has_ws,
                "{}: missing required metadata field '{field}'",
                c.name
            );
        }
        // Also check workspace-inherited fields
        let ws_fields = ["version", "edition", "license", "authors", "repository"];
        for field in &ws_fields {
            assert!(
                is_workspace_inherited(pkg, field) || has_string_value(pkg, field),
                "{}: missing workspace-inherited field '{field}'",
                c.name
            );
        }
    }
}

#[test]
fn test_keywords_are_reasonable_length() {
    for c in publishable_crates() {
        let pkg = c.package();
        if let Some(kw) = pkg.get("keywords").and_then(|k| k.as_array()) {
            assert!(
                kw.len() <= 5,
                "{}: crates.io allows at most 5 keywords, found {}",
                c.name,
                kw.len()
            );
        }
    }
}

#[test]
fn test_categories_use_valid_slugs() {
    // crates.io categories are slugged strings
    for c in publishable_crates() {
        let pkg = c.package();
        if let Some(cats) = pkg.get("categories").and_then(|k| k.as_array()) {
            for cat in cats {
                let s = cat.as_str().expect("category should be a string");
                assert!(!s.is_empty(), "{}: category should not be empty", c.name);
                assert!(
                    s == s.to_lowercase() || s.contains('-'),
                    "{}: category '{s}' should be a valid crates.io slug",
                    c.name
                );
            }
        }
    }
}

#[test]
fn test_workspace_dep_features_are_explicit_for_serde() {
    let root = root_cargo_toml();
    let deps = workspace_deps_table(&root);
    let serde = deps.get("serde").expect("serde should be a workspace dep");
    if let Value::Table(t) = serde {
        let features = t.get("features").and_then(|f| f.as_array());
        assert!(
            features.is_some(),
            "serde workspace dep should declare features explicitly"
        );
        let feature_strs: Vec<&str> = features
            .unwrap()
            .iter()
            .filter_map(|f| f.as_str())
            .collect();
        assert!(
            feature_strs.contains(&"derive"),
            "serde should have 'derive' feature enabled"
        );
    }
}

#[test]
fn test_workspace_dep_features_are_explicit_for_tokio() {
    let root = root_cargo_toml();
    let deps = workspace_deps_table(&root);
    let tokio = deps.get("tokio").expect("tokio should be a workspace dep");
    if let Value::Table(t) = tokio {
        let features = t.get("features").and_then(|f| f.as_array());
        assert!(
            features.is_some(),
            "tokio workspace dep should declare features explicitly"
        );
    }
}

#[test]
fn test_repository_url_format() {
    let root = root_cargo_toml();
    let pkg = workspace_package(&root);
    let repo = pkg["repository"].as_str().unwrap();
    assert!(
        repo.starts_with("https://github.com/"),
        "repository URL should be a GitHub HTTPS URL"
    );
}

#[test]
fn test_readme_files_exist_for_publishable_crates() {
    let ws_root = workspace_root();
    for c in publishable_crates() {
        let pkg = c.package();
        if let Some(readme) = pkg.get("readme").and_then(|r| r.as_str()) {
            let readme_path = ws_root.join(&c.member_path).join(readme);
            assert!(
                readme_path.is_file(),
                "{}: readme file '{}' does not exist",
                c.name,
                readme
            );
        }
    }
}
