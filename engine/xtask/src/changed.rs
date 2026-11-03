//! Which crates, C# test projects and other parts a change reaches, for the
//! fast level.
//!
//! The change is the Git diff against a merge base, plus uncommitted and
//! untracked files. The base comes from the branch's upstream when it has one,
//! otherwise from origin/main. Each path maps to a workspace crate and the
//! crates that depend on it, a C# test project, or the Win32 test host. A path
//! that reaches none of them either needs no test or is reported as changed
//! code with no test, which fails the level.
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// One workspace crate, as `cargo tree` describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    /// The crate folder from the repository root, such as `engine/op-core`.
    pub dir: String,
    /// The workspace crates it depends on: normal, build and dev.
    pub deps: Vec<String>,
}

/// One C# test project and the paths its tests depend on.
#[derive(Debug, PartialEq, Eq)]
pub struct CSharpProject {
    /// The name of the part in a level's output.
    pub part: &'static str,
    pub project: &'static str,
    /// Extra `dotnet test` arguments that the tool's README asks for.
    pub args: &'static [&'static str],
    /// Path prefixes: a change under one of them runs this project.
    pub inputs: &'static [&'static str],
}

pub const CSHARP_TESTS: [CSharpProject; 3] = [
    CSharpProject {
        part: "capture-kit tests",
        project: "tools/pdn-capture/tests/PdnCaptureTests.csproj",
        args: &[],
        // `DecisionTests` reads the control list in the docs folder.
        inputs: &["tools/pdn-capture/", "docs/windows/pdn-5.1-controls.json"],
    },
    CSharpProject {
        part: "ui-spike tests",
        project: "tools/ui-spike/tests/SpikeTests.csproj",
        args: &["-c", "Release"],
        inputs: &["tools/ui-spike/"],
    },
    CSharpProject {
        part: "ui-look tests",
        project: "tools/ui-look/tests/UiLookTests.csproj",
        args: &[],
        // The icon tests read the icon table, the icon files and the Mac source.
        // The app copies the plasma picture into its output folder.
        inputs: &[
            "tools/ui-look/",
            "docs/windows/icons.toml",
            "tests/fixtures/plasma_alpha_512x384.png",
            "windows/assets/icons/",
            "mac/OpenPainterKit/Sources/",
            "mac/OpenPainter/",
            "mac/EngineBridge/Sources/",
        ],
    },
];

/// The Windows app's test project. It stays apart from `CSHARP_TESTS`, the tool
/// projects, so each list keeps its own place in a level.
pub const APP_TESTS: [CSharpProject; 1] = [CSharpProject {
    part: "Windows app tests",
    project: "windows/OpenPainter.Tests/OpenPainter.Tests.csproj",
    args: &[],
    // The app links W6's look files, embeds the catalog, and its bridge comes from
    // the C header. The tests read the Mac source for words and sizes, the harness's
    // judgement and the test values, and those values come from the Swift tests.
    inputs: &[
        "windows/OpenPainter/",
        "windows/OpenPainter.Tests/",
        "windows/tools/bridge-gen/",
        "windows/assets/",
        "windows/harness/",
        "windows/tests/",
        "mac/Tests/OpenPainterKitTests/ZoomTests.swift",
        "mac/Tests/OpenPainterKitTests/ColorsTests.swift",
        "mac/Tests/OpenPainterKitTests/menu-skeleton.txt",
        "engine/op-ffi/include/openpainter.h",
        "tools/ui-look/app/src/Tokens.cs",
        "tools/ui-look/app/src/Icons.cs",
        "tools/ui-look/app/src/Ui.cs",
        "mac/OpenPainterKit/Sources/Shell/StatusBarView.swift",
        "mac/OpenPainterKit/Sources/Shell/EditorWindowController.swift",
    ],
}];

impl CSharpProject {
    fn reads(&self, path: &str) -> bool {
        // A README in a tool folder changes no test. Under windows/assets/icons
        // a test does read LICENSES.md, so this rule covers tools/ only.
        if path.starts_with("tools/") && path.ends_with(".md") {
            return false;
        }
        self.inputs.iter().any(|input| path.starts_with(input))
    }
}

/// The Win32 test host's folder. It is outside the engine workspace.
const HOST_DIR: &str = "tools/win32-host/";

/// The workspace crates that `tools/win32-host/Cargo.toml` names. A change that
/// reaches one of them reaches the host.
pub const HOST_CRATES: [&str; 2] = ["op-ffi", "op-color"];

