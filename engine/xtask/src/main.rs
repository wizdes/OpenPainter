//! Run the workspace check tiers.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

mod changed;
mod fuzz;

use changed::{CSharpProject, FastPlan};

/// The fast level's time limit in `docs/windows/PLAN.md` section 4.1.
const FAST_LIMIT_SECONDS: u64 = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Os {
    Windows,
    Macos,
    Linux,
}

impl Os {
    fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
}

#[derive(Clone, Copy)]
enum Status {
    Pass,
    Fail,
    Skip,
    NotRun,
    /// Prints NOT RUN and is listed as owed, but does not fail the level.
    Owed,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Skip => "SKIP",
            Self::NotRun | Self::Owed => "NOT RUN",
        }
    }
}

struct Step {
    name: String,
    duration: Duration,
    status: Status,
}

/// The engine workspace folder. `cargo run` sets CARGO_MANIFEST_DIR at run
/// time; prefer it over the path fixed at compile time, because cargo can
/// reuse an xtask binary built when the source lived somewhere else, and the
/// checks would then run against that old folder.
fn engine_dir(runtime: Option<std::ffi::OsString>, compiled: &str) -> PathBuf {
    let manifest = runtime.map_or_else(|| PathBuf::from(compiled), PathBuf::from);
    manifest.parent().expect("workspace parent").to_path_buf()
}

/// How a command's result is read, beyond its exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputRule {
    ExitCode,
    /// `dotnet test` exits 0 when it runs no tests, so the total must be above 0.
    DotnetTotal,
    /// The host exits 0 with `NOT RUN` lines, so those lines are read.
    HostScenarios,
    /// `dotnet publish` with NativeAOT: the exit code, with the tool it needs on PATH.
    AotPublish,
    /// The real-window harness: any part that did not pass fails, by name.
    Harness,
}

const HOST_MANIFEST: &str = "tools/win32-host/Cargo.toml";

/// What `dotnet test` prints: each failure with its message and stack.
const DOTNET_TEST_LOGGER: &str = "console;verbosity=minimal";

/// One command a tier runs, with any extra environment it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandStep {
    name: String,
    program: &'static str,
    args: Vec<&'static str>,
    env: Vec<(&'static str, &'static str)>,
    /// Runs from the repository root instead of the engine folder.
    in_repo: bool,
    /// A missing program is a SKIP instead of a FAIL.
    skip_if_missing: bool,
    output: OutputRule,
    /// A part that must draw on the GPU, so an environment that turns the GPU
    /// off fails it.
    needs_gpu: bool,
}

impl CommandStep {
    fn runs_cargo_test(&self) -> bool {
        self.program == "cargo" && self.args.first() == Some(&"test")
    }

    fn cargo(args: &[&'static str], env: &[(&'static str, &'static str)]) -> Self {
        let prefix: String = env
            .iter()
            .map(|(key, value)| format!("{key}={value} "))
            .collect();
        Self {
            name: format!("{prefix}cargo {}", args.join(" ")),
            program: "cargo",
            args: args.to_vec(),
            env: env.to_vec(),
            in_repo: false,
            skip_if_missing: false,
            output: OutputRule::ExitCode,
            needs_gpu: false,
        }
    }
}

/// How many cases each proptest runs in a tier's release test run.
fn proptest_cases(tier: &str) -> Option<&'static str> {
    match tier {
        "milestone" => Some("1024"),
        "checkpoint" => Some("10000"),
        _ => None,
    }
}

fn fmt_step() -> CommandStep {
    CommandStep::cargo(&["fmt", "--all", "--check"], &[])
}

/// The engine checks the milestone and checkpoint tiers run first.
fn base_steps() -> Vec<CommandStep> {
    vec![
        fmt_step(),
        CommandStep::cargo(
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            &[],
        ),
        // Each cargo test step runs every test program, so one known failure
        // cannot hide the programs after it.
        CommandStep::cargo(&["test", "--workspace", "--no-fail-fast"], &[]),
    ]
}

/// The release tests, oracle tests and benchmarks of the milestone and checkpoint tiers.
/// The debug test run above keeps the default case count, because debug proptests run
/// about ten times slower.
fn deep_steps(tier: &str) -> Vec<CommandStep> {
    let Some(cases) = proptest_cases(tier) else {
        return Vec::new();
    };
    let mut steps = vec![
        CommandStep::cargo(
            &["test", "--workspace", "--release", "--no-fail-fast"],
            &[("OP_PROPTEST_CASES", cases)],
        ),
        CommandStep {
            name: "python3 -B tests/oracle/legacy336/test_oracle.py".into(),
            program: "python3",
            args: vec!["-B", "tests/oracle/legacy336/test_oracle.py"],
            env: Vec::new(),
            in_repo: true,
            skip_if_missing: true,
            output: OutputRule::ExitCode,
            needs_gpu: false,
        },
    ];
    if tier == "checkpoint" {
        steps.push(CommandStep::cargo(
            &[
                "run",
                "-p",
                "op-core",
                "--example",
                "tile_bench",
                "--release",
            ],
            &[],
        ));
    }
    steps
}

/// M57 robustness checks of the checkpoint tier: one minute of fuzzing per
/// reader (`cargo xtask fuzz`, the 10-minute run is by hand), the 200 MP
/// document and the full-disk test (a 20 MB disk image it detaches by its own
/// device path; macOS only).
fn robustness_steps(tier: &str) -> Vec<CommandStep> {
    if tier != "checkpoint" {
        return Vec::new();
    }
    vec![
        CommandStep::cargo(
            &[
                "run",
                "--quiet",
                "-p",
                "xtask",
                "--",
                "fuzz",
                "--seconds",
                "60",
            ],
            &[],
        ),
        CommandStep::cargo(
            &[
                "test",
                "-p",
                "op-engine",
                "--release",
                "--test",
                "huge_document",
                "--",
                "--ignored",
            ],
            &[],
        ),
        CommandStep::cargo(
            &[
                "test",
                "-p",
                "op-engine",
                "--test",
                "low_disk",
                "--",
                "--ignored",
            ],
            &[],
        ),
    ]
}

/// The full-disk test compiles to zero tests off macOS, so a pass there would
/// prove nothing.
fn is_macos_only(step: &CommandStep) -> bool {
    step.args.contains(&"low_disk")
}

/// `dotnet test` for one C# project. The two last switches stop the build from
/// leaving MSBuild and compiler servers running after the level ends. The
/// environment turns off the usage reports that .NET and Avalonia's build send.
/// `-v:q` also quiets the test logger, which then prints only `[FAIL]` and the name, so the
/// logger is named on its own: at `minimal` it prints each failure's message and stack, and
/// no line for a test that passes.
fn csharp_step(project: &CSharpProject) -> CommandStep {
    let mut args = vec![
        "test",
        project.project,
        "-v:q",
        "--logger",
        DOTNET_TEST_LOGGER,
        "-nr:false",
        "-p:UseSharedCompilation=false",
    ];
    args.extend_from_slice(project.args);
    CommandStep {
        name: format!("{}: dotnet test {}", project.part, project.project),
        program: "dotnet",
        args,
        env: vec![
            ("AVALONIA_TELEMETRY_OPTOUT", "1"),
            ("DOTNET_CLI_TELEMETRY_OPTOUT", "1"),
        ],
        in_repo: true,
        skip_if_missing: false,
        output: OutputRule::DotnetTotal,
        needs_gpu: false,
    }
}

/// The C# tests of the milestone and checkpoint tiers. All 3 projects target
/// Windows, so Mac and Linux runs print a SKIP row instead.
fn csharp_steps(tier: &str, windows: bool) -> Vec<CommandStep> {
    if !windows || !matches!(tier, "milestone" | "checkpoint") {
        return Vec::new();
    }
    changed::CSHARP_TESTS.iter().map(csharp_step).collect()
}

/// The Windows app's parts of the milestone and checkpoint tiers. Its tests read
/// the export table of the release DLL, so the DLL is built first.
fn app_steps(tier: &str, windows: bool) -> Vec<CommandStep> {
    if !windows || !matches!(tier, "milestone" | "checkpoint") {
        return Vec::new();
    }
    let mut steps = vec![CommandStep {
        name: "release op_ffi.dll: cargo build -p op-ffi --release".into(),
        program: "cargo",
        args: vec!["build", "-p", "op-ffi", "--release", "-j", "8"],
        env: Vec::new(),
        in_repo: false,
        skip_if_missing: false,
        output: OutputRule::ExitCode,
        needs_gpu: false,
    }];
    steps.extend(changed::APP_TESTS.iter().map(csharp_step));
    steps
}

/// The checkpoint tier's GPU parts on Windows, on each adapter: the suites of
/// `docs/windows/test-counts.md`, then the Win32 test host. The host needs a
/// signed-in desktop session and fails by name without one.
fn windows_gpu_steps(tier: &str, windows: bool) -> Vec<CommandStep> {
    if !windows || tier != "checkpoint" {
        return Vec::new();
    }
    let mut steps = Vec::new();
    for (adapter, label) in [("hardware", "RTX"), ("warp", "WARP")] {
        let env = [("OP_GPU_ADAPTER", adapter)];
        let suites: [&[&'static str]; 4] = [
            &[
                "test",
                "-p",
                "op-gpu",
                "--no-fail-fast",
                "-j",
                "8",
                "--",
                "--test-threads=1",
            ],
            &[
                "test",
                "-p",
                "op-ffi",
                "--no-fail-fast",
                "-j",
                "8",
                "--",
                "--test-threads=1",
            ],
            &[
                "test",
                "-p",
                "op-engine",
                "--test",
                "m54_parity",
                "--test",
                "engine",
                "--test",
                "jobs",
                "--test",
                "gpu_effects",
                "--no-fail-fast",
                "-j",
                "8",
                "--",
                "--test-threads=1",
            ],
            &[
                "test",
                "-p",
                "op-engine",
                "--lib",
                "--no-fail-fast",
                "-j",
                "8",
                "--",
                "--test-threads=1",
            ],
        ];
        for suite in suites {
            let mut step = CommandStep::cargo(suite, &env);
            step.name = format!("GPU matrix on {label}: {}", step.name);
            step.needs_gpu = true;
            steps.push(step);
        }
    }
    for (adapter, label) in [("hardware", "RTX"), ("warp", "WARP")] {
        let mut step = CommandStep::cargo(
            &[
                "run",
                "--manifest-path",
                "tools/win32-host/Cargo.toml",
                "-j",
                "8",
            ],
            &[("OP_GPU_ADAPTER", adapter)],
        );
        step.name = format!("Win32 test host on {label}: {}", step.name);
        step.in_repo = true;
        step.output = OutputRule::HostScenarios;
        step.needs_gpu = true;
        steps.push(step);
    }
    steps
}

/// Format, lint and build for the Win32 test host, which is outside the
/// workspace. None of the three opens a window.
fn host_static_steps() -> Vec<CommandStep> {
    let suites: [(&str, &[&'static str]); 3] = [
        (
            "format",
            &["fmt", "--manifest-path", HOST_MANIFEST, "--check"],
        ),
        (
            "lint",
            &[
                "clippy",
                "--manifest-path",
                HOST_MANIFEST,
                "--no-deps",
                "--all-targets",
                "-j",
                "8",
                "--",
                "-D",
                "warnings",
            ],
        ),
        (
            "build",
            &["build", "--manifest-path", HOST_MANIFEST, "-j", "8"],
        ),
    ];
    suites
        .into_iter()
        .map(|(label, args)| {
            let mut step = CommandStep::cargo(args, &[]);
            step.name = format!("Win32 test host {label}: {}", step.name);
            step.in_repo = true;
            step
        })
        .collect()
}

/// A named part of a level that this run does not reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UnreachedPart {
    name: &'static str,
    reason: &'static str,
    /// Listed as owed instead of failing the level.
    owed: bool,
}

const fn fails_level(name: &'static str, reason: &'static str) -> UnreachedPart {
    UnreachedPart {
        name,
        reason,
        owed: false,
    }
}

/// Parts that `docs/windows/PLAN.md` section 4.1 names and this run does not
/// reach. Each prints NOT RUN and fails the level. The one exception is the
/// paint.net comparison off Windows: captured images are private, so a Mac or
/// Linux run lists it as owed and does not fail.
fn unreached_parts(tier: &str, windows: bool) -> Vec<UnreachedPart> {
    match (tier, windows) {
        ("milestone", true) => vec![
            fails_level(
                "headless UI on the AOT build",
                "the run plan puts its steps here",
            ),
            fails_level(
                "9 to 12 critical flows",
                "no list of flows exists yet; a flow needs tools, files and undo, which come in W8 to W10",
            ),
        ],
        ("checkpoint", true) => vec![
            fails_level(
                "all real-window flows",
                "--tier window runs the W7 checks; the flows need tools and files from W8 to W10",
            ),
            fails_level(
                "paint.net comparison",
                "no level step yet; the capture kit runs by hand",
            ),
            fails_level("Mac run", "run this level on the Mac and record the result"),
            fails_level("Linux run", "run this level on Linux and record the result"),
        ],
        ("checkpoint", false) => vec![UnreachedPart {
            name: "paint.net comparison",
            reason: "captured images are private to the capture repo",
            owed: true,
        }],
        _ => Vec::new(),
    }
}

fn record_unreached(steps: &mut Vec<Step>, part: UnreachedPart) {
    if part.owed {
        println!("NOT RUN 0.00s {}: {} (owed)", part.name, part.reason);
        steps.push(Step {
            name: part.name.into(),
            duration: Duration::ZERO,
            status: Status::Owed,
        });
    } else {
        not_run(steps, part.name, part.reason);
    }
}

/// Why this run has no Swift build, or None when it has one. Only macOS has
/// xcodebuild, and `mac/project.yml` is on the Windows branch and in Linux copies.
fn swift_build_skip(os: Os, has_project: bool) -> Option<&'static str> {
    if os != Os::Macos {
        Some("the Mac app builds only on macOS")
    } else if has_project {
        None
    } else {
        Some("mac/project.yml is absent")
    }
}

fn swift_part(os: Os, has_project: bool) -> Part {
    if let Some(reason) = swift_build_skip(os, has_project) {
        return Part::skip("Swift build", reason);
    }
    // TODO: Generate the Xcode project from project.yml before this build when the Mac app exists.
    Part::Command(CommandStep {
        name: "Swift build".into(),
        program: "xcodebuild",
        args: vec![
            "-project",
            "mac/OpenPainter.xcodeproj",
            "-scheme",
            "OpenPainter",
            "build",
        ],
        env: Vec::new(),
        in_repo: true,
        skip_if_missing: false,
        output: OutputRule::ExitCode,
        needs_gpu: false,
    })
}

/// One thing a level does, in the order it does it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Command(CommandStep),
    /// A cargo command on the crates a change reached.
    CrateCommand(Vec<String>),
    Skip {
        name: String,
        reason: String,
    },
    Fail {
        name: String,
        reason: String,
    },
    NotRun {
        name: String,
        reason: String,
    },
    Unreached(UnreachedPart),
    /// One command for each file in `tests/scenarios`, listed when it runs.
    Scenarios,
    Script(&'static str),
    /// A row that names something an earlier part already ran, and takes its result.
    SameAs {
        name: String,
        part: &'static str,
    },
}

impl Part {
    fn skip(name: &str, reason: &str) -> Self {
        Self::Skip {
            name: name.into(),
            reason: reason.into(),
        }
    }

    /// The name of the row this part prints.
    fn name(&self) -> String {
        match self {
            Self::Command(step) => step.name.clone(),
            Self::CrateCommand(args) => format!("cargo {}", args.join(" ")),
            Self::Skip { name, .. }
            | Self::Fail { name, .. }
            | Self::NotRun { name, .. }
            | Self::SameAs { name, .. } => name.clone(),
            Self::Unreached(part) => part.name.to_owned(),
            Self::Scenarios => "opcli scenarios".to_owned(),
            Self::Script(name) => (*name).to_owned(),
        }
    }
}

const WINDOWS_ONLY: &str = "this part exists only on Windows";

/// The parts of a level that exist only on Windows. A Mac or Linux run prints a
/// SKIP row for each, so none is silently left out.
fn windows_only_names(tier: &str) -> &'static [&'static str] {
    match tier {
        "milestone" => &[
            "C# tests",
            "Win32 test host",
            "headless UI on the AOT build",
            "9 to 12 critical flows",
        ],
        "checkpoint" => &[
            "C# tests",
            "Win32 test host",
            "GPU matrix on RTX and WARP",
            "all real-window flows",
        ],
        _ => &[],
    }
}

