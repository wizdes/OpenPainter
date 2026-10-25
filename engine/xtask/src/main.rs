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
    for step in deep_steps(tier) {
        run_step(&mut steps, &step, &engine, repo);
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
        let app = find_app(repo);
        run_script(&mut steps, "scripts/size-report.sh", repo, app.as_deref());
        run_script(&mut steps, "scripts/launch-time.sh", repo, app.as_deref());
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

fn find_app(repo: &Path) -> Option<PathBuf> {
    [
        "mac/build/OpenPainter.app",
        "mac/build/Build/Products/Release/OpenPainter.app",
        "mac/build/Release/OpenPainter.app",
    ]
    .iter()
    .map(|relative| repo.join(relative))
    .find(|path| path.is_dir())
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

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_script_reads_five_ready_lines_without_a_temp_file() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let fixture = std::env::temp_dir().join(format!("op-launch-test-{}", std::process::id()));
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
        fs::write(&binary, "#!/bin/sh\necho 'OP_READY 123'\nexec sleep 5\n").unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let output = Command::new("bash")
            .arg("scripts/launch-time.sh")
            .arg(&app)
            .current_dir(repo)
            .output()
            .unwrap();
        fs::remove_dir_all(&fixture).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("Cold launch median: 123 ms"));
    }
}
