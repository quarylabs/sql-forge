use clap::Parser as _;
use commands::{FixArgs, Format, LintArgs};
use sqruff_lib::cli::formatters::OutputStreamFormatter;
use sqruff_lib::core::config::FluffConfig;
use sqruff_lib::core::linter::linter::Linter;

use crate::commands::{Cli, Commands};

mod commands;

#[cfg(all(feature = "jemalloc", not(target_env = "msvc")))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

fn main() {
    let config = FluffConfig::from_root(None, false, None).unwrap();

    let cli = Cli::parse();

    match cli.command {
        Commands::Lint(LintArgs { paths, format }) => {
            let mut linter = linter(config, format);
            let result = linter.lint_paths(paths, false);

            if let Format::GithubAnnotationNative = format {
                for path in result.paths {
                    for file in path.files {
                        for violation in file.violations {
                            let mut line = "::error ".to_string();
                            line.push_str("title=SQLFluff,");
                            line.push_str(&format!("file={},", file.path));
                            line.push_str(&format!("line={},", violation.line_no));
                            line.push_str(&format!("col={}", violation.line_pos));
                            line.push_str("::");
                            line.push_str(&format!(
                                "{}: {}",
                                violation.rule.as_ref().unwrap().code(),
                                violation.description
                            ));
                            eprintln!("{line}");
                        }
                    }
                }
            }

            std::process::exit(if linter.formatter.unwrap().has_fail.get() { 1 } else { 0 })
        }
        Commands::Fix(FixArgs { paths, force, format }) => {
            let mut linter = linter(config, format);
            let result = linter.lint_paths(paths, true);

            if !force {
                match check_user_input() {
                    Some(true) => {
                        println!("Attempting fixes...");
                    }
                    Some(false) => return,
                    None => {
                        println!("Invalid input, please enter 'Y' or 'N'");
                        println!("Aborting...");
                    }
                }
            }

            for linted_dir in result.paths {
                for file in linted_dir.files {
                    let write_buff = file.fix_string();
                    std::fs::write(file.path, write_buff).unwrap();
                }
            }

            linter.formatter.as_mut().unwrap().completion_message();
        }
    }
}

fn linter(config: FluffConfig, format: Format) -> Linter {
    let output_stream: Box<dyn std::io::Write> = match format {
        Format::Human => Box::new(std::io::stderr()),
        Format::GithubAnnotationNative => Box::new(std::io::sink()),
    };

    let formatter = OutputStreamFormatter::new(
        output_stream,
        config.get("nocolor", "core").as_bool().unwrap_or_default(),
    );

    Linter::new(config, formatter.into(), None)
}

fn check_user_input() -> Option<bool> {
    use std::io::Write;

    let mut term = console::Term::stdout();
    term.write(b"Are you sure you wish to attempt to fix these? [Y/n] ").unwrap();
    term.flush().unwrap();

    let ret = match term.read_char().unwrap().to_ascii_lowercase() {
        'y' | '\r' | '\n' => Some(true),
        'n' => Some(false),
        _ => None,
    };
    term.write(b" ...\n").unwrap();
    ret
}