/// Files outside `docs/` that change no behaviour. Keep this list short: a path
/// on it can never fail the fast level.
const NO_TEST_FILES: [&str; 3] = [".gitignore", ".gitattributes", "LICENSE"];

/// Files outside a crate folder that one crate's tests read.
const CRATE_INPUTS: [(&str, &str); 11] = [
    ("docs/reports/parity-matrix.md", "op-engine"),
    // `opcli dump-catalog` builds these into itself, and its tests hold it to
    // the Mac's own records and the checked-in copy.
    (
        "mac/OpenPainterKit/Sources/Shell/MenuSkeleton.swift",
        "op-cli",
    ),
    ("mac/OpenPainterKit/Sources/Shell/ToolState.swift", "op-cli"),
    (
        "mac/OpenPainterKit/Sources/Shell/PanelLayout.swift",
        "op-cli",
    ),
    ("mac/OpenPainterKit/Sources/ZoomPresets.swift", "op-cli"),
    (
        "mac/OpenPainterKit/Sources/Shell/SelectionEditActions.swift",
        "op-cli",
    ),
    (
        "mac/OpenPainterKit/Sources/Shell/KeyboardShortcuts.swift",
        "op-cli",
    ),
    ("docs/windows/icons.toml", "op-cli"),
    ("mac/Tests/OpenPainterKitTests/menu-skeleton.txt", "op-cli"),
    ("docs/shortcuts.md", "op-cli"),
    ("windows/assets/catalog/mac-catalog.json", "op-cli"),
];

/// The crate whose tests run the shell scripts in `scripts/`. Those tests exist
/// only on Unix, so on Windows a changed script has no test.
const SCRIPT_TESTS: &str = "xtask";

/// What the fast level runs for a set of changed paths.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FastPlan {
    /// Changed crates and their dependents, sorted by name.
    pub crates: Vec<String>,
    pub csharp: Vec<&'static CSharpProject>,
    /// The Windows app's test project, when the change reaches it.
    pub app: Vec<&'static CSharpProject>,
    /// The Win32 test host's folder or a crate it depends on changed.
    pub host: bool,
    /// Changed paths that reach nothing and need no test.
    pub no_test_needed: Vec<String>,
    /// Changed paths that reach nothing but should have a test.
    pub no_test: Vec<String>,
    /// Changed paths under `mac/` that only the Swift build reads.
    pub swift_only: Vec<String>,
    /// The branch and short hash the change was measured from, when Git found
    /// no changed file at all.
    pub nothing_changed_since: Option<String>,
}

/// True for a path that reaches nothing and is allowed to: docs, Markdown and
/// the files on `NO_TEST_FILES`.
fn needs_no_test(path: &str) -> bool {
    path.starts_with("docs/") || path.ends_with(".md") || NO_TEST_FILES.contains(&path)
}

/// The crate whose tests read this path, for a path outside any crate folder.
fn crate_input(path: &str, script_tests_run: bool) -> Option<&'static str> {
    if script_tests_run && path.starts_with("scripts/") && path.ends_with(".sh") {
        return Some(SCRIPT_TESTS);
    }
    CRATE_INPUTS
        .iter()
        .find(|(input, _)| *input == path)
        .map(|(_, name)| *name)
}

/// Paths that every crate depends on: the workspace build settings, and the
/// shared `tests/` folder of fixtures, goldens and tolerances.
fn reaches_every_crate(path: &str) -> bool {
    matches!(path, "engine/Cargo.toml" | "engine/Cargo.lock")
        || path.starts_with("engine/.cargo/")
        // No crate runs a Python file there; the oracle runs at milestone.
        || (path.starts_with("tests/") && !path.ends_with(".py"))
}