/// The row for changed code that nothing tested, or None when there is none.
fn no_test_row(paths: &[String]) -> Option<Part> {
    const SHOWN: usize = 5;
    if paths.is_empty() {
        return None;
    }
    let mut names = paths
        .iter()
        .take(SHOWN)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if paths.len() > SHOWN {
        names.push_str(", and more");
    }
    Some(Part::NotRun {
        name: format!("changed files with no test: {}, {names}", paths.len()),
        reason: "no crate, C# project or host reads them".into(),
    })
}

/// What the fast level runs, given the changed files or the reason they are
/// unknown.
fn fast_parts(os: Os, has_project: bool, plan: &Result<FastPlan, String>) -> Vec<Part> {
    let mut parts = vec![Part::Command(fmt_step())];
    let plan = match plan {
        Ok(plan) => plan,
        Err(reason) => {
            let unknown = "the changed files are unknown".to_owned();
            parts.push(Part::Fail {
                name: "changed files".into(),
                reason: reason.clone(),
            });
            for name in ["changed crates", "changed C# tests"] {
                parts.push(Part::NotRun {
                    name: name.into(),
                    reason: unknown.clone(),
                });
            }
            parts.push(swift_part(os, has_project));
            return parts;
        }
    };
    if let Some(since) = &plan.nothing_changed_since {
        parts.push(Part::skip(
            "changed crates and C# tests",
            &format!("nothing changed since {since}"),
        ));
    }
    parts.extend(
        fast_crate_commands(&plan.crates)
            .into_iter()
            .map(Part::CrateCommand),
    );
    if plan.host {
        if os == Os::Windows {
            parts.extend(host_static_steps().into_iter().map(Part::Command));
        } else {
            parts.push(Part::skip("Win32 test host", WINDOWS_ONLY));
        }
    }
    if os == Os::Windows && !plan.app.is_empty() {
        // The app's tests load the release DLL, and a clean tree has none.
        parts.push(Part::Command(app_steps("milestone", true).remove(0)));
    }
    for project in plan.csharp.iter().chain(&plan.app) {
        let step = csharp_step(project);
        if os == Os::Windows {
            parts.push(Part::Command(step));
        } else {
            parts.push(Part::skip(&step.name, WINDOWS_ONLY));
        }
    }
    if os != Os::Windows {
        parts.push(Part::skip("headless UI for the changed area", WINDOWS_ONLY));
    } else if let Some(app) = plan.app.first() {
        parts.push(Part::SameAs {
            name: format!("headless UI for the changed area, inside {}", app.part),
            part: app.part,
        });
    }
    // Only the Swift build reads the rest of `mac/`, and only macOS has it.
    let mut no_test = plan.no_test.clone();
    if os != Os::Macos {
        no_test.extend_from_slice(&plan.swift_only);
    }
    parts.extend(no_test_row(&no_test));
    parts.push(swift_part(os, has_project));
    parts
}

/// What a level runs, in order. `plan` is read only by the fast level.
fn level_parts(
    tier: &str,
    os: Os,
    has_project: bool,
    plan: &Result<FastPlan, String>,
) -> Vec<Part> {
    if tier == "fast" {
        return fast_parts(os, has_project, plan);
    }
    let windows = os == Os::Windows;
    let mut parts: Vec<Part> = base_steps().into_iter().map(Part::Command).collect();
    parts.push(swift_part(os, has_project));
    for step in deep_steps(tier).into_iter().chain(robustness_steps(tier)) {
        if os != Os::Macos && is_macos_only(&step) {
            parts.push(Part::skip(
                &step.name,
                "this test compiles to zero tests off macOS",
            ));
        } else {
            parts.push(Part::Command(step));
        }
    }
    if matches!(tier, "milestone" | "checkpoint") {
        parts.push(Part::Scenarios);
    }
    parts.extend(csharp_steps(tier, windows).into_iter().map(Part::Command));
    if windows && matches!(tier, "milestone" | "checkpoint") {
        parts.extend(host_static_steps().into_iter().map(Part::Command));
    }
    parts.extend(
        windows_gpu_steps(tier, windows)
            .into_iter()
            .map(Part::Command),
    );
    if tier == "checkpoint" {
        for script in checkpoint_scripts() {
            if os == Os::Macos {
                parts.push(Part::Script(script));
            } else {
                parts.push(Part::skip(script, "the Mac app builds only on macOS"));
            }
        }
    }
    if !windows {
        parts.extend(
            windows_only_names(tier)
                .iter()
                .map(|name| Part::skip(name, WINDOWS_ONLY)),
        );
    }
    parts.extend(
        unreached_parts(tier, windows)
            .into_iter()
            .map(Part::Unreached),
    );
    parts
}

/// All of a level's parts: `level_parts`, with the Windows app's parts after the
/// C# tool projects.
fn all_level_parts(
    tier: &str,
    os: Os,
    has_project: bool,
    plan: &Result<FastPlan, String>,
) -> Vec<Part> {
    let mut parts = level_parts(tier, os, has_project, plan);
    let app = app_steps(tier, os == Os::Windows);
    if app.is_empty() {
        return parts;
    }
    let last_tool = changed::CSHARP_TESTS[changed::CSHARP_TESTS.len() - 1].part;
    let after = parts
        .iter()
        .position(|part| matches!(part, Part::Command(step) if step.name.starts_with(last_tool)))
        .map_or(parts.len(), |at| at + 1);
    parts.splice(after..after, app.into_iter().map(Part::Command));
    parts
}

const AOT_TESTS_PROJECT: &str = "windows/harness/HeadlessAot/HeadlessAot.csproj";
const AOT_TESTS_FOLDER: &str = "windows/harness/HeadlessAot/bin/publish";
const AOT_TESTS_PROGRAM: &str =
    "windows/harness/HeadlessAot/bin/publish/OpenPainter.HeadlessAot.exe";
const APP_PROJECT: &str = "windows/OpenPainter/OpenPainter.csproj";
const APP_PUBLISH_FOLDER: &str = "windows/OpenPainter/bin/publish";
const APP_PROGRAM: &str = "windows/OpenPainter/bin/publish/OpenPainter.exe";
const HARNESS_PROJECT: &str = "windows/harness/OpenPainter.Harness.csproj";
const HARNESS_OUT: &str = "windows/harness/bin/last-run";
const DOTNET_ENV: [(&str, &str); 2] = [
    ("AVALONIA_TELEMETRY_OPTOUT", "1"),
    ("DOTNET_CLI_TELEMETRY_OPTOUT", "1"),
];

fn dotnet_in_repo(name: &str, args: &[&'static str], output: OutputRule) -> CommandStep {
    CommandStep {
        name: name.to_owned(),
        program: "dotnet",
        args: args.to_vec(),
        env: DOTNET_ENV.to_vec(),
        in_repo: true,
        skip_if_missing: false,
        output,
        needs_gpu: false,
    }
}

fn aot_publish_step(name: &str, project: &'static str, folder: &'static str) -> CommandStep {
    dotnet_in_repo(
        name,
        &[
            "publish",
            project,
            "-c",
            "Release",
            "-r",
            "win-x64",
            "-o",
            folder,
            "-v:q",
            "-nr:false",
            "-p:UseSharedCompilation=false",
        ],
        OutputRule::AotPublish,
    )
}

/// The app's headless tests compiled by NativeAOT, then run with no test framework.
/// The run prints a total in `dotnet test`'s words, so the same rule reads it.
fn aot_headless_steps() -> Vec<CommandStep> {
    vec![
        aot_publish_step(
            "publish the AOT headless tests: dotnet publish",
            AOT_TESTS_PROJECT,
            AOT_TESTS_FOLDER,
        ),
        CommandStep {
            name: "headless UI on the AOT build: all the app's headless tests".to_owned(),
            program: AOT_TESTS_PROGRAM,
            args: Vec::new(),
            env: DOTNET_ENV.to_vec(),
            in_repo: true,
            skip_if_missing: false,
            output: OutputRule::DotnetTotal,
            needs_gpu: false,
        },
    ]
}

/// The window level on Windows: the AOT app, then the real-window harness. No switch
/// here lets a short or busy run pass, and the harness fails on any part that did not.
fn window_parts() -> Vec<Part> {
    let dll = app_steps("milestone", true).remove(0);
    let steps = vec![
        dll,
        aot_publish_step(
            "publish the AOT app: dotnet publish",
            APP_PROJECT,
            APP_PUBLISH_FOLDER,
        ),
        dotnet_in_repo(
            "build the harness: dotnet build",
            &[
                "build",
                HARNESS_PROJECT,
                "-c",
                "Release",
                "-v:q",
                "-nr:false",
                "-p:UseSharedCompilation=false",
            ],
            OutputRule::ExitCode,
        ),
        dotnet_in_repo(
            "real-window harness: pan trace, read-back, color, shots, hit test, panels, icons",
            &[
                "run",
                "--no-build",
                "-c",
                "Release",
                "--project",
                HARNESS_PROJECT,
                "--",
                "--app",
                APP_PROGRAM,
                "--out",
                HARNESS_OUT,
            ],
            OutputRule::Harness,
        ),
    ];
    steps.into_iter().map(Part::Command).collect()
}

const FLOW_LIST: &str = "docs/windows/flows.md";
const FLOW_ROW: &str = "9 to 12 critical flows";
const FEWEST_FLOWS: usize = 9;
const MOST_FLOWS: usize = 12;

/// One row of `docs/windows/flows.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Flow {
    number: String,
    name: String,
    milestone: String,
    how: String,
    runs_now: bool,
}

/// The rows of the flow table: `| number | name | steps | milestone | how | runs now |`.
fn read_flows(text: &str) -> Result<Vec<Flow>, String> {
    let mut flows = Vec::new();
    for line in text.lines() {
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if !line.trim_start().starts_with('|')
            || cells.first().is_none_or(|c| c.parse::<u32>().is_err())
        {
            continue;
        }
        let [number, name, _steps, milestone, how, runs] = cells.as_slice() else {
            return Err(format!(
                "{FLOW_LIST}: a row needs 6 cells, and this has {}: {line}",
                cells.len()
            ));
        };
        let runs_now = match *runs {
            "yes" => true,
            "no" => false,
            other => {
                return Err(format!(
                    "{FLOW_LIST}: flow {number}: `{other}` is not yes or no"
                ));
            }
        };
        flows.push(Flow {
            number: (*number).to_owned(),
            name: (*name).to_owned(),
            milestone: (*milestone).to_owned(),
            how: (*how).to_owned(),
            runs_now,
        });
    }
    if !(FEWEST_FLOWS..=MOST_FLOWS).contains(&flows.len()) {
        return Err(format!(
            "{FLOW_LIST}: {} flows, and the level needs {FEWEST_FLOWS} to {MOST_FLOWS}",
            flows.len()
        ));
    }
    Ok(flows)
}

/// One row per flow. A flow that can run runs its one test on the AOT build, and any other
/// prints NOT RUN by name and fails the level, as PLAN 4.1 says of a part it does not reach.
fn flow_parts(flows: &[Flow]) -> Vec<Part> {
    flows
        .iter()
        .map(|flow| {
            let name = format!("flow {}: {}", flow.number, flow.name);
            if !flow.runs_now {
                return Part::NotRun {
                    name,
                    reason: format!("{} makes this possible ({})", flow.milestone, flow.how),
                };
            }
            // A step keeps its arguments as static words, and this one lasts for the whole run.
            let test: &'static str =
                Box::leak(format!("FlowTests.Flow{:0>2}", flow.number).into_boxed_str());
            Part::Command(CommandStep {
                name,
                program: AOT_TESTS_PROGRAM,
                args: vec!["--only", test],
                env: DOTNET_ENV.to_vec(),
                in_repo: true,
                skip_if_missing: false,
                output: OutputRule::DotnetTotal,
                needs_gpu: false,
            })
        })
        .collect()
}

/// Puts the flow list in place of the one NOT RUN row. With no list the row stays and fails
/// the level. With a list that does not read, the level fails by name.
fn with_flows(parts: Vec<Part>, list: Option<&str>) -> Vec<Part> {
    let Some(text) = list else { return parts };
    let is_row = |part: &Part| matches!(part, Part::Unreached(row) if row.name == FLOW_ROW);
    let Some(at) = parts.iter().position(is_row) else {
        return parts;
    };
    let rows = match read_flows(text) {
        Ok(flows) => flow_parts(&flows),
        Err(reason) => vec![Part::Fail {
            name: FLOW_ROW.to_owned(),
            reason,
        }],
    };
    let mut parts = parts;
    parts.splice(at..=at, rows);
    parts
}

/// What a level really runs. `level_parts` and `all_level_parts` keep the rows the tests
/// of earlier stages hold them to, and this puts the W7 steps in: the AOT tests take the
/// place of the NOT RUN row on Windows, and the window level runs the harness.
fn run_parts(tier: &str, os: Os, has_project: bool, plan: &Result<FastPlan, String>) -> Vec<Part> {
    if tier == "window" && os == Os::Windows {
        return window_parts();
    }
    let mut parts = all_level_parts(tier, os, has_project, plan);
    if os == Os::Windows && matches!(tier, "milestone" | "checkpoint") {
        let aot = aot_headless_steps().into_iter().map(Part::Command);
        let row = parts.iter().position(|part| {
            matches!(part, Part::Unreached(row) if row.name == "headless UI on the AOT build")
        });
        match row {
            Some(row) => {
                parts.splice(row..=row, aot);
            }
            None => {
                let at = parts
                    .iter()
                    .position(|part| matches!(part, Part::Unreached(_)))
                    .unwrap_or(parts.len());
                parts.splice(at..at, aot);
            }
        }
    }
    parts
}

