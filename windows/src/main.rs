// No console window when opened from Explorer; the CLI reattaches to the parent's.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod gui;
mod merge;
mod office;
mod package;
mod pdf;
mod powerpoint;
mod scaler;
mod textfit;
mod tools;
mod verifier;
mod xml;

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Error {
    Fail(String),
    /// Some inputs are not 16:9: the user must choose to fit them or stop.
    FormatChoice(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Fail(s) | Error::FormatChoice(s) => f.write_str(s),
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(Error::Fail(message.into()))
}

/// Pretty JSON with sorted keys, like the macOS reports.
pub fn to_json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    let v = serde_json::to_value(value).or_else(|e| fail(e.to_string()))?;
    serde_json::to_vec_pretty(&v).or_else(|e| fail(e.to_string()))
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", String::from_utf8_lossy(&to_json(value)?));
    Ok(())
}

#[cfg(windows)]
fn attach_console() {
    extern "system" {
        fn AttachConsole(process: u32) -> i32;
    }
    unsafe {
        AttachConsole(u32::MAX);
    }
}

fn run_cli(all: &[String]) -> i32 {
    let fit = all.iter().any(|a| a == "--fit-non169");
    let args: Vec<&String> = all.iter().filter(|a| *a != "--fit-non169").collect();
    let paths = |from: usize| -> Vec<PathBuf> { args[from..].iter().map(PathBuf::from).collect() };
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("");
    let result: Result<i32> = (|| {
        match command {
            "--office-prepare" | "--office-verify" if args.len() >= 4 => {
                if command == "--office-prepare" {
                    print_json(&office::prepare(&paths(3), &PathBuf::from(args[2]), fit)?)?;
                    Ok(0)
                } else {
                    let checks = office::verify(&paths(3), &PathBuf::from(args[2]))?;
                    print_json(&checks)?;
                    Ok(if checks.iter().all(|c| c.issues.is_empty()) { 0 } else { 2 })
                }
            }
            "--verify" if args.len() >= 5 => {
                let checks = verifier::inspect(&paths(3), &PathBuf::from(args[2]))?;
                print_json(&checks)?;
                Ok(if checks.iter().all(|c| c.issues.is_empty()) { 0 } else { 2 })
            }
            "--pdf-to-ppt" | "--to-169" | "--extract-notes" if args.len() == 4 => {
                let (input, output) = (PathBuf::from(args[3]), PathBuf::from(args[2]));
                let report = match command {
                    "--pdf-to-ppt" => tools::pdf_to_ppt(&input, &output),
                    "--to-169" => tools::widescreen(&input, &output),
                    _ => tools::extract_notes(&input, &output),
                };
                match report {
                    Ok(r) => {
                        print_json(&r)?;
                        Ok(0)
                    }
                    Err(e) => {
                        let log = tools::log_failure(&input, &e.to_string());
                        eprintln!("{e}\nRegistre: {log}");
                        Ok(1)
                    }
                }
            }
            "--merge-powerpoint" if args.len() >= 5 => {
                let report = powerpoint::merge(&paths(3), &PathBuf::from(args[2]), fit, &|m| eprintln!("{m}"))?;
                println!("{} diapositives desades per PowerPoint. Informe: {}.report.json", report.power_point_slides, args[2]);
                Ok(if report.passed() { 0 } else { 2 })
            }
            "--merge" if args.len() >= 5 => {
                let report = merge::merge(&paths(3), &PathBuf::from(args[2]), fit)?;
                println!("{} diapositives. Informe: {}.report.json", report.total_slides, args[2]);
                Ok(if report.structural_validation_passed() { 0 } else { 2 })
            }
            _ => fail("Ús: PPTMerger --merge sortida.pptm entrada1.pptm entrada2.pptm […]\n     PPTMerger --merge-powerpoint sortida.pptx entrada1.pptx entrada2.pptx […]"),
        }
    })();
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        #[cfg(windows)]
        attach_console();
        std::process::exit(run_cli(&args));
    }
    if let Err(e) = gui::run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