/// `script_tests_run` is false on Windows, where the tests of `scripts/*.sh` do
/// not exist.
pub fn plan(paths: &[String], members: &[Member], script_tests_run: bool) -> FastPlan {
    let mut reached = BTreeSet::new();
    let mut csharp = BTreeSet::new();
    let mut app = BTreeSet::new();
    let mut host = false;
    let mut no_test_needed = Vec::new();
    let mut no_test = Vec::new();
    let mut swift_only = Vec::new();
    for path in paths {
        let mut mapped = false;
        if reaches_every_crate(path) {
            reached.extend(members.iter().map(|member| member.name.as_str()));
            mapped = true;
        } else if let Some(member) = members
            .iter()
            .find(|member| path.starts_with(&format!("{}/", member.dir)))
        {
            reached.insert(member.name.as_str());
            mapped = true;
        } else if let Some(name) = crate_input(path, script_tests_run) {
            reached.insert(name);
            mapped = true;
        }
        if path.starts_with(HOST_DIR) && !path.ends_with(".md") {
            host = true;
            mapped = true;
        }
        for (index, project) in CSHARP_TESTS.iter().enumerate() {
            if project.reads(path) {
                csharp.insert(index);
                mapped = true;
            }
        }
        for (index, project) in APP_TESTS.iter().enumerate() {
            if project.reads(path) {
                app.insert(index);
                mapped = true;
            }
        }
        if mapped {
            continue;
        }
        if needs_no_test(path) {
            no_test_needed.push(path.clone());
        } else if path.starts_with("mac/") {
            swift_only.push(path.clone());
        } else {
            no_test.push(path.clone());
        }
    }
    // Add dependents until a pass adds nothing new.
    loop {
        let before = reached.len();
        for member in members {
            if member.deps.iter().any(|dep| reached.contains(dep.as_str())) {
                reached.insert(member.name.as_str());
            }
        }
        if reached.len() == before {
            break;
        }
    }
    let host = host || HOST_CRATES.iter().any(|name| reached.contains(name));
    FastPlan {
        crates: reached.into_iter().map(str::to_owned).collect(),
        csharp: csharp.into_iter().map(|i| &CSHARP_TESTS[i]).collect(),
        app: app.into_iter().map(|i| &APP_TESTS[i]).collect(),
        host,
        no_test_needed,
        no_test,
        swift_only,
        nothing_changed_since: None,
    }
}

/// The plan for a tree with no Git: every crate, every C# project and the host.
pub fn plan_everything(members: &[Member]) -> FastPlan {
    let mut crates: Vec<String> = members.iter().map(|member| member.name.clone()).collect();
    crates.sort();
    FastPlan {
        crates,
        csharp: CSHARP_TESTS.iter().collect(),
        app: APP_TESTS.iter().collect(),
        host: true,
        ..FastPlan::default()
    }
}

/// The arguments that list each workspace crate at depth 0 and its direct
/// dependencies at depth 1: all kinds, on every target. `cargo tree` keeps this
/// tool free of a JSON crate. A new crate would change `engine/Cargo.lock`,
/// and a lock change reaches every crate until it merges to main.
const TREE_ARGS: [&str; 11] = [
    "tree",
    "--workspace",
    "--target",
    "all",
    "--no-dedupe",
    "--prefix",
    "depth",
    "--depth",
    "1",
    "--edges=normal,build,dev",
    "--format={p}",
];

/// Splits `op-core v0.1.0 (C:\engine\op-core) (*)` into name and path. A crate
/// from the registry has no path; a proc macro has `(proc-macro)` there.
fn tree_entry(text: &str) -> Option<(&str, Option<&str>)> {
    let text = text.strip_suffix(" (*)").unwrap_or(text);
    let mut words = text.splitn(3, ' ');
    let name = words.next()?;
    words.next()?;
    let path = words
        .next()
        .and_then(|rest| rest.strip_prefix('('))
        .and_then(|rest| rest.strip_suffix(')'));
    Some((name, path))
}

/// Reads `cargo tree` output. `root` is the engine folder.
pub fn parse_tree(text: &str, root: &str) -> Result<Vec<Member>, String> {
    let root = root.replace('\\', "/").to_ascii_lowercase();
    let mut members: Vec<Member> = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let entry = line.get(1..).and_then(tree_entry);
        let Some((name, path)) = entry else {
            return Err(format!("cargo tree printed a bad line: {line}"));
        };
        if line.starts_with('0') {
            let manifest = path.unwrap_or_default().replace('\\', "/");
            let folder = manifest
                .to_ascii_lowercase()
                .strip_prefix(&format!("{root}/"))
                .map(|rest| manifest[manifest.len() - rest.len()..].to_owned())
                .ok_or_else(|| format!("cargo tree: {line} is not a crate in {root}"))?;
            members.push(Member {
                name: name.to_owned(),
                dir: format!("engine/{folder}"),
                deps: Vec::new(),
            });
        } else if line.starts_with('1') {
            let Some(member) = members.last_mut() else {
                return Err(format!("cargo tree: {line} comes before any crate"));
            };
            if path.is_some() && name != member.name {
                member.deps.push(name.to_owned());
            }
        } else {
            return Err(format!("cargo tree printed a bad line: {line}"));
        }
    }
    if members.is_empty() {
        return Err("cargo tree listed no crates".to_owned());
    }
    // A dependency outside the workspace reaches no crate here.
    let names: BTreeSet<String> = members.iter().map(|member| member.name.clone()).collect();
    for member in &mut members {
        member.deps.retain(|dep| names.contains(dep));
        member.deps.sort();
        member.deps.dedup();
    }
    Ok(members)
}