/// `cargo clippy` and `cargo test` for the crates a change reaches. Clippy's
/// `--no-deps` keeps it from linting crates the change did not reach.
fn fast_crate_commands(crates: &[String]) -> Vec<Vec<String>> {
    if crates.is_empty() {
        return Vec::new();
    }
    let selected: Vec<String> = crates
        .iter()
        .flat_map(|name| ["-p".to_owned(), name.clone()])
        .collect();
    let command = |verb: &str, tail: &[&str]| {
        let mut args = vec![verb.to_owned()];
        args.extend_from_slice(&selected);
        args.extend(tail.iter().map(|arg| (*arg).to_owned()));
        args
    };
    vec![
        command(
            "clippy",
            &["--no-deps", "--all-targets", "--", "-D", "warnings"],
        ),
        command("test", &["--no-fail-fast"]),
    ]
}

fn fast_plan(engine: &Path, repo: &Path) -> Result<FastPlan, String> {
    let changed = changed::changed_files(repo)?;
    println!("Changed files are measured from {}.", changed.base);
    let members = changed::workspace_members(engine)?;
    let script_tests_run = Os::current() != Os::Windows;
    let mut plan = changed::plan(&changed.paths, &members, script_tests_run);
    if changed.paths.is_empty() {
        plan.nothing_changed_since = Some(changed.since);
    }
    Ok(plan)
}

/// The plan for `--all-changed`: Git is not asked.
fn whole_tree_plan(engine: &Path) -> Result<FastPlan, String> {
    println!(
        "{ALL_CHANGED}: Git is not asked; every crate and every C# project counts as changed."
    );
    let members = changed::workspace_members(engine)?;
    Ok(changed::plan_everything(&members))
}

fn print_path_list(heading: &str, paths: &[String]) {
    if paths.is_empty() {
        return;
    }
    println!("{} {heading}:", paths.len());
    for path in paths.iter().take(20) {
        println!("  {path}");
    }
    if paths.len() > 20 {
        println!("  and {} more", paths.len() - 20);
    }
}

fn print_fast_plan(plan: &FastPlan) {
    if plan.crates.is_empty() && plan.csharp.is_empty() && plan.app.is_empty() && !plan.host {
        println!("Nothing maps to a crate, a C# test project or the host.");
    }
    if !plan.crates.is_empty() {
        println!("Changed crates and dependents: {}", plan.crates.join(", "));
    }
    for project in plan.csharp.iter().chain(&plan.app) {
        println!("Changed C# test project: {}", project.project);
    }
    if plan.host {
        println!("The Win32 test host is reached.");
    }
    print_path_list("changed files need no test", &plan.no_test_needed);
    print_path_list("changed files only the Swift build reads", &plan.swift_only);
    print_path_list("changed files have no test", &plan.no_test);
}

/// The line that reports how long a level took.
fn time_line(tier: &str, seconds: f64) -> String {
    if tier != "fast" {
        return format!("{tier} level took {seconds:.2} s");
    }
    let over = if seconds >= FAST_LIMIT_SECONDS as f64 {
        ": OVER THE LIMIT"
    } else {
        ""
    };
    format!("fast level took {seconds:.2} s; the limit is {FAST_LIMIT_SECONDS} s{over}")
}

/// Where a level runs and what the shell set.
struct RunPlace<'a> {
    engine: &'a Path,
    repo: &'a Path,
    shell_env: &'a dyn Fn(&str) -> Option<String>,
}

fn run_step(steps: &mut Vec<Step>, step: &CommandStep, place: &RunPlace) {
    let dir = if step.in_repo {
        place.repo
    } else {
        place.engine
    };
    if stopped_by_shell(
        steps,
        &step.name,
        step.runs_cargo_test(),
        step.needs_gpu,
        place.shell_env,
    ) {
        return;
    }
    match step.output {
        OutputRule::ExitCode => run_with(
            steps,
            &step.name,
            dir,
            step.program,
            &step.args,
            &step.env,
            step.skip_if_missing,
        ),
        OutputRule::DotnetTotal
        | OutputRule::HostScenarios
        | OutputRule::AotPublish
        | OutputRule::Harness => {
            run_reading_output(steps, step, dir);
        }
    }
}

/// Shell settings that would let a part pass without testing anything. With
/// `OP_GPU_TESTS=0` the GPU tests print SKIP and return as passed. With
/// `OP_FORCE_CPU=1` the engine draws effects on the CPU. With `OP_BLESS` set, a
/// golden test overwrites its image and passes. Any value fails, so nobody has
/// to know which values count.
fn shell_setting_problem(
    runs_cargo_test: bool,
    needs_gpu: bool,
    shell_env: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let set = |names: &[&str]| -> String {
        names
            .iter()
            .filter_map(|name| shell_env(name).map(|value| format!("{name}={value}")))
            .collect::<Vec<_>>()
            .join(" and ")
    };
    let mut problems = Vec::new();
    if runs_cargo_test || needs_gpu {
        let gpu_off = set(&["OP_FORCE_CPU", "OP_GPU_TESTS"]);
        if !gpu_off.is_empty() {
            problems.push(format!("{gpu_off} could let this part pass with no GPU"));
        }
    }
    if runs_cargo_test {
        let bless = set(&["OP_BLESS"]);
        if !bless.is_empty() {
            problems.push(format!(
                "{bless} makes a golden test overwrite its image and pass"
            ));
        }
    }
    if problems.is_empty() {
        None
    } else {
        Some(format!("{}; unset it in the shell", problems.join("; ")))
    }
}

/// Fails the part by name, without running it, when the shell has a setting
/// that would spoil it.
fn stopped_by_shell(
    steps: &mut Vec<Step>,
    name: &str,
    runs_cargo_test: bool,
    needs_gpu: bool,
    shell_env: &dyn Fn(&str) -> Option<String>,
) -> bool {
    let Some(reason) = shell_setting_problem(runs_cargo_test, needs_gpu, shell_env) else {
        return false;
    };
    fail(steps, &format!("{name}: {reason}"), "this part did not run");
    true
}

/// Runs a command, prints its output as it comes, and judges it by that output.
fn run_reading_output(steps: &mut Vec<Step>, step: &CommandStep, dir: &Path) {
    println!("Running {}", step.name);
    let start = Instant::now();
    let mut command = Command::new(program_path(step.program, dir));
    command
        .args(&step.args)
        .current_dir(dir)
        .stdout(Stdio::piped());
    for (key, value) in &step.env {
        command.env(key, value);
    }
    if step.output == OutputRule::AotPublish
        && let Some(path) = path_with_vs_installer(
            std::env::var_os("PATH").as_deref(),
            std::env::var_os("ProgramFiles(x86)").as_deref(),
        )
    {
        command.env("PATH", path);
    }
    let finished = command.spawn().and_then(|mut child| {
        let mut text = String::new();
        if let Some(stdout) = child.stdout.take() {
            let mut reader = BufReader::new(stdout);
            let mut line = Vec::new();
            while reader.read_until(b'\n', &mut line)? > 0 {
                let mut out = std::io::stdout().lock();
                out.write_all(&line)?;
                out.flush()?;
                text.push_str(&String::from_utf8_lossy(&line));
                line.clear();
            }
        }
        Ok((child.wait()?, text))
    });
    let (result, skip_row) = match finished {
        Ok((exit, text)) => match step.output {
            OutputRule::DotnetTotal => (dotnet_result(exit.success(), &text), None),
            OutputRule::HostScenarios => {
                let warp = step.env.contains(&("OP_GPU_ADAPTER", "warp"));
                host_result(exit.success(), &text, warp)
            }
            OutputRule::Harness => (harness_result(exit.success(), &text), None),
            OutputRule::ExitCode | OutputRule::AotPublish => (exit_result(exit.success()), None),
        },
        Err(error) => (Err(format!("could not run: {error}")), None),
    };
    let duration = start.elapsed();
    let (status, name) = match result {
        Ok(()) => (Status::Pass, step.name.clone()),
        Err(reason) => {
            eprintln!("{}: {reason}", step.name);
            let status = if reason.starts_with(HARNESS_NOT_RUN) {
                Status::NotRun
            } else {
                Status::Fail
            };
            (status, format!("{}: {reason}", step.name))
        }
    };
    println!("{} {:.2}s {name}", status.label(), duration.as_secs_f64());
    steps.push(Step {
        name,
        duration,
        status,
    });
    if let Some(scenarios) = skip_row {
        skip(
            steps,
            &format!("{} scenarios: {scenarios}", host_label(step)),
            HOST_RTX_SKIP,
        );
    }
}

/// True for all 4 scenarios: the host runs them in one branch, only with
/// `OP_GPU_ADAPTER=warp`. Three call `RemoveDevice`, which `op-ffi` refuses
/// elsewhere. The fourth forces a frame panic and checks that the device stays.
const HOST_RTX_SKIP: &str = "the host runs these device recovery scenarios only on WARP: 3 remove the device, which it never does on the RTX, and 1 forces a frame panic";

/// A program named with a folder is relative to the place the step runs. Windows would
/// look for it from this process's folder instead.
fn program_path(program: &str, dir: &Path) -> PathBuf {
    if program.contains('/') {
        dir.join(program)
    } else {
        PathBuf::from(program)
    }
}

/// NativeAOT's build looks for `vswhere.exe` on PATH, and it lives in this folder. Returns
/// the new PATH, or None when the folder is not there and PATH stays as it is.
fn path_with_vs_installer(
    path: Option<&std::ffi::OsStr>,
    program_files_x86: Option<&std::ffi::OsStr>,
) -> Option<std::ffi::OsString> {
    let installer = Path::new(program_files_x86?)
        .join("Microsoft Visual Studio")
        .join("Installer");
    if !installer.is_dir() {
        return None;
    }
    let mut folders = vec![installer];
    if let Some(path) = path {
        folders.extend(std::env::split_paths(path));
    }
    std::env::join_paths(folders).ok()
}

const HARNESS_NOT_RUN: &str = "NOT RUN:";

/// Each harness part prints one line, and the last line sums them up. A part that printed
/// FAIL fails the row. If every part that did not pass printed NOT RUN, the row says NOT RUN.
/// Both fail the level. A run with no last line did not finish.
fn harness_result(success: bool, stdout: &str) -> Result<(), String> {
    let part = |line: &str, word: &str| {
        line.split_once(word)
            .filter(|(name, _)| !name.starts_with(' ') && !name.is_empty() && *name != "HARNESS")
            .map(|(name, _)| name.to_owned())
    };
    let failed: Vec<String> = stdout
        .lines()
        .filter_map(|line| part(line, ": FAIL"))
        .collect();
    let not_run: Vec<String> = stdout
        .lines()
        .filter_map(|line| part(line, ": NOT RUN"))
        .collect();
    if !failed.is_empty() {
        return Err(format!(
            "failed: {}; not run: {}",
            failed.join(", "),
            not_run.join(", ")
        ));
    }
    if !not_run.is_empty() {
        return Err(format!("{HARNESS_NOT_RUN} {}", not_run.join(", ")));
    }
    if !stdout.lines().any(|line| line.starts_with("HARNESS: PASS")) {
        return Err("the harness did not print HARNESS: PASS".to_owned());
    }
    exit_result(success)
}

/// Takes the result of the earlier part whose row starts with `part`.
fn same_as(steps: &mut Vec<Step>, name: &str, part: &str) {
    match steps.iter().find(|step| step.name.starts_with(part)) {
        Some(step) => {
            let status = step.status;
            println!("{} 0.00s {name}", status.label());
            steps.push(Step {
                name: name.to_owned(),
                duration: Duration::ZERO,
                status,
            });
        }
        None => not_run(steps, name, "the part that runs it did not run"),
    }
}

fn host_label(step: &CommandStep) -> &str {
    step.name.split(':').next().unwrap_or(&step.name)
}

fn exit_result(success: bool) -> Result<(), String> {
    if success {
        Ok(())
    } else {
        Err("the command failed".into())
    }
}

/// `dotnet test` exits 0 when no test ran, and then prints no total line. A real
/// run on .NET 10.0.403 did this both with a filter that matches nothing and with
/// a project that has no test class.
fn dotnet_result(success: bool, stdout: &str) -> Result<(), String> {
    exit_result(success)?;
    let summaries: Vec<&str> = stdout
        .lines()
        .filter(|line| line.contains("Failed:") && line.contains("Total:"))
        .collect();
    if summaries.is_empty() {
        return Err("dotnet test printed no total line, so no test ran".into());
    }
    for line in summaries {
        if summary_count(line, "Total:") == Some("0") {
            return Err("dotnet test ran Total: 0 tests".into());
        }
        // A run where every test is skipped proves nothing either.
        match summary_count(line, "Passed:") {
            Some("0") => return Err("dotnet test ran with Passed: 0".into()),
            None => return Err("dotnet test printed no Passed count".into()),
            Some(_) => {}
        }
    }
    Ok(())
}

/// The number after a label such as `Total:` in `dotnet test`'s summary line.
fn summary_count<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let rest = line.split(label).nth(1)?;
    Some(rest.split(',').next()?.trim())
}

/// The line that every good host run prints near its end. `desktop-close: PASS`
/// comes after it, but a good run can print a warning to stderr in its place.
const HOST_END_LINE: &str = "validation-errors: PASS";

/// The host exits 0 even when it did not run a scenario, so its output decides
/// the part. On WARP any `NOT RUN` line fails. On the RTX the WARP-only scenarios
/// are not run: the names come back for a SKIP row, and any other reason for a
/// `NOT RUN` line fails. A run with no end line did not finish its checks.
fn host_result(success: bool, stdout: &str, warp: bool) -> (Result<(), String>, Option<String>) {
    const RTX_REASON: &str = "NOT RUN (OP_GPU_ADAPTER is not warp)";
    let lines: Vec<(&str, &str)> = stdout
        .lines()
        .filter_map(|line| line.split_once(": NOT RUN"))
        .collect();
    let names = |list: &[(&str, &str)]| {
        list.iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>()
            .join(", ")
    };
    if let Err(reason) = exit_result(success) {
        return (Err(reason), None);
    }
    if warp && !lines.is_empty() {
        return (Err(format!("not run on WARP: {}", names(&lines))), None);
    }
    let unexpected: Vec<(&str, &str)> = stdout
        .lines()
        .filter(|line| line.contains(": NOT RUN") && !line.ends_with(RTX_REASON))
        .filter_map(|line| line.split_once(": NOT RUN"))
        .collect();
    if !unexpected.is_empty() {
        return (
            Err(format!(
                "not run for another reason: {}",
                names(&unexpected)
            )),
            None,
        );
    }
    if !stdout.lines().any(|line| line.starts_with(HOST_END_LINE)) {
        return (
            Err(format!("the host did not print its {HOST_END_LINE} line")),
            None,
        );
    }
    if lines.is_empty() {
        return (Ok(()), None);
    }
    (Ok(()), Some(names(&lines)))
}

fn run_part(steps: &mut Vec<Step>, part: &Part, place: &RunPlace) {
    match part {
        Part::Command(step) => run_step(steps, step, place),
        Part::CrateCommand(args) => {
            let name = part.name();
            let runs_cargo_test = args.first().is_some_and(|verb| verb == "test");
            if stopped_by_shell(steps, &name, runs_cargo_test, false, place.shell_env) {
                return;
            }
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            run(steps, &name, place.engine, "cargo", &args);
        }
        Part::Skip { name, reason } => skip(steps, name, reason),
        Part::Fail { name, reason } => fail(steps, name, reason),
        Part::NotRun { name, reason } => not_run(steps, name, reason),
        Part::Unreached(unreached) => record_unreached(steps, *unreached),
        Part::Scenarios => run_scenarios(steps, place.engine, place.repo),
        Part::Script(script) => run_script(steps, script, place.repo, None),
        Part::SameAs { name, part } => same_as(steps, name, part),
    }
}

