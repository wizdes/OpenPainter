//! Run the workspace check tiers.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

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

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 || args[0] != "check" || args[1] != "--tier" {
        eprintln!("Usage: cargo xtask check --tier fast|milestone|checkpoint");
        return ExitCode::from(2);
    }
    let tier = args[2].as_str();
    if !matches!(tier, "fast" | "milestone" | "checkpoint") {
        eprintln!("Unknown tier: {tier}");
        return ExitCode::from(2);
    }
    let engine = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace parent")
        .to_path_buf();
    let repo = engine.parent().expect("repository parent");
    let mut steps = Vec::new();
    run(
        &mut steps,
        "cargo fmt --all --check",
        &engine,
        "cargo",
        &["fmt", "--all", "--check"],
    );
    run(
        &mut steps,
        "cargo clippy --workspace --all-targets -- -D warnings",
        &engine,
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    );
    run(
        &mut steps,
        "cargo test --workspace",
        &engine,
        "cargo",
        &["test", "--workspace"],
    );
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
    if matches!(tier, "milestone" | "checkpoint") {
        run(
            &mut steps,
            "cargo test --workspace --release",
            &engine,
            "cargo",
            &["test", "--workspace", "--release"],
        );
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
    println!("Running {name}");
    let start = Instant::now();
    let status = match Command::new(program).args(args).current_dir(dir).status() {
        Ok(exit) if exit.success() => Status::Pass,
        Ok(exit) => {
            eprintln!("{name} exited with {exit}");
            Status::Fail
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