pub fn workspace_members(engine: &Path) -> Result<Vec<Member>, String> {
    let text = command_output("cargo", &TREE_ARGS, engine)?;
    parse_tree(&text, &engine.to_string_lossy())
}

/// The branch to compare with, and how it was chosen. A long-lived branch has
/// everything since it began in its diff with origin/main, so the upstream
/// comes first. `@{upstream}` fails when the branch has none, or at a detached
/// HEAD; a Git that cannot run then fails by name at the merge base.
fn base_branch(repo: &Path) -> (String, &'static str) {
    let args = [
        "rev-parse",
        "--abbrev-ref",
        "--symbolic-full-name",
        "@{upstream}",
    ];
    match command_output("git", &args, repo) {
        Ok(name) if !name.trim().is_empty() => (name.trim().to_owned(), "the branch's upstream"),
        _ => ("origin/main".to_owned(), "the branch has no upstream"),
    }
}

/// The paths a change touched, and what they were compared with.
#[derive(Debug)]
pub struct Changed {
    pub base: String,
    /// The branch and the merge base's short hash, such as `origin/main 1a2b3c4`.
    pub since: String,
    pub paths: Vec<String>,
}

/// Every path that differs from the merge base with the branch's upstream or
/// with origin/main: commits, staged and unstaged changes, and untracked files
/// that Git does not ignore. A rename lists both the old and the new path.
pub fn changed_files(repo: &Path) -> Result<Changed, String> {
    let (branch, why) = base_branch(repo);
    let base = command_output("git", &["merge-base", "HEAD", &branch], repo)?;
    let short = command_output("git", &["rev-parse", "--short", base.trim()], repo)?;
    let diff = command_output(
        "git",
        &["diff", "--name-only", "--no-renames", "-z", base.trim()],
        repo,
    )?;
    let new = command_output(
        "git",
        &["ls-files", "--others", "--exclude-standard", "-z"],
        repo,
    )?;
    let files: BTreeSet<&str> = diff
        .split('\0')
        .chain(new.split('\0'))
        .filter(|path| !path.is_empty())
        .collect();
    Ok(Changed {
        base: format!("the merge base with {branch} ({why}) at {}", short.trim()),
        since: format!("{branch} {}", short.trim()),
        paths: files.into_iter().map(str::to_owned).collect(),
    })
}