fn run_scenarios(steps: &mut Vec<Step>, engine: &Path, repo: &Path) {
    let scenario_dir = repo.join("tests/scenarios");
    let mut scenarios = match fs::read_dir(&scenario_dir) {
        Ok(entries) => {
            let mut paths = Vec::new();
            for entry in entries {
                match entry {
                    Ok(entry) => {
                        let path = entry.path();
                        if path
                            .extension()
                            .is_some_and(|extension| extension == "json")
                        {
                            paths.push(path);
                        }
                    }
                    Err(error) => fail(steps, "opcli scenarios", &error.to_string()),
                }
            }
            paths
        }
        Err(error) => {
            fail(
                steps,
                "opcli scenarios",
                &format!("cannot read {}: {error}", scenario_dir.display()),
            );
            Vec::new()
        }
    };
    scenarios.sort();
    if scenarios.is_empty()
        && !steps
            .iter()
            .any(|step| step.name == "opcli scenarios" && matches!(step.status, Status::Fail))
    {
        skip(steps, "opcli scenarios", "no tests/scenarios/*.json files");
    }
    for scenario in scenarios {
        let name = format!("opcli scenario {}", scenario.display());
        let argument = scenario.to_string_lossy();
        let out = std::env::temp_dir().join(format!(
            "op-scenario-{}-{}",
            std::process::id(),
            scenario.file_stem().unwrap_or_default().to_string_lossy()
        ));
        let out = out.to_string_lossy();
        run(
            steps,
            &name,
            engine,
            "cargo",
            &[
                "run", "--quiet", "-p", "op-cli", "--", "scenario", &argument, "-o", &out,
            ],
        );
    }
}

const ALL_CHANGED: &str = "--all-changed";
const USAGE: &str = "Usage: cargo xtask check --tier fast|milestone|window|checkpoint|speed [--all-changed, with fast only]";

/// Reads the `check` arguments into the level and whether Git is left out.
fn parse_check_args(args: &[String]) -> Result<(&str, bool), String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (tier, all_changed) = match args.as_slice() {
        ["check", "--tier", tier] => (*tier, false),
        ["check", "--tier", tier, flag] if *flag == ALL_CHANGED => (*tier, true),
        _ => return Err(USAGE.to_owned()),
    };
    if !matches!(
        tier,
        "fast" | "milestone" | "window" | "checkpoint" | "speed"
    ) {
        return Err(format!("Unknown tier: {tier}"));
    }
    if all_changed && tier != "fast" {
        return Err(format!(
            "{ALL_CHANGED} works only with --tier fast, and this is {tier}"
        ));
    }
    Ok((tier, all_changed))
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("fuzz") {
        let engine = engine_dir(
            std::env::var_os("CARGO_MANIFEST_DIR"),
            env!("CARGO_MANIFEST_DIR"),
        );
        return fuzz::main(&engine, &args[1..]);
    }
    let (tier, all_changed) = match parse_check_args(&args) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    if tier == "speed" || (tier == "window" && Os::current() != Os::Windows) {
        let mut steps = Vec::new();
        for part in pending_parts(tier) {
            not_run(&mut steps, part, "not implemented");
        }
        println!("\nSummary");
        for step in &steps {
            println!(
                "{} {:.2}s {}",
                step.status.label(),
                step.duration.as_secs_f64(),
                step.name
            );
        }
        return ExitCode::FAILURE;
    }
    let engine = engine_dir(
        std::env::var_os("CARGO_MANIFEST_DIR"),
        env!("CARGO_MANIFEST_DIR"),
    );
    let repo = engine.parent().expect("repository parent");
    let os = Os::current();
    let start = Instant::now();
    let mut steps = Vec::new();
    let plan = if tier != "fast" {
        Ok(FastPlan::default())
    } else if all_changed {
        whole_tree_plan(&engine)
    } else {
        fast_plan(&engine, repo)
    };
    if tier == "fast"
        && let Ok(plan) = &plan
    {
        print_fast_plan(plan);
    }
    let shell_env =
        |name: &str| std::env::var_os(name).map(|value| value.to_string_lossy().into_owned());
    let place = RunPlace {
        engine: &engine,
        repo,
        shell_env: &shell_env,
    };
    let has_project = repo.join("mac/project.yml").is_file();
    let flow_list = std::fs::read_to_string(repo.join(FLOW_LIST)).ok();
    for part in with_flows(
        run_parts(tier, os, has_project, &plan),
        flow_list.as_deref(),
    ) {
        run_part(&mut steps, &part, &place);
    }
    println!("\nSummary");
    println!("{:<8} {:>10}  Step", "Result", "Duration");
    for step in &steps {
        println!(
            "{:<8} {:>8.2}s  {}",
            step.status.label(),
            step.duration.as_secs_f64(),
            step.name
        );
    }
    for step in &steps {
        if matches!(step.status, Status::Owed) {
            println!("Owed: {}", step.name);
        }
    }
    println!("{}", time_line(tier, start.elapsed().as_secs_f64()));
    if tier_failed(&steps) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn tier_failed(steps: &[Step]) -> bool {
    steps
        .iter()
        .any(|step| matches!(step.status, Status::Fail | Status::NotRun))
}

fn pending_parts(tier: &str) -> &'static [&'static str] {
    match tier {
        "window" => &[
            "critical flows in real windows",
            "one shot per changed screen",
        ],
        "speed" => &["launch", "app size", "file open", "blur", "brush"],
        _ => &[],
    }
}

fn not_run(steps: &mut Vec<Step>, name: &str, reason: &str) {
    println!("NOT RUN 0.00s {name}: {reason}");
    steps.push(Step {
        name: name.into(),
        duration: Duration::ZERO,
        status: Status::NotRun,
    });
}

fn run(steps: &mut Vec<Step>, name: &str, dir: &Path, program: &str, args: &[&str]) {
    run_with(steps, name, dir, program, args, &[], false);
}

fn run_with(
    steps: &mut Vec<Step>,
    name: &str,
    dir: &Path,
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
    skip_if_missing: bool,
) {
    println!("Running {name}");
    let start = Instant::now();
    let mut command = Command::new(program);
    command.args(args).current_dir(dir);
    for (key, value) in env {
        command.env(key, value);
    }
    let status = match command.status() {
        Ok(exit) if exit.success() => Status::Pass,
        Ok(exit) => {
            eprintln!("{name} exited with {exit}");
            Status::Fail
        }
        Err(error) if skip_if_missing && error.kind() == std::io::ErrorKind::NotFound => {
            skip(steps, name, &format!("{program} is not installed"));
            return;
        }
        Err(error) => {
            eprintln!("{name} could not start: {error}");
            Status::Fail
        }
    };
    let duration = start.elapsed();
    println!("{} {:.2}s {name}", status.label(), duration.as_secs_f64());
    steps.push(Step {
        name: name.into(),
        duration,
        status,
    });
}

fn skip(steps: &mut Vec<Step>, name: &str, reason: &str) {
    println!("SKIP 0.00s {name}: {reason}");
    steps.push(Step {
        name: format!("{name}: {reason}"),
        duration: Duration::ZERO,
        status: Status::Skip,
    });
}

fn fail(steps: &mut Vec<Step>, name: &str, reason: &str) {
    eprintln!("FAIL 0.00s {name}: {reason}");
    steps.push(Step {
        name: name.into(),
        duration: Duration::ZERO,
        status: Status::Fail,
    });
}

/// Scripts the checkpoint tier runs from the repository root on macOS (M58).
/// `scripts/perf-check.sh` builds the Release app fresh, then fails on the
/// size and launch budgets, an effect pipeline before `OP_READY`, a first
/// effect off the GPU, or a regression over `perf/baseline.toml`.
fn checkpoint_scripts() -> [&'static str; 1] {
    ["scripts/perf-check.sh"]
}

fn script_status(success: bool, stdout: &str) -> Status {
    if !success {
        Status::Fail
    } else if stdout.lines().any(|line| line.starts_with("SKIP ")) {
        Status::Skip
    } else {
        Status::Pass
    }
}

