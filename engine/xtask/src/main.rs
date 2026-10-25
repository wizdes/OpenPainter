//! Run the workspace check tiers.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

mod fuzz;

#[derive(Clone, Copy)]
enum Status {
    Pass,
    Fail,
    Skip,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Skip => "SKIP",
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
}

impl CommandStep {
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

/// The engine checks every tier runs first.
fn base_steps() -> Vec<CommandStep> {
    vec![
        CommandStep::cargo(&["fmt", "--all", "--check"], &[]),
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
        CommandStep::cargo(&["test", "--workspace"], &[]),
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
            &["test", "--workspace", "--release"],
            &[("OP_PROPTEST_CASES", cases)],
        ),
        CommandStep {
            name: "python3 -B tests/oracle/legacy336/test_oracle.py".into(),
            program: "python3",
            args: vec!["-B", "tests/oracle/legacy336/test_oracle.py"],
            env: Vec::new(),
            in_repo: true,
            skip_if_missing: true,
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

fn run_step(steps: &mut Vec<Step>, step: &CommandStep, engine: &Path, repo: &Path) {
    let dir = if step.in_repo { repo } else { engine };
    run_with(
        steps,
        &step.name,
        dir,
        step.program,
        &step.args,
        &step.env,
        step.skip_if_missing,
    );
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
    if args.len() != 3 || args[0] != "check" || args[1] != "--tier" {
        eprintln!("Usage: cargo xtask check --tier fast|milestone|checkpoint");
        return ExitCode::from(2);
    }
    let tier = args[2].as_str();
    if !matches!(tier, "fast" | "milestone" | "checkpoint") {
        eprintln!("Unknown tier: {tier}");
        return ExitCode::from(2);
    }
    let engine = engine_dir(
        std::env::var_os("CARGO_MANIFEST_DIR"),
        env!("CARGO_MANIFEST_DIR"),
    );
    let repo = engine.parent().expect("repository parent");
    let mut steps = Vec::new();
    for step in base_steps() {
        run_step(&mut steps, &step, &engine, repo);
    }
    if repo.join("mac/project.yml").is_file() {
        // TODO: Generate the Xcode project from project.yml before this build when the Mac app exists.
        run(
            &mut steps,
            "Swift build",
            repo,
            "xcodebuild",
            &[
                "-project",
                "mac/OpenPainter.xcodeproj",
                "-scheme",
                "OpenPainter",
                "build",
            ],
        );
    } else {
        skip(&mut steps, "Swift build", "mac/project.yml is absent");
    }
    for step in deep_steps(tier).iter().chain(&robustness_steps(tier)) {
        run_step(&mut steps, step, &engine, repo);
    }
    if matches!(tier, "milestone" | "checkpoint") {
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
                        Err(error) => fail(&mut steps, "opcli scenarios", &error.to_string()),
                    }
                }
                paths
            }
            Err(error) => {
                fail(
                    &mut steps,
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
            skip(
                &mut steps,
                "opcli scenarios",
                "no tests/scenarios/*.json files",
            );
        }
        for scenario in scenarios {
            let name = format!("opcli scenario {}", scenario.display());
            let argument = scenario.to_string_lossy();
            run(
                &mut steps,
                &name,
                &engine,
                "cargo",
                &[
                    "run", "--quiet", "-p", "op-cli", "--", "scenario", &argument,
                ],
            );
        }
    }
    if tier == "checkpoint" {
        for script in checkpoint_scripts() {
            if cfg!(target_os = "macos") {
                run_script(&mut steps, script, repo, None);
            } else {
                skip(&mut steps, script, "the Mac app builds only on macOS");
            }
        }
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
    if steps.iter().any(|step| matches!(step.status, Status::Fail)) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
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
        name: name.into(),
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
    use std::os::unix::fs::PermissionsExt;

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
                "cargo test --workspace",
            ]
        );
    }

    #[test]
    fn milestone_tier_runs_1024_cases_and_the_oracle_tests() {
        let steps = deep_steps("milestone");
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].program, "cargo");
        assert_eq!(steps[0].args, ["test", "--workspace", "--release"]);
        assert_eq!(steps[0].env, [("OP_PROPTEST_CASES", "1024")]);
        assert_eq!(
            steps[0].name,
            "OP_PROPTEST_CASES=1024 cargo test --workspace --release"
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
        assert_eq!(steps[0].args, ["test", "--workspace", "--release"]);
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
            "sh",
            &["-c", "test \"$OP_PROPTEST_CASES\" = 1024"],
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
    fn perf_compare_fails_on_a_launch_p95_regression() {
        let (ok, text) = perf_compare("p95", &[("launch_p95_ms", "2201")], &[]);
        assert!(
            !ok && text.contains("FAIL regression: launch p95"),
            "{text}"
        );
    }

    #[test]
    fn perf_compare_fails_on_a_first_effect_regression() {
        let (ok, text) = perf_compare("effect", &[("first_effect_median_ms", "1300")], &[]);
        assert!(
            !ok && text.contains("FAIL regression: first GPU effect median"),
            "{text}"
        );
    }

    #[test]
    fn perf_compare_refuses_a_purged_run_against_an_unpurged_baseline() {
        let (ok, text) = perf_compare("purge", &[("cache_purged", "1")], &[]);
        assert!(!ok && text.contains("do not compare"), "{text}");
        let (ok, text) = perf_compare("purge2", &[], &[("cache_purged", "1")]);
        assert!(!ok && text.contains("do not compare"), "{text}");
    }

    #[test]
    fn perf_compare_refuses_another_machine() {
        let (ok, text) = perf_compare("machine", &[], &[("machine", "\"Other Mac\"")]);
        assert!(!ok && text.contains("the baseline is from"), "{text}");
    }
}