fn command_output(program: &str, args: &[&str], dir: &Path) -> Result<String, String> {
    let line = format!("{program} {}", args.join(" "));
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|error| format!("{line} could not start: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{line} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| format!("{line} printed bad text: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A plan on Mac or Linux, where the script tests run.
    fn plan(paths: &[String], members: &[Member]) -> FastPlan {
        super::plan(paths, members, true)
    }

    fn member(name: &str, deps: &[&str]) -> Member {
        Member {
            name: name.into(),
            dir: format!("engine/{name}"),
            deps: deps.iter().map(|dep| (*dep).to_owned()).collect(),
        }
    }

    /// `leaf` has no dependents; `core` -> `mid` -> `top`, and `tester` depends
    /// on `core` as a dev dependency, which `cargo tree` lists the same way.
    fn members() -> Vec<Member> {
        vec![
            member("core", &[]),
            member("mid", &["core"]),
            member("top", &["mid"]),
            member("tester", &["core"]),
            member("leaf", &[]),
        ]
    }

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|path| (*path).to_owned()).collect()
    }

    fn parts(plan: &FastPlan) -> Vec<&'static str> {
        plan.csharp.iter().map(|project| project.part).collect()
    }

    #[test]
    fn a_crate_file_maps_to_the_crate_and_all_its_dependents() {
        let plan = plan(&paths(&["engine/core/src/lib.rs"]), &members());
        assert_eq!(plan.crates, ["core", "mid", "tester", "top"]);
        assert!(plan.csharp.is_empty() && plan.no_test.is_empty());
    }

    #[test]
    fn a_crate_with_no_dependents_maps_to_itself_only() {
        let plan = plan(&paths(&["engine/leaf/tests/a.rs"]), &members());
        assert_eq!(plan.crates, ["leaf"]);
    }

    #[test]
    fn a_folder_that_only_starts_with_a_crate_name_does_not_map_to_it() {
        let plan = plan(&paths(&["engine/core-notes/a.rs"]), &members());
        assert!(plan.crates.is_empty());
        assert_eq!(plan.no_test, ["engine/core-notes/a.rs"]);
    }

    #[test]
    fn workspace_files_and_shared_test_data_map_to_every_crate() {
        for path in [
            "engine/Cargo.lock",
            "engine/Cargo.toml",
            "engine/.cargo/config.toml",
            "tests/tolerances.toml",
            "tests/fixtures/a.png",
        ] {
            let plan = plan(&paths(&[path]), &members());
            assert_eq!(plan.crates.len(), 5, "{path}");
            assert!(plan.no_test.is_empty(), "{path}");
        }
    }

    #[test]
    fn a_csharp_path_chooses_its_test_project() {
        for (path, part, project) in [
            (
                "tools/pdn-capture/tests/src/DecisionTests.cs",
                "capture-kit tests",
                "tools/pdn-capture/tests/PdnCaptureTests.csproj",
            ),
            (
                "tools/pdn-capture/driver/Program.cs",
                "capture-kit tests",
                "tools/pdn-capture/tests/PdnCaptureTests.csproj",
            ),
            (
                "tools/ui-spike/avalonia/src/App.cs",
                "ui-spike tests",
                "tools/ui-spike/tests/SpikeTests.csproj",
            ),
            (
                "tools/ui-look/app/src/Icons.cs",
                "ui-look tests",
                "tools/ui-look/tests/UiLookTests.csproj",
            ),
        ] {
            let plan = plan(&paths(&[path]), &members());
            assert_eq!(parts(&plan), [part], "{path}");
            assert_eq!(plan.csharp[0].project, project);
            assert!(plan.crates.is_empty() && plan.no_test.is_empty(), "{path}");
        }
    }

    #[test]
    fn files_the_ui_look_tests_read_choose_the_ui_look_project() {
        for path in [
            "docs/windows/icons.toml",
            "windows/assets/icons/LICENSES.md",
            "mac/OpenPainterKit/Sources/Shell/ToolState.swift",
            "mac/OpenPainter/App.swift",
            "mac/EngineBridge/Sources/Bridge.swift",
        ] {
            let plan = plan(&paths(&[path]), &members());
            assert_eq!(parts(&plan), ["ui-look tests"], "{path}");
        }
    }

    #[test]
    fn a_change_under_the_windows_app_runs_its_tests() {
        for path in [
            "windows/OpenPainter/src/Program.cs",
            "windows/OpenPainter/OpenPainter.csproj",
            "windows/OpenPainter.Tests/src/ShellTests.cs",
            "windows/tools/bridge-gen/src/Header.cs",
            "windows/assets/icons/cmd-save.svg",
            "windows/assets/catalog/mac-catalog.json",
            "engine/op-ffi/include/openpainter.h",
            "tools/ui-look/app/src/Tokens.cs",
        ] {
            let plan = plan(&paths(&[path]), &members());
            let projects: Vec<_> = plan.app.iter().map(|project| project.project).collect();
            assert_eq!(
                projects,
                ["windows/OpenPainter.Tests/OpenPainter.Tests.csproj"],
                "{path}"
            );
            assert!(plan.no_test.is_empty(), "{path}");
        }
    }

    #[test]
    fn a_change_under_the_harness_or_the_vectors_runs_the_windows_app_tests() {
        for path in [
            "windows/harness/src/Logic.cs",
            "windows/harness/OpenPainter.Harness.csproj",
            "windows/harness/HeadlessAot/src/Runner.cs",
            "windows/tests/vectors/zoom-steps.json",
            "mac/Tests/OpenPainterKitTests/ZoomTests.swift",
            "mac/Tests/OpenPainterKitTests/ColorsTests.swift",
            "mac/Tests/OpenPainterKitTests/menu-skeleton.txt",
        ] {
            let plan = plan(&paths(&[path]), &members());
            assert_eq!(plan.app.len(), 1, "{path}");
            assert!(
                plan.no_test.is_empty() && plan.swift_only.is_empty(),
                "{path}"
            );
        }
    }

    #[test]
    fn other_changes_do_not_run_the_windows_app_tests() {
        for path in [
            "tools/ui-look/app/src/Panels.cs",
            "windows/probes/a.cs",
            "engine/op-core/src/lib.rs",
            "docs/windows/PLAN.md",
        ] {
            let plan = plan(&paths(&[path]), &members());
            assert!(plan.app.is_empty(), "{path}");
        }
    }

    #[test]
    fn files_dump_catalog_reads_reach_op_cli() {
        for path in [
            "mac/OpenPainterKit/Sources/Shell/MenuSkeleton.swift",
            "mac/OpenPainterKit/Sources/Shell/ToolState.swift",
            "mac/OpenPainterKit/Sources/Shell/PanelLayout.swift",
            "mac/OpenPainterKit/Sources/ZoomPresets.swift",
            "mac/OpenPainterKit/Sources/Shell/SelectionEditActions.swift",
            "mac/OpenPainterKit/Sources/Shell/KeyboardShortcuts.swift",
            "docs/windows/icons.toml",
            "docs/shortcuts.md",
            "mac/Tests/OpenPainterKitTests/menu-skeleton.txt",
            "windows/assets/catalog/mac-catalog.json",
        ] {
            let plan = plan(&paths(&[path]), &members());
            assert!(plan.crates.contains(&"op-cli".to_owned()), "{path}");
        }
    }

    #[test]
    fn the_whole_tree_plan_has_the_windows_app_tests() {
        assert_eq!(plan_everything(&members()).app.len(), APP_TESTS.len());
    }

    #[test]
    fn a_csharp_project_runs_once_for_many_files() {
        let plan = plan(
            &paths(&["tools/ui-look/app/a.cs", "tools/ui-look/tests/b.cs"]),
            &members(),
        );
        assert_eq!(parts(&plan), ["ui-look tests"]);
    }

    #[test]
    fn docs_markdown_and_listed_files_need_no_test() {
        let list = [
            ".gitattributes",
            ".gitignore",
            "LICENSE",
            "README.md",
            "docs/windows/PLAN.md",
            "docs/windows/w3-host.toml",
            "tools/ui-look/README.md",
            "tools/win32-host/README.md",
        ];
        let plan = plan(&paths(&list), &members());
        assert!(plan.crates.is_empty() && plan.csharp.is_empty() && !plan.host);
        assert_eq!(plan.no_test_needed, list);
        assert!(plan.no_test.is_empty() && plan.swift_only.is_empty());
    }

    #[test]
    fn changed_code_that_reaches_nothing_is_named_as_having_no_test() {
        let list = [
            "perf/baseline.toml",
            "scripts/windows/preflight.ps1",
            "tools/catalog/a.py",
            "windows/probes/a.cs",
        ];
        let plan = plan(&paths(&list), &members());
        assert!(plan.crates.is_empty() && plan.csharp.is_empty());
        assert_eq!(plan.no_test, list);
        assert!(plan.no_test_needed.is_empty());
    }

    #[test]
    fn a_mac_path_outside_the_icon_folders_counts_only_for_the_swift_build() {
        let plan = plan(&paths(&["mac/project.yml", "mac/README.md"]), &members());
        assert_eq!(plan.swift_only, ["mac/project.yml"]);
        assert_eq!(plan.no_test_needed, ["mac/README.md"]);
        assert!(plan.no_test.is_empty());
    }

    #[test]
    fn a_host_file_reaches_the_host_and_no_crate() {
        let plan = plan(&paths(&["tools/win32-host/src/main.rs"]), &members());
        assert!(plan.host);
        assert!(plan.crates.is_empty() && plan.no_test.is_empty());
    }

    #[test]
    fn a_crate_the_host_depends_on_reaches_the_host() {
        for (path, host) in [
            ("engine/op-ffi/src/lib.rs", true),
            ("engine/op-color/src/lib.rs", true),
            ("engine/leaf/src/lib.rs", false),
        ] {
            let members = [
                member("op-ffi", &[]),
                member("op-color", &[]),
                member("leaf", &[]),
            ];
            assert_eq!(plan(&paths(&[path]), &members).host, host, "{path}");
        }
    }

    #[test]
    fn host_crates_match_the_hosts_manifest() {
        let manifest =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/win32-host/Cargo.toml");
        let text = fs::read_to_string(&manifest).unwrap();
        let mut named: Vec<&str> = text
            .split("path = \"../../engine/")
            .skip(1)
            .map(|rest| rest.split('"').next().unwrap())
            .collect();
        let mut listed = HOST_CRATES.to_vec();
        named.sort_unstable();
        listed.sort_unstable();
        assert_eq!(named, listed);
    }

    #[test]
    fn files_tests_read_outside_their_folders_reach_those_tests() {
        let plan_of = |path| {
            plan(
                &paths(&[path]),
                &[
                    member("op-engine", &[]),
                    member("xtask", &[]),
                    member("core", &[]),
                ],
            )
        };

        let controls = plan_of("docs/windows/pdn-5.1-controls.json");
        assert_eq!(parts(&controls), ["capture-kit tests"]);
        assert!(controls.no_test_needed.is_empty());

        let plasma = plan_of("tests/fixtures/plasma_alpha_512x384.png");
        assert_eq!(parts(&plasma), ["ui-look tests"]);
        assert_eq!(plasma.crates, ["core", "op-engine", "xtask"]);

        let matrix = plan_of("docs/reports/parity-matrix.md");
        assert_eq!(matrix.crates, ["op-engine"]);
        assert!(matrix.no_test_needed.is_empty());

        let script = plan_of("scripts/linux-check.sh");
        assert_eq!(script.crates, ["xtask"]);
        assert!(script.no_test.is_empty());
    }

    #[test]
    fn a_shell_script_reaches_xtask_only_where_its_tests_run() {
        let members = [member("xtask", &[])];
        let list = paths(&["scripts/linux-check.sh"]);
        let unix = super::plan(&list, &members, true);
        assert_eq!(unix.crates, ["xtask"]);
        assert!(unix.no_test.is_empty());
        let windows = super::plan(&list, &members, false);
        assert!(windows.crates.is_empty());
        assert_eq!(windows.no_test, list);
    }

    #[test]
    fn a_python_file_under_tests_has_no_test_and_reaches_no_crate() {
        let list = [
            "tests/oracle/legacy336/flood/test_flood.py",
            "tests/oracle/legacy336/test_oracle.py",
        ];
        let plan = plan(&paths(&list), &members());
        assert!(plan.crates.is_empty());
        assert_eq!(plan.no_test, list);
    }

    #[test]
    fn the_whole_tree_plan_has_every_crate_project_and_the_host() {
        let plan = plan_everything(&members());
        assert_eq!(plan.crates, ["core", "leaf", "mid", "tester", "top"]);
        assert_eq!(
            parts(&plan),
            ["capture-kit tests", "ui-spike tests", "ui-look tests"]
        );
        assert!(plan.host);
    }

    #[test]
    fn the_tree_gives_folders_and_workspace_path_dependencies() {
        let text = "0a v0.1.0 (C:\\repo\\engine\\a)\n\
                    1b v0.1.0 (C:\\repo\\engine\\b)\n\
                    1a v0.1.0 (C:\\repo\\engine\\a) (*)\n\
                    1serde v1.0.0\n\
                    1serde_derive v1.0.0 (proc-macro)\n\
                    1elsewhere v0.1.0 (C:\\other\\elsewhere)\n\
                    0b v0.1.0 (C:\\repo\\engine\\b)\n";
        assert_eq!(
            parse_tree(text, "c:\\repo\\engine").unwrap(),
            [member("a", &["b"]), member("b", &[])]
        );
    }

    #[test]
    fn a_tree_that_makes_no_sense_fails_by_name() {
        let error = parse_tree("0a v0.1.0 (/other/a)\n", "/repo/engine").unwrap_err();
        assert!(error.contains("/other/a"), "{error}");
        assert!(parse_tree("", "/repo/engine").is_err());
        assert!(parse_tree("1a v0.1.0 (/repo/engine/a)\n", "/repo/engine").is_err());
        assert!(parse_tree("garbage\n", "/repo/engine").is_err());
    }

    #[test]
    fn the_real_workspace_maps_op_gpu_to_its_dependents() {
        let engine = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let members = workspace_members(engine).unwrap();
        let plan = super::plan(&paths(&["engine/op-gpu/src/lib.rs"]), &members, true);
        assert_eq!(
            plan.crates,
            [
                "op-cli",
                "op-engine",
                "op-ffi",
                "op-gpu",
                "op-present-spike"
            ]
        );
        assert!(plan.host);
        let plan = self::plan(&paths(&["engine/xtask/src/main.rs"]), &members);
        assert_eq!(plan.crates, ["xtask"]);
        assert!(!plan.host);
    }

    /// A throwaway repository with one commit. `origin_main` adds the ref the
    /// fast level looks for.
    struct TempRepo(std::path::PathBuf);

    impl TempRepo {
        fn new(tag: &str, origin_main: bool) -> Self {
            let dir =
                std::env::temp_dir().join(format!("op-xtask-git-{tag}-{}", std::process::id()));
            if dir.exists() {
                fs::remove_dir_all(&dir).unwrap();
            }
            fs::create_dir_all(&dir).unwrap();
            let repo = Self(dir);
            repo.git(&["init", "-q"]);
            repo.write("kept.txt");
            repo.write("edited.txt");
            repo.write("staged.txt");
            repo.write(".gitignore");
            fs::write(repo.0.join(".gitignore"), "ignored.txt\n").unwrap();
            repo.git(&["add", "."]);
            repo.commit("first");
            if origin_main {
                repo.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
            }
            repo
        }

        fn write(&self, name: &str) {
            let old = fs::read_to_string(self.0.join(name)).unwrap_or_default();
            fs::write(self.0.join(name), format!("{old}line\n")).unwrap();
        }

        fn commit(&self, message: &str) {
            self.git(&[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-q",
                "-m",
                message,
            ]);
        }

        fn git(&self, args: &[&str]) {
            command_output("git", args, &self.0).unwrap();
        }

        fn short_head(&self) -> String {
            let text = command_output("git", &["rev-parse", "--short", "HEAD"], &self.0).unwrap();
            text.trim().to_owned()
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_dir_all(&self.0) {
                eprintln!("could not remove {}: {error}", self.0.display());
            }
        }
    }

    #[test]
    fn changed_files_lists_commits_staged_unstaged_and_untracked_files() {
        let repo = TempRepo::new("all", true);
        repo.write("committed.txt");
        repo.git(&["add", "committed.txt"]);
        repo.commit("second");
        repo.write("edited.txt");
        repo.write("staged.txt");
        repo.git(&["add", "staged.txt"]);
        repo.write("untracked.txt");
        repo.write("ignored.txt");
        assert_eq!(
            changed_files(&repo.0).unwrap().paths,
            ["committed.txt", "edited.txt", "staged.txt", "untracked.txt"]
        );
    }

    #[test]
    fn a_rename_lists_both_paths() {
        let repo = TempRepo::new("rename", true);
        repo.git(&["mv", "kept.txt", "moved.txt"]);
        assert_eq!(
            changed_files(&repo.0).unwrap().paths,
            ["kept.txt", "moved.txt"]
        );
    }

    #[test]
    fn with_no_upstream_the_change_is_measured_from_origin_main_and_says_so() {
        let repo = TempRepo::new("no-upstream", true);
        let short = repo.short_head();
        let changed = changed_files(&repo.0).unwrap();
        assert_eq!(
            changed.base,
            format!("the merge base with origin/main (the branch has no upstream) at {short}")
        );
        assert_eq!(changed.since, format!("origin/main {short}"));
    }

    #[test]
    fn a_branch_with_an_upstream_is_measured_from_it_and_not_from_origin_main() {
        let repo = TempRepo::new("upstream", true);
        repo.git(&["checkout", "-q", "-b", "long"]);
        repo.write("old-branch-work.txt");
        repo.git(&["add", "old-branch-work.txt"]);
        repo.commit("work already on the upstream");
        repo.git(&["update-ref", "refs/remotes/origin/long", "HEAD"]);
        repo.git(&["config", "remote.origin.url", "."]);
        repo.git(&[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ]);
        repo.git(&["config", "branch.long.remote", "origin"]);
        repo.git(&["config", "branch.long.merge", "refs/heads/long"]);
        let short = repo.short_head();
        repo.write("edited.txt");
        let changed = changed_files(&repo.0).unwrap();
        assert_eq!(changed.paths, ["edited.txt"]);
        assert_eq!(
            changed.base,
            format!("the merge base with origin/long (the branch's upstream) at {short}")
        );
    }

    #[test]
    fn a_clean_tree_at_the_merge_base_has_no_changed_files() {
        let repo = TempRepo::new("clean", true);
        assert!(changed_files(&repo.0).unwrap().paths.is_empty());
    }

    #[test]
    fn a_missing_origin_main_fails_and_names_the_git_command() {
        let repo = TempRepo::new("no-origin", false);
        let error = changed_files(&repo.0).unwrap_err();
        assert!(
            error.contains("git merge-base HEAD origin/main exited with"),
            "{error}"
        );
    }
}