fn run_script(steps: &mut Vec<Step>, name: &str, repo: &Path, app: Option<&Path>) {
    println!("Running {name}");
    let start = Instant::now();
    let mut command = Command::new("bash");
    command.arg(name).current_dir(repo);
    if let Some(path) = app {
        command.arg(path);
    }
    let status = match command.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            print!("{stdout}");
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
            script_status(output.status.success(), &stdout)
        }
        Err(error) => {
            eprintln!("{name} could not start: {error}");
            Status::Fail
        }
    };
    let duration = start.elapsed();
    println!("{} {:.2}s {name}", status.label(), duration.as_secs_f64());
    steps.push(Step {
        name: name.into(),
        duration,
        status,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn run_part_names(tier: &str, os: Os) -> Vec<String> {
        names(&run_parts(tier, os, true, &Ok(FastPlan::default())))
    }

    const AOT_RUN_PART: &str = "headless UI on the AOT build: all the app's headless tests";
    const AOT_PUBLISH_PART: &str = "publish the AOT headless tests: dotnet publish";

    #[test]
    fn milestone_on_windows_runs_the_aot_tests_and_prints_no_not_run_row_for_them() {
        let parts = run_parts("milestone", Os::Windows, true, &Ok(FastPlan::default()));
        let all = names(&parts);
        assert!(!all.iter().any(|n| n == "headless UI on the AOT build"));
        let publish = all.iter().position(|n| n == AOT_PUBLISH_PART).unwrap();
        assert_eq!(all[publish + 1], AOT_RUN_PART);
        // The tests read the release DLL's export table, so the DLL comes first.
        let dll = all.iter().position(|n| n == DLL_PART).unwrap();
        assert!(dll < publish);
        assert!(matches!(parts[publish], Part::Command(_)));
        assert!(matches!(parts[publish + 1], Part::Command(_)));
    }

    #[test]
    fn with_no_flow_list_the_critical_flows_row_stays_not_run_and_fails_the_milestone() {
        let parts = run_parts("milestone", Os::Windows, true, &Ok(FastPlan::default()));
        let row = parts
            .iter()
            .find(|part| part.name() == "9 to 12 critical flows")
            .unwrap();
        let Part::Unreached(row) = row else {
            panic!("the flows must stay a NOT RUN row: {row:?}");
        };
        assert!(!row.owed);
        assert!(row.reason.contains("W8 to W10"), "{}", row.reason);
    }

    fn flow_table(rows: usize) -> String {
        let mut text = String::from(
            "| # | Flow | Steps | Possible from | How | Runs now |\n| --- | --- | --- | --- | --- | --- |\n",
        );
        for i in 1..=rows {
            let (milestone, runs) = if i % 2 == 0 {
                ("W8", "yes")
            } else {
                ("W9", "no")
            };
            text.push_str(&format!(
                "| {i} | Flow {i} | a, b | {milestone} | headless | {runs} |\n"
            ));
        }
        text
    }

    #[test]
    fn a_flow_list_replaces_the_one_row_with_a_row_per_flow() {
        let parts = run_parts("milestone", Os::Windows, true, &Ok(FastPlan::default()));
        let table = flow_table(10);
        let parts = with_flows(parts, Some(&table));
        let all = names(&parts);
        assert!(!all.iter().any(|n| n == FLOW_ROW), "{all:?}");
        assert_eq!(all.iter().filter(|n| n.starts_with("flow ")).count(), 10);
        // The flows run on the AOT build, so they come after its publish.
        let publish = all.iter().position(|n| n == AOT_PUBLISH_PART).unwrap();
        let first = all.iter().position(|n| n.starts_with("flow ")).unwrap();
        assert!(publish < first);
    }

    #[test]
    fn a_flow_that_can_run_runs_its_own_test_and_any_other_prints_not_run_by_name() {
        let table = flow_table(9);
        let parts = flow_parts(&read_flows(&table).unwrap());
        let Part::Command(step) = &parts[1] else {
            panic!("flow 2 must run: {:?}", parts[1]);
        };
        assert_eq!(step.name, "flow 2: Flow 2");
        assert_eq!(step.program, AOT_TESTS_PROGRAM);
        assert_eq!(step.args, ["--only", "FlowTests.Flow02"]);
        assert_eq!(step.output, OutputRule::DotnetTotal);
        let Part::NotRun { name, reason } = &parts[0] else {
            panic!("flow 1 must not run: {:?}", parts[0]);
        };
        assert_eq!(name, "flow 1: Flow 1");
        assert!(reason.contains("W9"), "{reason}");
    }

    #[test]
    fn a_flow_list_of_the_wrong_size_or_shape_fails_the_level_by_name() {
        for table in [
            flow_table(8),
            flow_table(13),
            flow_table(9).replace("| yes |", "| maybe |"),
            flow_table(9).replace("| headless |", ""),
        ] {
            let parts = with_flows(
                vec![Part::Unreached(fails_level(FLOW_ROW, "no list"))],
                Some(&table),
            );
            assert!(
                matches!(&parts[..], [Part::Fail { name, .. }] if name == FLOW_ROW),
                "{parts:?}"
            );
        }
    }

    #[test]
    fn with_no_flow_file_the_row_stays() {
        let parts = run_parts("milestone", Os::Windows, true, &Ok(FastPlan::default()));
        let before = names(&parts);
        assert_eq!(names(&with_flows(parts, None)), before);
    }

    #[test]
    fn the_checked_in_flow_list_reads_and_only_the_milestone_level_takes_it() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let text = std::fs::read_to_string(repo.join(FLOW_LIST)).unwrap();
        let flows = read_flows(&text).unwrap();
        assert!(flows.iter().any(|f| f.runs_now));
        assert!(flows.iter().any(|f| !f.runs_now));
        let fast = run_parts("fast", Os::Windows, true, &Ok(FastPlan::default()));
        let before = names(&fast);
        assert_eq!(names(&with_flows(fast, Some(&text))), before);
    }

    #[test]
    fn checkpoint_on_windows_runs_the_aot_tests_too() {
        let all = run_part_names("checkpoint", Os::Windows);
        let run = all.iter().position(|n| n == AOT_RUN_PART).unwrap();
        let first_unreached = all
            .iter()
            .position(|n| n == "all real-window flows")
            .unwrap();
        assert!(run < first_unreached);
    }

    #[test]
    fn the_aot_tests_are_not_on_mac_or_linux() {
        for os in [Os::Macos, Os::Linux] {
            let parts = run_parts("milestone", os, true, &Ok(FastPlan::default()));
            let row = parts
                .iter()
                .find(|part| part.name() == "headless UI on the AOT build")
                .unwrap();
            assert!(matches!(row, Part::Skip { .. }), "{row:?}");
            assert!(!names(&parts).iter().any(|n| n == AOT_RUN_PART));
        }
    }

    #[test]
    fn the_aot_run_counts_only_with_tests_in_its_total() {
        let steps = aot_headless_steps();
        assert_eq!(steps[0].output, OutputRule::AotPublish);
        assert_eq!(steps[1].output, OutputRule::DotnetTotal);
        assert_eq!(steps[1].program, AOT_TESTS_PROGRAM);
        assert!(steps[0].args.contains(&AOT_TESTS_FOLDER));
        assert!(
            AOT_TESTS_PROGRAM.starts_with(AOT_TESTS_FOLDER),
            "the program runs from the folder the publish fills"
        );
    }

    #[test]
    fn the_aot_runners_total_line_reads_as_a_dotnet_total() {
        const LINE: &str =
            "Failed: 0, Passed: 187, Skipped: 0, Total: 187, 143 test methods on the AOT build\n";
        assert_eq!(dotnet_result(true, LINE), Ok(()));
        assert!(dotnet_result(false, LINE).is_err());
        assert!(
            dotnet_result(
                true,
                "Failed: 0, Passed: 0, Skipped: 0, Total: 0, 0 test methods\n"
            )
            .is_err()
        );
    }

    #[test]
    fn a_program_with_a_folder_is_found_from_the_steps_place() {
        let dir = Path::new("repo");
        assert_eq!(program_path("dotnet", dir), PathBuf::from("dotnet"));
        assert_eq!(program_path("a/b.exe", dir), dir.join("a/b.exe"));
    }

    #[test]
    fn the_aot_publish_finds_the_visual_studio_installer_folder_only_when_it_exists() {
        let root = std::env::temp_dir().join(format!("op-xtask-vs-{}", std::process::id()));
        let installer = root.join("Microsoft Visual Studio").join("Installer");
        fs::create_dir_all(&installer).unwrap();
        let first = std::env::temp_dir();
        let path = path_with_vs_installer(Some(first.as_os_str()), Some(root.as_os_str())).unwrap();
        let folders: Vec<_> = std::env::split_paths(&path).collect();
        assert_eq!(folders, [installer, first]);
        fs::remove_dir_all(&root).unwrap();
        assert!(path_with_vs_installer(None, Some(root.as_os_str())).is_none());
        assert!(path_with_vs_installer(None, None).is_none());
    }

    #[test]
    fn the_window_level_on_windows_builds_the_aot_app_and_runs_the_harness_last() {
        let parts = run_parts("window", Os::Windows, true, &Ok(FastPlan::default()));
        let all = names(&parts);
        assert_eq!(all[0], DLL_PART);
        assert_eq!(all[1], "publish the AOT app: dotnet publish");
        assert_eq!(all[2], "build the harness: dotnet build");
        assert!(all[3].starts_with("real-window harness"));
        assert_eq!(all.len(), 4);
        let Part::Command(harness) = &parts[3] else {
            panic!("the harness must be a command");
        };
        assert_eq!(harness.output, OutputRule::Harness);
        assert!(harness.args.contains(&APP_PROGRAM));
        // A short or busy run must never count at this level.
        for switch in ["--pan-seconds", "--allow-busy", "--ignore-disk", "--parts"] {
            assert!(!harness.args.contains(&switch), "{switch}");
        }
    }

    #[test]
    fn harness_lines_decide_the_row() {
        const GOOD: &str =
            "UI TESTS: PASS\npan trace: PASS (average 60.0 fps)\nHARNESS: PASS (1 parts)\n";
        assert_eq!(harness_result(true, GOOD), Ok(()));

        let failed = harness_result(
            false,
            "UI TESTS: PASS\npan trace: FAIL (a stall)\npanels: NOT RUN (covered)\nHARNESS: FAIL (pan trace, panels)\n",
        );
        let reason = failed.unwrap_err();
        assert!(!reason.starts_with(HARNESS_NOT_RUN), "{reason}");
        assert!(
            reason.contains("pan trace") && reason.contains("panels"),
            "{reason}"
        );

        let locked = harness_result(
            false,
            "UI TESTS: NOT RUN (desktop is locked)\npan: NOT RUN (desktop is locked)\nHARNESS: FAIL (pan)\n",
        );
        assert!(locked.unwrap_err().starts_with(HARNESS_NOT_RUN));

        // Exit 0 with no last line did not finish, and a bad exit is never a pass.
        assert!(harness_result(true, "pan trace: PASS\n").is_err());
        assert!(harness_result(false, GOOD).is_err());
        // An indented note holding the word is not a part.
        assert_eq!(
            harness_result(true, &format!("  note: FAIL words\n{GOOD}")),
            Ok(())
        );
    }

    #[test]
    fn a_harness_row_that_did_not_run_prints_not_run_and_fails_the_level() {
        let mut steps = Vec::new();
        let dir = dir_for_tests();
        let step = echo_step("pan: NOT RUN (locked)", OutputRule::Harness, &[]);
        run_step(&mut steps, &step, &test_place(&dir));
        assert_eq!(steps[0].status.label(), "NOT RUN");
        assert!(tier_failed(&steps));
    }

    #[test]
    fn the_fast_level_names_the_headless_ui_row_inside_the_app_tests_on_windows() {
        let plan = Ok(FastPlan {
            app: changed::APP_TESTS.iter().collect(),
            ..FastPlan::default()
        });
        let parts = fast_parts(Os::Windows, true, &plan);
        let all = names(&parts);
        let app = all.iter().position(|n| n == APP_TEST_PART).unwrap();
        assert_eq!(
            all[app + 1],
            "headless UI for the changed area, inside Windows app tests"
        );
    }

    #[test]
    fn the_fast_row_takes_the_app_tests_result_and_is_not_run_without_them() {
        let same = Part::SameAs {
            name: "row".into(),
            part: "Windows app tests",
        };
        let dir = dir_for_tests();
        let place = test_place(&dir);

        let mut none = Vec::new();
        run_part(&mut none, &same, &place);
        assert_eq!(none[0].status.label(), "NOT RUN");

        for (status, label) in [(Status::Pass, "PASS"), (Status::Fail, "FAIL")] {
            let mut steps = vec![Step {
                name: "Windows app tests: dotnet test".into(),
                duration: Duration::ZERO,
                status,
            }];
            run_part(&mut steps, &same, &place);
            assert_eq!(steps[1].status.label(), label);
        }
    }

    #[test]
    fn window_and_speed_parts_are_pending_and_fail_the_tier() {
        assert_eq!(
            pending_parts("window"),
            [
                "critical flows in real windows",
                "one shot per changed screen"
            ]
        );
        assert_eq!(
            pending_parts("speed"),
            ["launch", "app size", "file open", "blur", "brush"]
        );
        for tier in ["window", "speed"] {
            let mut steps = Vec::new();
            for part in pending_parts(tier) {
                not_run(&mut steps, part, "not implemented");
            }
            assert!(steps.iter().all(|s| matches!(s.status, Status::NotRun)));
            assert!(tier_failed(&steps));
        }
    }

    #[test]
    fn fast_tier_runs_only_the_base_checks() {
        assert!(deep_steps("fast").is_empty());
        assert_eq!(proptest_cases("fast"), None);
        let names: Vec<_> = base_steps().into_iter().map(|step| step.name).collect();
        assert_eq!(
            names,
            [
                "cargo fmt --all --check",
                "cargo clippy --workspace --all-targets -- -D warnings",
                "cargo test --workspace --no-fail-fast",
            ]
        );
    }

    #[test]
    fn milestone_tier_runs_1024_cases_and_the_oracle_tests() {
        let steps = deep_steps("milestone");
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].program, "cargo");
        assert_eq!(
            steps[0].args,
            ["test", "--workspace", "--release", "--no-fail-fast"]
        );
        assert_eq!(steps[0].env, [("OP_PROPTEST_CASES", "1024")]);
        assert_eq!(
            steps[0].name,
            "OP_PROPTEST_CASES=1024 cargo test --workspace --release --no-fail-fast"
        );
        assert_eq!(steps[1].program, "python3");
        assert_eq!(
            steps[1].args,
            ["-B", "tests/oracle/legacy336/test_oracle.py"]
        );
        assert!(steps[1].in_repo && steps[1].skip_if_missing);
        assert!(!steps.iter().any(|step| step.args.contains(&"tile_bench")));
    }

    #[test]
    fn checkpoint_tier_runs_the_m57_robustness_steps() {
        assert!(robustness_steps("fast").is_empty());
        assert!(robustness_steps("milestone").is_empty());
        let steps = robustness_steps("checkpoint");
        let args: Vec<Vec<&str>> = steps.iter().map(|s| s.args.clone()).collect();
        assert_eq!(
            args,
            [
                vec![
                    "run",
                    "--quiet",
                    "-p",
                    "xtask",
                    "--",
                    "fuzz",
                    "--seconds",
                    "60"
                ],
                vec![
                    "test",
                    "-p",
                    "op-engine",
                    "--release",
                    "--test",
                    "huge_document",
                    "--",
                    "--ignored"
                ],
                vec![
                    "test",
                    "-p",
                    "op-engine",
                    "--test",
                    "low_disk",
                    "--",
                    "--ignored"
                ],
            ]
        );
        assert!(steps.iter().all(|s| !s.in_repo && !s.skip_if_missing));
    }

    #[test]
    fn checkpoint_tier_runs_10000_cases_oracle_tests_and_tile_bench() {
        let steps = deep_steps("checkpoint");
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].env, [("OP_PROPTEST_CASES", "10000")]);
        assert_eq!(
            steps[0].args,
            ["test", "--workspace", "--release", "--no-fail-fast"]
        );
        assert_eq!(
            steps[1].args,
            ["-B", "tests/oracle/legacy336/test_oracle.py"]
        );
        assert_eq!(
            steps[2].args,
            [
                "run",
                "-p",
                "op-core",
                "--example",
                "tile_bench",
                "--release"
            ]
        );
        assert!(!steps[2].in_repo && !steps[2].skip_if_missing);
    }

    #[test]
    fn checkpoint_tier_runs_the_perf_check() {
        // M58: perf-check builds the Release app fresh and runs the size,
        // launch (with --first-effect), lazy-pipeline and regression checks.
        assert_eq!(checkpoint_scripts(), ["scripts/perf-check.sh"]);
    }

    #[test]
    fn a_missing_optional_program_is_a_skip_and_a_missing_required_one_fails() {
        let dir = std::env::temp_dir();
        let missing = "op-xtask-no-such-program";
        let mut steps = Vec::new();
        run_with(&mut steps, "optional", &dir, missing, &[], &[], true);
        run_with(&mut steps, "required", &dir, missing, &[], &[], false);
        assert!(matches!(steps[0].status, Status::Skip));
        assert!(matches!(steps[1].status, Status::Fail));
    }

    #[test]
    fn step_environment_reaches_the_command() {
        let dir = std::env::temp_dir();
        let mut steps = Vec::new();
        run_with(
            &mut steps,
            "env",
            &dir,
            if cfg!(windows) { "cmd" } else { "sh" },
            if cfg!(windows) {
                &[
                    "/C",
                    "if %OP_PROPTEST_CASES%==1024 (exit /b 0) else (exit /b 1)",
                ]
            } else {
                &["-c", "test \"$OP_PROPTEST_CASES\" = 1024"]
            },
            &[("OP_PROPTEST_CASES", "1024")],
            false,
        );
        assert!(matches!(steps[0].status, Status::Pass));
    }

    #[test]
    fn engine_dir_prefers_the_run_time_manifest_path() {
        let dir = engine_dir(Some("/fresh/engine/xtask".into()), "/stale/engine/xtask");
        assert_eq!(dir, PathBuf::from("/fresh/engine"));
        assert_eq!(
            engine_dir(None, "/stale/engine/xtask"),
            PathBuf::from("/stale/engine")
        );
    }

    #[test]
    fn script_skip_is_recorded_as_skip() {
        assert!(matches!(
            script_status(true, "SKIP size report: no app path\n"),
            Status::Skip
        ));
    }

    #[test]
    #[cfg(unix)]
    fn size_report_rejects_even_a_rounded_overrun() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let fixture = std::env::temp_dir().join(format!("op-size-test-{}", std::process::id()));
        let bin = fixture.join("bin");
        let app = fixture.join("Fake.app");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&app).unwrap();
        let fake_du = bin.join("du");
        fs::write(&fake_du, "#!/bin/sh\nprintf '76801\\t%s\\n' \"$2\"\n").unwrap();
        fs::set_permissions(&fake_du, fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        let output = Command::new("bash")
            .arg("scripts/size-report.sh")
            .arg(&app)
            .current_dir(repo)
            .env("PATH", path)
            .output()
            .unwrap();
        fs::remove_dir_all(&fixture).unwrap();
        assert!(
            !output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    /// Runs scripts/launch-time.sh (with `args`) on a fake app whose binary
    /// prints `ready` and then waits. Returns (success, stdout, stderr).
    #[cfg(target_os = "macos")]
    fn launch_script(tag: &str, ready: Option<&str>, args: &[&str]) -> (bool, String, String) {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let fixture =
            std::env::temp_dir().join(format!("op-launch-test-{tag}-{}", std::process::id()));
        let app = fixture.join("Fake.app");
        let contents = app.join("Contents");
        let macos = contents.join("MacOS");
        fs::create_dir_all(&macos).unwrap();
        fs::write(
            contents.join("Info.plist"),
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>Fake</string></dict></plist>",
        )
        .unwrap();
        let binary = macos.join("Fake");
        let script = match ready {
            Some(line) => format!("#!/bin/sh\necho '{line}'\nexec sleep 5\n"),
            None => "#!/bin/sh\nexec sleep 5\n".to_owned(),
        };
        fs::write(&binary, script).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let mut command = Command::new("bash");
        command.arg("scripts/launch-time.sh").args(["--runs", "3"]);
        command.args(args).current_dir(repo);
        if tag != "no-app" {
            command.arg(&app);
        }
        let output = command.output().unwrap();
        fs::remove_dir_all(&fixture).unwrap();
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_script_reads_five_ready_lines_without_a_temp_file() {
        // M58 fix: the fake app prints the pipeline counts the real app prints.
        let (ok, out, err) = launch_script("ok", Some("OP_READY 123 effect=0 view=1 mip=0"), &[]);
        assert!(ok, "{err}");
        assert!(out.contains("Cold launch median: 123 ms"), "{out}");
        assert!(out.contains("PASS lazy pipelines"), "{out}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_script_without_an_app_fails() {
        let (ok, out, err) = launch_script("no-app", None, &[]);
        assert!(!ok, "{out}");
        assert!(err.contains("Usage"), "{err}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_script_fails_without_pipeline_counts_in_every_mode() {
        let (ok, out, _) = launch_script("no-counts", Some("OP_READY 123"), &[]);
        assert!(!ok, "{out}");
        assert!(!out.contains("PASS launch time"), "{out}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_script_fails_when_no_canvas_view_is_ready() {
        // The empty start window prints view=0: it is not a usable canvas.
        let (ok, out, err) =
            launch_script("no-view", Some("OP_READY 123 effect=0 view=0 mip=0"), &[]);
        assert!(!ok, "{out}");
        assert!(err.contains("view"), "{err}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_script_fails_on_an_effect_pipeline_before_ready() {
        let (ok, out, err) =
            launch_script("early", Some("OP_READY 123 effect=2 view=1 mip=0"), &[]);
        assert!(!ok, "{out}");
        assert!(err.contains("FAIL lazy pipelines"), "{err}");
    }

    /// Runs scripts/perf-compare.sh on a run and a baseline that differ from
    /// the defaults by `run` and `base` (key, value) pairs.
    #[cfg(unix)]
    fn perf_compare(tag: &str, run: &[(&str, &str)], base: &[(&str, &str)]) -> (bool, String) {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let dir = std::env::temp_dir().join(format!("op-compare-{tag}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, changes: &[(&str, &str)]| {
            let mut values = [
                ("machine", "\"Test Mac\""),
                ("cache_purged", "0"),
                ("launch_median_ms", "1000"),
                ("launch_p95_ms", "2000"),
                ("first_effect_median_ms", "1100"),
                ("app_bundle_mb", "10.75"),
            ];
            for (key, value) in changes {
                values.iter_mut().find(|(k, _)| k == key).unwrap().1 = value;
            }
            let text: String = values.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
            let path = dir.join(name);
            fs::write(&path, text).unwrap();
            path
        };
        let numbers = write("numbers.toml", run);
        let baseline = write("baseline.toml", base);
        let output = Command::new("bash")
            .arg("scripts/perf-compare.sh")
            .arg(&numbers)
            .arg(&baseline)
            .current_dir(repo)
            .output()
            .unwrap();
        fs::remove_dir_all(&dir).unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        (output.status.success(), text)
    }

    #[test]
    #[cfg(unix)]
    fn perf_compare_passes_within_ten_percent() {
        let (ok, text) = perf_compare("ok", &[("launch_p95_ms", "2199")], &[]);
        assert!(ok, "{text}");
        for name in [
            "launch median",
            "launch p95",
            "first GPU effect median",
            "app bundle",
        ] {
            assert!(text.contains(&format!("PASS regression: {name}")), "{text}");
        }
    }

    #[test]
    #[cfg(unix)]
    fn perf_compare_fails_on_a_launch_p95_regression() {
        let (ok, text) = perf_compare("p95", &[("launch_p95_ms", "2201")], &[]);
        assert!(
            !ok && text.contains("FAIL regression: launch p95"),
            "{text}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn perf_compare_fails_on_a_first_effect_regression() {
        let (ok, text) = perf_compare("effect", &[("first_effect_median_ms", "1300")], &[]);
        assert!(
            !ok && text.contains("FAIL regression: first GPU effect median"),
            "{text}"
        );
    }

    #[test]
    #[cfg(unix)]
    fn perf_compare_refuses_a_purged_run_against_an_unpurged_baseline() {
        let (ok, text) = perf_compare("purge", &[("cache_purged", "1")], &[]);
        assert!(!ok && text.contains("do not compare"), "{text}");
        let (ok, text) = perf_compare("purge2", &[], &[("cache_purged", "1")]);
        assert!(!ok && text.contains("do not compare"), "{text}");
    }

    #[test]
    #[cfg(unix)]
    fn perf_compare_refuses_another_machine() {
        let (ok, text) = perf_compare("machine", &[], &[("machine", "\"Other Mac\"")]);
        assert!(!ok && text.contains("the baseline is from"), "{text}");
    }

    #[test]
    fn fast_runs_clippy_and_tests_on_the_changed_crates_only() {
        let crates = ["op-color".to_owned(), "op-core".to_owned()];
        assert_eq!(
            fast_crate_commands(&crates),
            [
                vec![
                    "clippy",
                    "-p",
                    "op-color",
                    "-p",
                    "op-core",
                    "--no-deps",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings"
                ],
                vec!["test", "-p", "op-color", "-p", "op-core", "--no-fail-fast"],
            ]
        );
    }

    #[test]
    fn fast_with_no_changed_crate_runs_no_cargo_clippy_or_test() {
        assert!(fast_crate_commands(&[]).is_empty());
        assert_eq!(fmt_step().name, "cargo fmt --all --check");
    }

    #[test]
    fn a_git_failure_fails_the_fast_level_by_name_and_tests_nothing() {
        // The repository has no commit, so `git merge-base` must fail.
        let dir = std::env::temp_dir().join(format!("op-xtask-fast-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .unwrap();
        assert!(init.success());
        let engine = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let reason = fast_plan(engine, &dir).unwrap_err();
        fs::remove_dir_all(&dir).unwrap();
        assert!(reason.contains("git merge-base"), "{reason}");

        let parts = fast_parts(Os::Windows, false, &Err(reason));
        assert_eq!(
            names(&parts),
            [
                "cargo fmt --all --check",
                "changed files",
                "changed crates",
                "changed C# tests",
                "Swift build",
            ]
        );
        let mut steps = Vec::new();
        for part in &parts[1..4] {
            run_part(&mut steps, part, &test_place(&dir_for_tests()));
        }
        assert_eq!(steps[0].name, "changed files");
        assert!(matches!(steps[0].status, Status::Fail));
        assert!(matches!(steps[1].status, Status::NotRun));
        assert!(matches!(steps[2].status, Status::NotRun));
        assert!(tier_failed(&steps));
    }

    #[test]
    fn a_level_passes_when_every_part_passed_or_was_skipped() {
        let step = |status| Step {
            name: "part".into(),
            duration: Duration::ZERO,
            status,
        };
        assert!(!tier_failed(&[step(Status::Pass), step(Status::Skip)]));
    }

    #[test]
    fn a_not_run_part_fails_the_level() {
        for (tier, count) in [("milestone", 2), ("checkpoint", 4)] {
            let parts = unreached_parts(tier, true);
            assert_eq!(parts.len(), count, "{tier}");
            let mut steps = Vec::new();
            for part in parts {
                record_unreached(&mut steps, part);
            }
            assert!(steps.iter().all(|step| step.status.label() == "NOT RUN"));
            assert!(tier_failed(&steps), "{tier}");
        }
    }

    #[test]
    fn the_comparison_is_owed_on_mac_and_linux_and_does_not_fail_the_level() {
        let parts = unreached_parts("checkpoint", false);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "paint.net comparison");
        let mut steps = Vec::new();
        record_unreached(&mut steps, parts[0]);
        assert_eq!(steps[0].status.label(), "NOT RUN");
        assert!(matches!(steps[0].status, Status::Owed));
        assert!(!tier_failed(&steps));
    }

    #[test]
    fn the_comparison_fails_the_level_on_windows() {
        let parts = unreached_parts("checkpoint", true);
        let part = parts
            .iter()
            .find(|part| part.name == "paint.net comparison")
            .unwrap();
        assert!(!part.owed);
        let mut steps = Vec::new();
        record_unreached(&mut steps, *part);
        assert!(tier_failed(&steps));
    }

    #[test]
    fn only_the_comparison_can_be_owed() {
        for tier in ["fast", "milestone", "checkpoint"] {
            for windows in [true, false] {
                for part in unreached_parts(tier, windows) {
                    assert!(!part.owed || part.name == "paint.net comparison");
                }
            }
        }
    }

    #[test]
    fn the_pending_parts_are_named_on_windows_and_absent_elsewhere() {
        let names = |tier, windows| -> Vec<&str> {
            unreached_parts(tier, windows)
                .iter()
                .map(|part| part.name)
                .collect()
        };
        assert_eq!(
            names("milestone", true),
            ["headless UI on the AOT build", "9 to 12 critical flows"]
        );
        assert_eq!(
            names("checkpoint", true),
            [
                "all real-window flows",
                "paint.net comparison",
                "Mac run",
                "Linux run"
            ]
        );
        assert!(names("fast", true).is_empty());
        assert!(names("fast", false).is_empty());
        assert!(names("milestone", false).is_empty());
    }

    #[test]
    fn milestone_and_checkpoint_run_the_three_csharp_projects_on_windows_only() {
        for tier in ["milestone", "checkpoint"] {
            let steps = csharp_steps(tier, true);
            let projects: Vec<_> = steps.iter().map(|step| step.args[1]).collect();
            assert_eq!(
                projects,
                [
                    "tools/pdn-capture/tests/PdnCaptureTests.csproj",
                    "tools/ui-spike/tests/SpikeTests.csproj",
                    "tools/ui-look/tests/UiLookTests.csproj",
                ]
            );
            assert!(steps.iter().all(|step| step.program == "dotnet"
                && step.args[0] == "test"
                && step.in_repo
                && !step.skip_if_missing));
            assert!(steps[0].name.starts_with("capture-kit tests: "));
            assert!(csharp_steps(tier, false).is_empty());
        }
        assert!(csharp_steps("fast", true).is_empty());
    }

    fn all_names(tier: &str, os: Os) -> Vec<String> {
        names(&all_level_parts(tier, os, true, &Ok(FastPlan::default())))
    }

    const APP_TEST_PART: &str =
        "Windows app tests: dotnet test windows/OpenPainter.Tests/OpenPainter.Tests.csproj";
    const DLL_PART: &str = "release op_ffi.dll: cargo build -p op-ffi --release";

    #[test]
    fn milestone_and_checkpoint_run_the_windows_app_tests_after_the_tool_projects() {
        for tier in ["milestone", "checkpoint"] {
            let names = all_names(tier, Os::Windows);
            let look = names
                .iter()
                .position(|n| n.starts_with("ui-look tests"))
                .unwrap();
            assert_eq!(names[look + 1], DLL_PART, "{tier}");
            assert_eq!(names[look + 2], APP_TEST_PART, "{tier}");
        }
    }

    #[test]
    fn the_windows_app_parts_add_to_the_level_and_change_nothing_else() {
        for tier in ["milestone", "checkpoint"] {
            let mut all = all_names(tier, Os::Windows);
            all.retain(|n| n != DLL_PART && n != APP_TEST_PART);
            let before = level_parts(tier, Os::Windows, true, &Ok(FastPlan::default()));
            assert_eq!(all, names(&before), "{tier}");
        }
    }

    #[test]
    fn the_windows_app_tests_count_only_with_tests_in_their_total() {
        let steps = app_steps("milestone", true);
        assert_eq!(steps.len(), 2);
        assert!(steps[1].output == OutputRule::DotnetTotal);
        assert!(!steps[1].skip_if_missing);
        assert!(steps[1].env.contains(&("AVALONIA_TELEMETRY_OPTOUT", "1")));
    }

    #[test]
    fn the_windows_app_parts_are_only_on_windows_and_only_at_the_two_levels() {
        assert!(app_steps("milestone", false).is_empty());
        assert!(app_steps("checkpoint", false).is_empty());
        assert!(app_steps("fast", true).is_empty());
        for os in [Os::Macos, Os::Linux] {
            let all = all_names("milestone", os);
            assert!(!all.iter().any(|n| n == APP_TEST_PART));
            assert!(all.iter().any(|n| n == "C# tests"));
        }
    }

    #[test]
    fn parts_the_level_does_not_reach_still_print_not_run_and_fail() {
        for (tier, name) in [
            ("milestone", "headless UI on the AOT build"),
            ("milestone", "9 to 12 critical flows"),
            ("checkpoint", "all real-window flows"),
        ] {
            let all = all_names(tier, Os::Windows);
            assert!(all.iter().any(|n| n == name), "{tier}: {name}");
            let unreached = all_level_parts(tier, Os::Windows, true, &Ok(FastPlan::default()))
                .into_iter()
                .filter(|part| matches!(part, Part::Unreached(_)))
                .count();
            assert!(unreached >= 2, "{tier}");
        }
    }

    /// The app's tests load the release DLL. A clean tree has none, and then 40 or more tests
    /// fail with a confusing error, so the fast level builds it first.
    #[test]
    fn a_fast_plan_that_reaches_the_app_builds_the_release_dll_before_its_tests() {
        let plan = Ok(FastPlan {
            app: changed::APP_TESTS.iter().collect(),
            ..FastPlan::default()
        });
        let all = names(&fast_parts(Os::Windows, true, &plan));
        let dll = all.iter().position(|n| n == DLL_PART).unwrap();
        let app = all.iter().position(|n| n == APP_TEST_PART).unwrap();
        assert!(dll < app, "{all:?}");
        assert_eq!(all.iter().filter(|n| *n == DLL_PART).count(), 1);
    }

    #[test]
    fn a_fast_plan_that_misses_the_app_or_runs_off_windows_builds_no_dll() {
        let none = Ok(FastPlan::default());
        assert!(
            !names(&fast_parts(Os::Windows, true, &none))
                .iter()
                .any(|n| n == DLL_PART)
        );
        let plan = Ok(FastPlan {
            app: changed::APP_TESTS.iter().collect(),
            ..FastPlan::default()
        });
        for os in [Os::Macos, Os::Linux] {
            assert!(
                !names(&fast_parts(os, true, &plan))
                    .iter()
                    .any(|n| n == DLL_PART)
            );
        }
    }

    #[test]
    fn a_fast_plan_that_reaches_the_app_runs_its_tests_on_windows_and_skips_elsewhere() {
        let plan = || {
            Ok(FastPlan {
                app: changed::APP_TESTS.iter().collect(),
                ..FastPlan::default()
            })
        };
        let on_windows = fast_parts(Os::Windows, true, &plan());
        assert!(names(&on_windows).iter().any(|n| n == APP_TEST_PART));
        let at = |parts: &[Part]| names(parts).iter().position(|n| n == APP_TEST_PART);
        let on_linux = fast_parts(Os::Linux, true, &plan());
        assert!(matches!(
            on_windows[at(&on_windows).unwrap()],
            Part::Command(_)
        ));
        assert!(matches!(
            on_linux[at(&on_linux).unwrap()],
            Part::Skip { .. }
        ));
    }

    /// Root's milestone run failed with only `[FAIL]` and a name, so nobody could read why.
    #[test]
    fn csharp_tests_print_each_failures_message_and_stack() {
        let projects = changed::CSHARP_TESTS.iter().chain(&changed::APP_TESTS);
        for project in projects {
            let step = csharp_step(project);
            let at = step
                .args
                .iter()
                .position(|arg| *arg == "--logger")
                .unwrap_or_else(|| panic!("{} names no test logger", project.project));
            let logger = step.args[at + 1];
            assert!(
                logger.starts_with("console;verbosity=") && !logger.ends_with("=quiet"),
                "{}: {logger} would hide the failure text",
                project.project
            );
            // The switch that hid it stays, for the build's own noise, and it must not be the logger's word.
            assert!(step.args.contains(&"-v:q"));
        }
    }

    /// The level reads this line, so the logger must keep it and still fail on it.
    #[test]
    fn a_failed_test_run_at_this_logger_still_fails_the_part_by_its_summary() {
        const RUN: &str = "  Failed OpenPainter.Tests.A.B [3 ms]
  Error Message:
   Assert.Equal() Failure
  Stack Trace:
     at A.B() in A.cs:line 1

Failed!  - Failed:     1, Passed:   595, Skipped:     0, Total:   596, Duration: 38 s - A.dll (net10.0)
";
        assert!(dotnet_result(false, RUN).is_err());
        const GOOD: &str = "Passed!  - Failed:     0, Passed:   596, Skipped:     0, Total:   596, Duration: 38 s - A.dll (net10.0)
";
        assert_eq!(dotnet_result(true, GOOD), Ok(()));
    }

    #[test]
    fn csharp_tests_turn_off_usage_reports() {
        for project in &changed::CSHARP_TESTS {
            let step = csharp_step(project);
            assert!(step.env.contains(&("AVALONIA_TELEMETRY_OPTOUT", "1")));
            assert!(step.env.contains(&("DOTNET_CLI_TELEMETRY_OPTOUT", "1")));
        }
    }

    #[test]
    fn ui_spike_tests_use_the_release_build_the_readme_names() {
        let step = &csharp_steps("milestone", true)[1];
        assert!(step.args.ends_with(&["-c", "Release"]));
    }

    #[test]
    fn checkpoint_runs_the_gpu_matrix_and_the_host_on_rtx_and_warp_on_windows() {
        let steps = windows_gpu_steps("checkpoint", true);
        assert_eq!(steps.len(), 10);
        for (range, adapter, label) in [(0..4, "hardware", "RTX"), (4..8, "warp", "WARP")] {
            for step in &steps[range] {
                assert_eq!(step.env, [("OP_GPU_ADAPTER", adapter)]);
                assert!(step.name.starts_with(&format!("GPU matrix on {label}: ")));
                assert!(step.args.ends_with(&["--", "--test-threads=1"]));
                assert!(step.args.contains(&"--no-fail-fast"));
            }
        }
        for suite in [&steps[0], &steps[4]] {
            assert_eq!(&suite.args[..3], ["test", "-p", "op-gpu"]);
        }
        for suite in [&steps[1], &steps[5]] {
            assert_eq!(&suite.args[..3], ["test", "-p", "op-ffi"]);
        }
        for suite in [&steps[2], &steps[6]] {
            for name in ["m54_parity", "engine", "jobs", "gpu_effects"] {
                assert!(suite.args.contains(&name));
            }
        }
        for suite in [&steps[3], &steps[7]] {
            assert!(suite.args.contains(&"--lib"));
        }
        for (step, adapter, label) in [(&steps[8], "hardware", "RTX"), (&steps[9], "warp", "WARP")]
        {
            assert_eq!(step.env, [("OP_GPU_ADAPTER", adapter)]);
            assert!(
                step.name
                    .starts_with(&format!("Win32 test host on {label}: "))
            );
            assert!(step.args.contains(&"tools/win32-host/Cargo.toml"));
            assert!(step.in_repo);
        }
    }

    #[test]
    fn the_gpu_parts_stay_off_mac_linux_and_the_lower_levels() {
        assert!(windows_gpu_steps("checkpoint", false).is_empty());
        assert!(windows_gpu_steps("milestone", true).is_empty());
        assert!(windows_gpu_steps("fast", true).is_empty());
    }

    #[test]
    fn only_the_full_disk_test_is_macos_only() {
        let flags: Vec<bool> = robustness_steps("checkpoint")
            .iter()
            .map(is_macos_only)
            .collect();
        assert_eq!(flags, [false, false, true]);
    }

    #[test]
    fn only_macos_has_the_swift_build_even_with_the_mac_project_present() {
        for os in [Os::Windows, Os::Linux] {
            assert_eq!(
                swift_build_skip(os, true),
                Some("the Mac app builds only on macOS")
            );
            assert!(matches!(swift_part(os, true), Part::Skip { .. }));
        }
        assert_eq!(swift_build_skip(Os::Macos, true), None);
        assert!(matches!(swift_part(Os::Macos, true), Part::Command(_)));
        assert_eq!(
            swift_build_skip(Os::Macos, false),
            Some("mac/project.yml is absent")
        );
    }

    fn names(parts: &[Part]) -> Vec<String> {
        parts.iter().map(Part::name).collect()
    }

    fn dir_for_tests() -> PathBuf {
        std::env::temp_dir()
    }

    fn no_shell_env(_: &str) -> Option<String> {
        None
    }

    fn test_place(dir: &Path) -> RunPlace<'_> {
        RunPlace {
            engine: dir,
            repo: dir,
            shell_env: &no_shell_env,
        }
    }

    /// A command that prints one line and exits 0 on any OS.
    fn echo_step(
        line: &'static str,
        output: OutputRule,
        env: &[(&'static str, &'static str)],
    ) -> CommandStep {
        let mut step = CommandStep::cargo(&[], env);
        step.name = "part".into();
        step.output = output;
        if cfg!(windows) {
            step.program = "cmd";
            // Rust quotes an argument with a space, and `cmd /C` removes those
            // quotes only when they wrap the whole command.
            // `line` may hold a newline; `&` runs a second echo for it.
            let text = format!("echo {}", line.replace('\n', "& echo "));
            step.args = vec!["/C", Box::leak(text.into_boxed_str())];
        } else {
            step.program = "echo";
            step.args = vec![line];
        }
        step
    }

    fn sample_plan() -> FastPlan {
        FastPlan {
            crates: vec!["op-core".into()],
            csharp: vec![&changed::CSHARP_TESTS[2]],
            host: true,
            ..FastPlan::default()
        }
    }

    fn host_names() -> Vec<String> {
        host_static_steps().into_iter().map(|s| s.name).collect()
    }

    const FMT: &str = "cargo fmt --all --check";
    const CLIPPY: &str = "cargo clippy -p op-core --no-deps --all-targets -- -D warnings";
    const TEST: &str = "cargo test -p op-core --no-fail-fast";
    const UI_LOOK: &str = "ui-look tests: dotnet test tools/ui-look/tests/UiLookTests.csproj";

    #[test]
    fn fast_on_windows_runs_each_part_in_order() {
        let parts = level_parts("fast", Os::Windows, true, &Ok(sample_plan()));
        let mut want: Vec<String> = [FMT, CLIPPY, TEST].map(str::to_owned).to_vec();
        want.extend(host_names());
        want.push(UI_LOOK.to_owned());
        want.push("Swift build".to_owned());
        assert_eq!(names(&parts), want);
        assert!(matches!(parts.last(), Some(Part::Skip { .. })));
        assert!(
            parts[..parts.len() - 1]
                .iter()
                .all(|part| !matches!(part, Part::Skip { .. }))
        );
    }

    #[test]
    fn fast_on_mac_and_linux_prints_skip_rows_for_the_windows_parts() {
        for os in [Os::Macos, Os::Linux] {
            let parts = level_parts("fast", os, false, &Ok(sample_plan()));
            assert_eq!(
                names(&parts),
                [
                    FMT,
                    CLIPPY,
                    TEST,
                    "Win32 test host",
                    UI_LOOK,
                    "headless UI for the changed area",
                    "Swift build",
                ]
            );
            for part in &parts[3..6] {
                assert!(matches!(part, Part::Skip { .. }), "{part:?}");
            }
        }
    }

    #[test]
    fn fast_with_no_change_runs_format_only() {
        let parts = level_parts("fast", Os::Windows, true, &Ok(FastPlan::default()));
        assert_eq!(names(&parts), [FMT, "Swift build"]);
    }

    #[test]
    fn changed_code_with_no_test_fails_the_fast_level_by_name() {
        let plan = FastPlan {
            no_test: vec!["tools/catalog/a.py".into(), "perf/b.toml".into()],
            no_test_needed: vec!["docs/windows/PLAN.md".into()],
            ..FastPlan::default()
        };
        let parts = fast_parts(Os::Windows, true, &Ok(plan));
        let row = "changed files with no test: 2, tools/catalog/a.py, perf/b.toml";
        assert_eq!(names(&parts), [FMT, row, "Swift build"]);
        let mut steps = Vec::new();
        run_part(&mut steps, &parts[1], &test_place(&dir_for_tests()));
        assert_eq!(steps[0].status.label(), "NOT RUN");
        assert_eq!(steps[0].name, row);
        assert!(tier_failed(&steps));
    }

    #[test]
    fn a_long_list_of_changed_files_with_no_test_is_cut_short() {
        let paths: Vec<String> = (0..7).map(|n| format!("f{n}")).collect();
        let row = no_test_row(&paths).unwrap().name();
        assert_eq!(
            row,
            "changed files with no test: 7, f0, f1, f2, f3, f4, and more"
        );
        assert_eq!(no_test_row(&[]), None);
    }

    #[test]
    fn nothing_changed_prints_a_skip_row_with_the_branch_and_hash() {
        let plan = FastPlan {
            nothing_changed_since: Some("origin/windows 097af23".into()),
            ..FastPlan::default()
        };
        let parts = fast_parts(Os::Windows, true, &Ok(plan));
        assert_eq!(
            names(&parts),
            [FMT, "changed crates and C# tests", "Swift build"]
        );
        let mut steps = Vec::new();
        run_part(&mut steps, &parts[1], &test_place(&dir_for_tests()));
        assert!(matches!(steps[0].status, Status::Skip));
        assert_eq!(
            steps[0].name,
            "changed crates and C# tests: nothing changed since origin/windows 097af23"
        );
        assert!(!tier_failed(&steps));
    }

    /// A throwaway repository with one commit and an origin/main at that commit.
    /// The caller removes the folder.
    fn clean_repo(tag: &str) -> (PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("op-xtask-{tag}-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("kept.txt"), "line\n").unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}");
            String::from_utf8(output.stdout).unwrap()
        };
        git(&["init", "-q"]);
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-q",
            "-m",
            "first",
        ]);
        git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        let short = git(&["rev-parse", "--short", "HEAD"]).trim().to_owned();
        (dir, short)
    }

    #[test]
    fn the_fast_plan_says_when_git_found_no_changed_file() {
        let (dir, short) = clean_repo("clean-plan");
        let engine = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let clean = fast_plan(engine, &dir).unwrap();
        fs::write(dir.join("new.txt"), "line\n").unwrap();
        let changed = fast_plan(engine, &dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            clean.nothing_changed_since,
            Some(format!("origin/main {short}"))
        );
        assert_eq!(changed.nothing_changed_since, None);
    }

    #[test]
    fn a_changed_script_has_no_test_on_windows_and_reaches_xtask_elsewhere() {
        let (dir, _) = clean_repo("script-plan");
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(dir.join("scripts/a.sh"), "true\n").unwrap();
        let engine = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let plan = fast_plan(engine, &dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        if cfg!(windows) {
            assert_eq!(plan.no_test, ["scripts/a.sh"]);
            assert!(plan.crates.is_empty());
        } else {
            assert_eq!(plan.crates, ["xtask"]);
            assert!(plan.no_test.is_empty());
        }
    }

    #[test]
    fn a_docs_change_alone_passes_the_fast_level() {
        let plan = FastPlan {
            no_test_needed: vec!["docs/windows/PLAN.md".into()],
            ..FastPlan::default()
        };
        let parts = fast_parts(Os::Windows, true, &Ok(plan));
        assert_eq!(names(&parts), [FMT, "Swift build"]);
    }

    #[test]
    fn a_change_only_the_swift_build_reads_needs_a_mac() {
        let plan = || {
            Ok(FastPlan {
                swift_only: vec!["mac/project.yml".into()],
                ..FastPlan::default()
            })
        };
        let row = "changed files with no test: 1, mac/project.yml";
        for os in [Os::Windows, Os::Linux] {
            assert!(names(&fast_parts(os, true, &plan())).contains(&row.to_owned()));
        }
        assert!(!names(&fast_parts(Os::Macos, true, &plan())).contains(&row.to_owned()));
    }

    #[test]
    fn milestone_on_windows_runs_each_part_in_order() {
        let parts = level_parts("milestone", Os::Windows, true, &Ok(FastPlan::default()));
        let mut want: Vec<String> = base_steps().into_iter().map(|s| s.name).collect();
        want.push("Swift build".into());
        want.extend(deep_steps("milestone").into_iter().map(|s| s.name));
        want.push("opcli scenarios".into());
        want.extend(csharp_steps("milestone", true).into_iter().map(|s| s.name));
        want.extend(host_names());
        want.push("headless UI on the AOT build".into());
        want.push("9 to 12 critical flows".into());
        assert_eq!(names(&parts), want);
    }

    #[test]
    fn checkpoint_on_windows_runs_each_part_in_order() {
        let parts = level_parts("checkpoint", Os::Windows, true, &Ok(FastPlan::default()));
        let mut want: Vec<String> = base_steps().into_iter().map(|s| s.name).collect();
        want.push("Swift build".into());
        want.extend(deep_steps("checkpoint").into_iter().map(|s| s.name));
        want.extend(robustness_steps("checkpoint").into_iter().map(|s| s.name));
        want.push("opcli scenarios".into());
        want.extend(csharp_steps("checkpoint", true).into_iter().map(|s| s.name));
        want.extend(host_names());
        want.extend(
            windows_gpu_steps("checkpoint", true)
                .into_iter()
                .map(|s| s.name),
        );
        want.push("scripts/perf-check.sh".into());
        for name in [
            "all real-window flows",
            "paint.net comparison",
            "Mac run",
            "Linux run",
        ] {
            want.push(name.into());
        }
        assert_eq!(names(&parts), want);
    }

    #[test]
    fn milestone_on_mac_and_linux_prints_skip_rows_for_the_windows_parts() {
        for os in [Os::Macos, Os::Linux] {
            let parts = level_parts("milestone", os, true, &Ok(FastPlan::default()));
            let mut want: Vec<String> = base_steps().into_iter().map(|s| s.name).collect();
            want.push("Swift build".into());
            want.extend(deep_steps("milestone").into_iter().map(|s| s.name));
            want.push("opcli scenarios".into());
            for name in [
                "C# tests",
                "Win32 test host",
                "headless UI on the AOT build",
                "9 to 12 critical flows",
            ] {
                want.push(name.into());
            }
            assert_eq!(names(&parts), want);
            let skips = parts.iter().filter(|p| matches!(p, Part::Skip { .. }));
            assert_eq!(skips.count(), if os == Os::Linux { 5 } else { 4 });
        }
    }

    #[test]
    fn checkpoint_on_mac_and_linux_prints_skip_rows_for_the_windows_parts() {
        for os in [Os::Macos, Os::Linux] {
            let parts = level_parts("checkpoint", os, true, &Ok(FastPlan::default()));
            let mut want: Vec<String> = base_steps().into_iter().map(|s| s.name).collect();
            want.push("Swift build".into());
            want.extend(deep_steps("checkpoint").into_iter().map(|s| s.name));
            want.extend(robustness_steps("checkpoint").into_iter().map(|s| s.name));
            want.push("opcli scenarios".into());
            want.push("scripts/perf-check.sh".into());
            for name in [
                "C# tests",
                "Win32 test host",
                "GPU matrix on RTX and WARP",
                "all real-window flows",
                "paint.net comparison",
            ] {
                want.push(name.into());
            }
            assert_eq!(names(&parts), want);
            let last = &parts[parts.len() - 2];
            assert!(matches!(last, Part::Skip { .. }), "{last:?}");
        }
    }

    #[test]
    fn the_low_disk_test_runs_only_on_macos_and_prints_skip_elsewhere() {
        let low_disk = |os| {
            level_parts("checkpoint", os, true, &Ok(FastPlan::default()))
                .into_iter()
                .find(|part| part.name().contains("low_disk"))
                .unwrap()
        };
        assert!(matches!(low_disk(Os::Macos), Part::Command(_)));
        for os in [Os::Windows, Os::Linux] {
            let Part::Skip { reason, .. } = low_disk(os) else {
                panic!("low_disk must print SKIP on {os:?}");
            };
            assert_eq!(reason, "this test compiles to zero tests off macOS");
        }
    }

    #[test]
    fn the_host_is_format_lint_and_build_with_warnings_as_errors() {
        let steps = host_static_steps();
        assert_eq!(steps.len(), 3);
        for step in &steps {
            assert!(step.in_repo, "{}", step.name);
            assert!(step.args.contains(&HOST_MANIFEST), "{}", step.name);
        }
        assert_eq!(steps[0].args[0], "fmt");
        assert!(steps[0].args.contains(&"--check"));
        assert_eq!(steps[1].args[0], "clippy");
        assert!(steps[1].args.ends_with(&["--", "-D", "warnings"]));
        assert_eq!(steps[2].args[0], "build");
    }

    #[test]
    fn only_the_gpu_parts_read_the_gpu_settings_and_only_the_host_reads_its_scenarios() {
        let gpu = windows_gpu_steps("checkpoint", true);
        assert!(gpu.iter().all(|step| step.needs_gpu));
        assert!(gpu[..8].iter().all(|s| s.output == OutputRule::ExitCode));
        assert!(
            gpu[8..]
                .iter()
                .all(|s| s.output == OutputRule::HostScenarios)
        );
        assert!(base_steps().iter().all(|step| !step.needs_gpu));
        assert!(
            csharp_steps("milestone", true)
                .iter()
                .all(|step| step.output == OutputRule::DotnetTotal)
        );
    }

    #[test]
    fn a_setting_that_turns_the_gpu_off_fails_a_gpu_part_by_name() {
        let dir = dir_for_tests();
        let mut step = echo_step("hello", OutputRule::ExitCode, &[]);
        step.name = "GPU matrix on RTX: suite".into();
        step.needs_gpu = true;

        let mut steps = Vec::new();
        run_step(&mut steps, &step, &test_place(&dir));
        assert!(matches!(steps[0].status, Status::Pass));

        for (setting, value) in [("OP_GPU_TESTS", "0"), ("OP_FORCE_CPU", "1")] {
            let shell_env = move |name: &str| (name == setting).then(|| value.to_owned());
            let place = RunPlace {
                engine: &dir,
                repo: &dir,
                shell_env: &shell_env,
            };
            let mut steps = Vec::new();
            run_step(&mut steps, &step, &place);
            assert!(steps[0].name.starts_with("GPU matrix on RTX: suite: "));
            assert!(steps[0].name.contains(setting), "{}", steps[0].name);
            assert!(matches!(steps[0].status, Status::Fail), "{setting}");
        }
    }

    #[test]
    fn a_shell_setting_problem_names_each_variable_set() {
        assert_eq!(shell_setting_problem(true, true, &no_shell_env), None);
        let all = |name: &str| Some(format!("{name}-value"));
        let text = shell_setting_problem(true, false, &all).unwrap();
        assert!(text.contains("OP_FORCE_CPU=OP_FORCE_CPU-value"), "{text}");
        assert!(text.contains("OP_GPU_TESTS=OP_GPU_TESTS-value"), "{text}");
        assert!(text.contains("OP_BLESS=OP_BLESS-value"), "{text}");
        // The host runs no golden test, so only the GPU settings count for it.
        let text = shell_setting_problem(false, true, &all).unwrap();
        assert!(text.contains("OP_FORCE_CPU"), "{text}");
        assert!(!text.contains("OP_BLESS"), "{text}");
        // A part with neither is left alone.
        assert_eq!(shell_setting_problem(false, false, &all), None);
    }

    /// Runs a part with one variable set in the shell.
    fn row_with_setting(part: &Part, setting: &'static str) -> Step {
        let dir = dir_for_tests();
        let shell_env = move |name: &str| (name == setting).then(|| "1".to_owned());
        let place = RunPlace {
            engine: &dir,
            repo: &dir,
            shell_env: &shell_env,
        };
        let mut steps = Vec::new();
        run_part(&mut steps, part, &place);
        steps.remove(0)
    }

    #[test]
    fn every_cargo_test_part_of_every_level_fails_by_name_on_a_spoiling_setting() {
        let mut parts = level_parts("fast", Os::Windows, true, &Ok(sample_plan()));
        parts.extend(level_parts(
            "milestone",
            Os::Linux,
            true,
            &Ok(FastPlan::default()),
        ));
        parts.extend(level_parts(
            "checkpoint",
            Os::Macos,
            true,
            &Ok(FastPlan::default()),
        ));
        parts.extend(level_parts(
            "checkpoint",
            Os::Windows,
            true,
            &Ok(FastPlan::default()),
        ));
        let tests: Vec<&Part> = parts
            .iter()
            .filter(|part| !matches!(part, Part::Skip { .. }))
            .filter(|part| part.name().contains("cargo test"))
            .collect();
        assert!(tests.len() >= 14, "{}", tests.len());
        for part in tests {
            for setting in ["OP_BLESS", "OP_GPU_TESTS", "OP_FORCE_CPU"] {
                let step = row_with_setting(part, setting);
                assert!(matches!(step.status, Status::Fail), "{}", step.name);
                assert!(step.name.starts_with(&part.name()), "{}", step.name);
                assert!(step.name.contains(setting), "{}", step.name);
            }
        }
    }

    #[test]
    fn a_setting_does_not_stop_a_part_that_runs_no_cargo_test() {
        let echo = Part::Command(echo_step("hello", OutputRule::ExitCode, &[]));
        assert!(matches!(
            row_with_setting(&echo, "OP_BLESS").status,
            Status::Pass
        ));
        let clippy = Part::CrateCommand(vec!["clippy".to_owned(), "--version".to_owned()]);
        assert!(matches!(
            row_with_setting(&clippy, "OP_BLESS").status,
            Status::Pass
        ));
    }

    const PASSED: &str = "Passed!  - Failed:     0, Passed:    74, Skipped:     0, Total:    74, Duration: 933 ms - PdnCaptureTests.dll (net10.0)";

    #[test]
    fn a_dotnet_run_counts_only_with_tests_in_its_total() {
        assert_eq!(dotnet_result(true, PASSED), Ok(()));
        let none = "Test run for a.dll\nA total of 1 test files matched the specified pattern.\n";
        assert!(
            dotnet_result(true, none)
                .unwrap_err()
                .contains("no total line")
        );
        let zero =
            "Passed!  - Failed:     0, Passed:     0, Skipped:     0, Total:     0, Duration: 1 ms";
        assert!(dotnet_result(true, zero).unwrap_err().contains("Total: 0"));
        let all_skipped =
            "Passed!  - Failed:     0, Passed:     0, Skipped:     5, Total:     5, Duration: 1 ms";
        assert!(
            dotnet_result(true, all_skipped)
                .unwrap_err()
                .contains("Passed: 0")
        );
        let no_passed = "Passed!  - Failed:     0, Skipped:     5, Total:     5, Duration: 1 ms";
        assert!(
            dotnet_result(true, no_passed)
                .unwrap_err()
                .contains("no Passed count")
        );
        assert!(dotnet_result(false, PASSED).is_err());
    }

    #[test]
    fn a_dotnet_step_with_no_total_line_fails_and_one_with_a_total_passes() {
        let dir = dir_for_tests();
        let mut steps = Vec::new();
        let none = echo_step(
            "A total of 1 test files matched",
            OutputRule::DotnetTotal,
            &[],
        );
        run_step(&mut steps, &none, &test_place(&dir));
        assert!(matches!(steps[0].status, Status::Fail));
        assert!(steps[0].name.contains("no total line"), "{}", steps[0].name);
        let some = echo_step(PASSED, OutputRule::DotnetTotal, &[]);
        run_step(&mut steps, &some, &test_place(&dir));
        assert!(matches!(steps[1].status, Status::Pass));
        assert_eq!(steps[1].name, "part");
    }

    const HOST_LINE: &str = "RemoveDevice-idle: NOT RUN (OP_GPU_ADAPTER is not warp)";

    /// The end of a real host run on the RTX, 2026-11-02.
    const REAL_RTX_END: &str = "destroy-before-frame: PASS
RemoveDevice-idle: NOT RUN (OP_GPU_ADAPTER is not warp)
RemoveDevice-retry-three-errors-second-view: NOT RUN (OP_GPU_ADAPTER is not warp)
RemoveDevice-surface-retry: NOT RUN (OP_GPU_ADAPTER is not warp)
frame-panic-keeps-device: NOT RUN (OP_GPU_ADAPTER is not warp)
validation-errors: PASS (0)
desktop-close: PASS
";

    #[test]
    fn a_real_rtx_run_passes_and_names_all_4_scenarios() {
        let (result, skipped) = host_result(true, REAL_RTX_END, false);
        assert_eq!(result, Ok(()));
        assert_eq!(
            skipped.as_deref(),
            Some(
                "RemoveDevice-idle, RemoveDevice-retry-three-errors-second-view, \
                 RemoveDevice-surface-retry, frame-panic-keeps-device"
            )
        );
    }

    #[test]
    fn a_silent_host_fails_by_name_on_either_adapter() {
        for warp in [true, false] {
            for text in ["", "attach: PASS\n"] {
                let (result, skipped) = host_result(true, text, warp);
                assert!(result.unwrap_err().contains(HOST_END_LINE), "{text}");
                assert_eq!(skipped, None);
            }
        }
    }

    #[test]
    fn a_good_run_may_end_without_the_desktop_close_line() {
        let text = "attach: PASS\nvalidation-errors: PASS (0)\n";
        assert_eq!(host_result(true, text, true), (Ok(()), None));
    }

    #[test]
    fn the_skip_reason_is_true_for_the_panic_scenario_as_well() {
        assert!(HOST_RTX_SKIP.contains("only on WARP"));
        assert!(HOST_RTX_SKIP.contains("3 remove the device"));
        assert!(HOST_RTX_SKIP.contains("1 forces a frame panic"));
        assert!(!HOST_RTX_SKIP.starts_with("device removal"));
    }

    #[test]
    fn the_host_on_the_rtx_passes_with_the_scenarios_named_for_a_skip_row() {
        let text = format!(
            "attach: PASS\n{HOST_LINE}\nframe-panic: NOT RUN (OP_GPU_ADAPTER is not warp)\nvalidation-errors: PASS (0)\n"
        );
        let (result, skipped) = host_result(true, &text, false);
        assert_eq!(result, Ok(()));
        assert_eq!(skipped.as_deref(), Some("RemoveDevice-idle, frame-panic"));
    }

    #[test]
    fn the_host_on_warp_fails_on_any_not_run_line() {
        let (result, skipped) = host_result(true, HOST_LINE, true);
        assert!(result.unwrap_err().contains("RemoveDevice-idle"));
        assert_eq!(skipped, None);
    }

    #[test]
    fn the_host_with_no_not_run_lines_passes_and_a_bad_exit_fails() {
        let text = "attach: PASS\nvalidation-errors: PASS (0)\n";
        assert_eq!(host_result(true, text, true), (Ok(()), None));
        assert_eq!(host_result(true, text, false), (Ok(()), None));
        assert!(host_result(false, text, true).0.is_err());
    }

    #[test]
    fn the_host_on_the_rtx_fails_on_a_not_run_line_with_another_reason() {
        let (result, _) = host_result(
            true,
            "other: NOT RUN (no desktop)\nvalidation-errors: PASS (0)\n",
            false,
        );
        assert!(result.unwrap_err().contains("other"));
    }

    #[test]
    fn the_host_step_prints_a_skip_row_on_the_rtx_and_fails_on_warp() {
        let dir = dir_for_tests();
        let rtx = echo_step(
            "RemoveDevice-idle: NOT RUN (OP_GPU_ADAPTER is not warp)\nvalidation-errors: PASS (0)",
            OutputRule::HostScenarios,
            &[("OP_GPU_ADAPTER", "hardware")],
        );
        let mut steps = Vec::new();
        run_step(&mut steps, &rtx, &test_place(&dir));
        assert_eq!(steps.len(), 2);
        assert!(matches!(steps[0].status, Status::Pass));
        assert!(matches!(steps[1].status, Status::Skip));
        assert!(
            steps[1].name.contains("RemoveDevice-idle"),
            "{}",
            steps[1].name
        );
        assert!(!tier_failed(&steps));

        let warp = echo_step(
            HOST_LINE,
            OutputRule::HostScenarios,
            &[("OP_GPU_ADAPTER", "warp")],
        );
        let mut steps = Vec::new();
        run_step(&mut steps, &warp, &test_place(&dir));
        assert_eq!(steps.len(), 1);
        assert!(matches!(steps[0].status, Status::Fail));
        assert!(tier_failed(&steps));
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|text| (*text).to_owned()).collect()
    }

    #[test]
    fn the_check_arguments_take_all_changed_with_fast_only() {
        assert_eq!(
            parse_check_args(&strings(&["check", "--tier", "fast"])),
            Ok(("fast", false))
        );
        assert_eq!(
            parse_check_args(&strings(&["check", "--tier", "fast", "--all-changed"])),
            Ok(("fast", true))
        );
        assert_eq!(
            parse_check_args(&strings(&["check", "--tier", "milestone"])),
            Ok(("milestone", false))
        );
        let args = strings(&["check", "--tier", "milestone", "--all-changed"]);
        let error = parse_check_args(&args);
        assert!(error.unwrap_err().contains("only with --tier fast"));
        assert!(parse_check_args(&strings(&["check", "--tier", "nope"])).is_err());
        assert!(parse_check_args(&strings(&["check", "--tier", "fast", "--other"])).is_err());
        assert!(parse_check_args(&strings(&["check"])).is_err());
    }

    #[test]
    fn the_linux_check_passes_all_changed_only_for_the_fast_level() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/linux-check.sh");
        let text = fs::read_to_string(script).unwrap();
        assert!(text.contains("all_changed=\"--all-changed\""), "{text}");
        assert!(text.contains("if [ \"$tier\" = fast ]"), "{text}");
        assert!(
            text.contains("cargo xtask check --tier $tier $all_changed"),
            "{text}"
        );
    }

    #[test]
    fn the_whole_tree_plan_needs_no_git_and_reaches_the_host() {
        // This plan never asks Git, the same as in the Linux check's copy.
        let engine = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let plan = whole_tree_plan(engine).unwrap();
        assert!(plan.crates.contains(&"xtask".to_owned()));
        assert_eq!(plan.csharp.len(), 3);
        assert!(plan.host);
    }

    #[test]
    fn a_folder_with_no_git_fails_by_name_without_the_flag() {
        let dir = std::env::temp_dir().join(format!("op-xtask-no-git-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let engine = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let reason = fast_plan(engine, &dir).unwrap_err();
        fs::remove_dir_all(&dir).unwrap();
        assert!(
            reason.contains("git merge-base HEAD origin/main"),
            "{reason}"
        );
    }

    #[test]
    fn the_fast_time_line_names_the_limit_and_says_when_it_is_over() {
        assert_eq!(
            time_line("fast", 12.5),
            "fast level took 12.50 s; the limit is 120 s"
        );
        assert!(time_line("fast", 120.0).ends_with("OVER THE LIMIT"));
        assert_eq!(
            time_line("milestone", 900.0),
            "milestone level took 900.00 s"
        );
    }
}
