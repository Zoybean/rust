// Copyright 2015 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

#![feature(rustc_private)]

#[macro_use]
extern crate log;
extern crate regex;
extern crate rustfmt_nightly as rustfmt;
extern crate term;

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::iter::Peekable;
use std::path::{Path, PathBuf};
use std::str::Chars;

use rustfmt::*;
use rustfmt::filemap::{write_system_newlines, FileMap};
use rustfmt::config::{Color, Config, ReportTactic};
use rustfmt::rustfmt_diff::*;

const DIFF_CONTEXT_SIZE: usize = 3;

fn get_path_string(dir_entry: io::Result<fs::DirEntry>) -> PathBuf {
    dir_entry.expect("Couldn't get DirEntry").path().to_owned()
}

// Integration tests. The files in the tests/source are formatted and compared
// to their equivalent in tests/target. The target file and config can be
// overridden by annotations in the source file. The input and output must match
// exactly.
#[test]
fn system_tests() {
    // Get all files in the tests/source directory.
    let files = fs::read_dir("tests/source").expect("Couldn't read source dir");
    // Turn a DirEntry into a String that represents the relative path to the
    // file.
    let files = files.map(get_path_string);
    let (_reports, count, fails) = check_files(files);

    // Display results.
    println!("Ran {} system tests.", count);
    assert_eq!(fails, 0, "{} system tests failed", fails);
}

// Do the same for tests/coverage-source directory
// the only difference is the coverage mode
#[test]
fn coverage_tests() {
    let files = fs::read_dir("tests/coverage/source").expect("Couldn't read source dir");
    let files = files.map(get_path_string);
    let (_reports, count, fails) = check_files(files);

    println!("Ran {} tests in coverage mode.", count);
    assert_eq!(fails, 0, "{} tests failed", fails);
}

#[test]
fn checkstyle_test() {
    let filename = "tests/writemode/source/fn-single-line.rs";
    let expected_filename = "tests/writemode/target/checkstyle.xml";
    assert_output(Path::new(filename), Path::new(expected_filename));
}

// Helper function for comparing the results of rustfmt
// to a known output file generated by one of the write modes.
fn assert_output(source: &Path, expected_filename: &Path) {
    let config = read_config(source);
    let (_error_summary, file_map, _report) = format_file(source, &config);

    // Populate output by writing to a vec.
    let mut out = vec![];
    let _ = filemap::write_all_files(&file_map, &mut out, &config);
    let output = String::from_utf8(out).unwrap();

    let mut expected_file = fs::File::open(&expected_filename).expect("Couldn't open target");
    let mut expected_text = String::new();
    expected_file
        .read_to_string(&mut expected_text)
        .expect("Failed reading target");

    let compare = make_diff(&expected_text, &output, DIFF_CONTEXT_SIZE);
    if !compare.is_empty() {
        let mut failures = HashMap::new();
        failures.insert(source.to_owned(), compare);
        print_mismatches(failures);
        assert!(false, "Text does not match expected output");
    }
}

// Idempotence tests. Files in tests/target are checked to be unaltered by
// rustfmt.
#[test]
fn idempotence_tests() {
    // Get all files in the tests/target directory.
    let files = fs::read_dir("tests/target")
        .expect("Couldn't read target dir")
        .map(get_path_string);
    let (_reports, count, fails) = check_files(files);

    // Display results.
    println!("Ran {} idempotent tests.", count);
    assert_eq!(fails, 0, "{} idempotent tests failed", fails);
}

// Run rustfmt on itself. This operation must be idempotent. We also check that
// no warnings are emitted.
#[test]
fn self_tests() {
    let files = fs::read_dir("src/bin")
        .expect("Couldn't read src dir")
        .chain(fs::read_dir("tests").expect("Couldn't read tests dir"))
        .map(get_path_string);
    // Hack because there's no `IntoIterator` impl for `[T; N]`.
    let files = files.chain(Some(PathBuf::from("src/lib.rs")).into_iter());
    let files = files.chain(Some(PathBuf::from("build.rs")).into_iter());

    let (reports, count, fails) = check_files(files);
    let mut warnings = 0;

    // Display results.
    println!("Ran {} self tests.", count);
    assert_eq!(fails, 0, "{} self tests failed", fails);

    for format_report in reports {
        println!("{}", format_report);
        warnings += format_report.warning_count();
    }

    assert_eq!(
        warnings, 0,
        "Rustfmt's code generated {} warnings",
        warnings
    );
}

#[test]
fn stdin_formatting_smoke_test() {
    let input = Input::Text("fn main () {}".to_owned());
    let config = Config::default();
    let (error_summary, file_map, _report) =
        format_input::<io::Stdout>(input, &config, None).unwrap();
    assert!(error_summary.has_no_errors());
    for &(ref file_name, ref text) in &file_map {
        if let FileName::Custom(ref file_name) = *file_name {
            if file_name == "stdin" {
                assert_eq!(text.to_string(), "fn main() {}\n");
                return;
            }
        }
    }
    panic!("no stdin");
}

// FIXME(#1990) restore this test
// #[test]
// fn stdin_disable_all_formatting_test() {
//     let input = String::from("fn main() { println!(\"This should not be formatted.\"); }");
//     let mut child = Command::new("./target/debug/rustfmt")
//         .stdin(Stdio::piped())
//         .stdout(Stdio::piped())
//         .arg("--config-path=./tests/config/disable_all_formatting.toml")
//         .spawn()
//         .expect("failed to execute child");

//     {
//         let stdin = child.stdin.as_mut().expect("failed to get stdin");
//         stdin
//             .write_all(input.as_bytes())
//             .expect("failed to write stdin");
//     }
//     let output = child.wait_with_output().expect("failed to wait on child");
//     assert!(output.status.success());
//     assert!(output.stderr.is_empty());
//     assert_eq!(input, String::from_utf8(output.stdout).unwrap());
// }

#[test]
fn format_lines_errors_are_reported() {
    let long_identifier = String::from_utf8(vec![b'a'; 239]).unwrap();
    let input = Input::Text(format!("fn {}() {{}}", long_identifier));
    let config = Config::default();
    let (error_summary, _file_map, _report) =
        format_input::<io::Stdout>(input, &config, None).unwrap();
    assert!(error_summary.has_formatting_errors());
}

// For each file, run rustfmt and collect the output.
// Returns the number of files checked and the number of failures.
fn check_files<I>(files: I) -> (Vec<FormatReport>, u32, u32)
where
    I: Iterator<Item = PathBuf>,
{
    let mut count = 0;
    let mut fails = 0;
    let mut reports = vec![];

    for file_name in files.filter(|f| f.extension().map_or(false, |f| f == "rs")) {
        debug!("Testing '{}'...", file_name.display());

        match idempotent_check(file_name) {
            Ok(ref report) if report.has_warnings() => {
                print!("{}", report);
                fails += 1;
            }
            Ok(report) => reports.push(report),
            Err(err) => {
                if let IdempotentCheckError::Mismatch(msg) = err {
                    print_mismatches(msg);
                }
                fails += 1;
            }
        }

        count += 1;
    }

    (reports, count, fails)
}

fn print_mismatches(result: HashMap<PathBuf, Vec<Mismatch>>) {
    let mut t = term::stdout().unwrap();

    for (file_name, diff) in result {
        print_diff(
            diff,
            |line_num| format!("\nMismatch at {}:{}:", file_name.display(), line_num),
            Color::Auto,
        );
    }

    t.reset().unwrap();
}

fn read_config(filename: &Path) -> Config {
    let sig_comments = read_significant_comments(filename);
    // Look for a config file... If there is a 'config' property in the significant comments, use
    // that. Otherwise, if there are no significant comments at all, look for a config file with
    // the same name as the test file.
    let mut config = if !sig_comments.is_empty() {
        get_config(sig_comments.get("config").map(Path::new))
    } else {
        get_config(filename.with_extension("toml").file_name().map(Path::new))
    };

    for (key, val) in &sig_comments {
        if key != "target" && key != "config" {
            config.override_value(key, val);
        }
    }

    // Don't generate warnings for to-do items.
    config.set().report_todo(ReportTactic::Never);

    config
}

fn format_file<P: Into<PathBuf>>(filepath: P, config: &Config) -> (Summary, FileMap, FormatReport) {
    let filepath = filepath.into();
    let input = Input::File(filepath);
    format_input::<io::Stdout>(input, config, None).unwrap()
}

pub enum IdempotentCheckError {
    Mismatch(HashMap<PathBuf, Vec<Mismatch>>),
    Parse,
}

pub fn idempotent_check(filename: PathBuf) -> Result<FormatReport, IdempotentCheckError> {
    let sig_comments = read_significant_comments(&filename);
    let config = read_config(&filename);
    let (error_summary, file_map, format_report) = format_file(filename, &config);
    if error_summary.has_parsing_errors() {
        return Err(IdempotentCheckError::Parse);
    }

    let mut write_result = HashMap::new();
    for &(ref filename, ref text) in &file_map {
        let mut v = Vec::new();
        // Won't panic, as we're not doing any IO.
        write_system_newlines(&mut v, text, &config).unwrap();
        // Won't panic, we are writing correct utf8.
        let one_result = String::from_utf8(v).unwrap();
        if let FileName::Real(ref filename) = *filename {
            write_result.insert(filename.to_owned(), one_result);
        }
    }

    let target = sig_comments.get("target").map(|x| &(*x)[..]);

    handle_result(write_result, target).map(|_| format_report)
}

// Reads test config file using the supplied (optional) file name. If there's no file name or the
// file doesn't exist, just return the default config. Otherwise, the file must be read
// successfully.
fn get_config(config_file: Option<&Path>) -> Config {
    let config_file_name = match config_file {
        None => return Default::default(),
        Some(file_name) => {
            let mut full_path = PathBuf::from("tests/config/");
            full_path.push(file_name);
            if !full_path.exists() {
                return Default::default();
            };
            full_path
        }
    };

    let mut def_config_file = fs::File::open(config_file_name).expect("Couldn't open config");
    let mut def_config = String::new();
    def_config_file
        .read_to_string(&mut def_config)
        .expect("Couldn't read config");

    Config::from_toml(&def_config).expect("Invalid toml")
}

// Reads significant comments of the form: // rustfmt-key: value
// into a hash map.
fn read_significant_comments(file_name: &Path) -> HashMap<String, String> {
    let file =
        fs::File::open(file_name).expect(&format!("Couldn't read file {}", file_name.display()));
    let reader = BufReader::new(file);
    let pattern = r"^\s*//\s*rustfmt-([^:]+):\s*(\S+)";
    let regex = regex::Regex::new(pattern).expect("Failed creating pattern 1");

    // Matches lines containing significant comments or whitespace.
    let line_regex = regex::Regex::new(r"(^\s*$)|(^\s*//\s*rustfmt-[^:]+:\s*\S+)")
        .expect("Failed creating pattern 2");

    reader
        .lines()
        .map(|line| line.expect("Failed getting line"))
        .take_while(|line| line_regex.is_match(line))
        .filter_map(|line| {
            regex.captures_iter(&line).next().map(|capture| {
                (
                    capture
                        .get(1)
                        .expect("Couldn't unwrap capture")
                        .as_str()
                        .to_owned(),
                    capture
                        .get(2)
                        .expect("Couldn't unwrap capture")
                        .as_str()
                        .to_owned(),
                )
            })
        })
        .collect()
}

// Compare output to input.
// TODO: needs a better name, more explanation.
fn handle_result(
    result: HashMap<PathBuf, String>,
    target: Option<&str>,
) -> Result<(), IdempotentCheckError> {
    let mut failures = HashMap::new();

    for (file_name, fmt_text) in result {
        // If file is in tests/source, compare to file with same name in tests/target.
        let target = get_target(&file_name, target);
        let open_error = format!("Couldn't open target {:?}", &target);
        let mut f = fs::File::open(&target).expect(&open_error);

        let mut text = String::new();
        let read_error = format!("Failed reading target {:?}", &target);
        f.read_to_string(&mut text).expect(&read_error);

        // Ignore LF and CRLF difference for Windows.
        if !string_eq_ignore_newline_repr(&fmt_text, &text) {
            let diff = make_diff(&text, &fmt_text, DIFF_CONTEXT_SIZE);
            assert!(
                !diff.is_empty(),
                "Empty diff? Maybe due to a missing a newline at the end of a file?"
            );
            failures.insert(file_name, diff);
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(IdempotentCheckError::Mismatch(failures))
    }
}

// Map source file paths to their target paths.
fn get_target(file_name: &Path, target: Option<&str>) -> PathBuf {
    if let Some(n) = file_name
        .components()
        .position(|c| c.as_os_str() == "source")
    {
        let mut target_file_name = PathBuf::new();
        for (i, c) in file_name.components().enumerate() {
            if i == n {
                target_file_name.push("target");
            } else {
                target_file_name.push(c.as_os_str());
            }
        }
        if let Some(replace_name) = target {
            target_file_name.with_file_name(replace_name)
        } else {
            target_file_name
        }
    } else {
        // This is either and idempotence check or a self check
        file_name.to_owned()
    }
}

#[test]
fn rustfmt_diff_make_diff_tests() {
    let diff = make_diff("a\nb\nc\nd", "a\ne\nc\nd", 3);
    assert_eq!(
        diff,
        vec![
            Mismatch {
                line_number: 1,
                lines: vec![
                    DiffLine::Context("a".into()),
                    DiffLine::Resulting("b".into()),
                    DiffLine::Expected("e".into()),
                    DiffLine::Context("c".into()),
                    DiffLine::Context("d".into()),
                ],
            },
        ]
    );
}

#[test]
fn rustfmt_diff_no_diff_test() {
    let diff = make_diff("a\nb\nc\nd", "a\nb\nc\nd", 3);
    assert_eq!(diff, vec![]);
}

// Compare strings without distinguishing between CRLF and LF
fn string_eq_ignore_newline_repr(left: &str, right: &str) -> bool {
    let left = CharsIgnoreNewlineRepr(left.chars().peekable());
    let right = CharsIgnoreNewlineRepr(right.chars().peekable());
    left.eq(right)
}

struct CharsIgnoreNewlineRepr<'a>(Peekable<Chars<'a>>);

impl<'a> Iterator for CharsIgnoreNewlineRepr<'a> {
    type Item = char;
    fn next(&mut self) -> Option<char> {
        self.0.next().map(|c| {
            if c == '\r' {
                if *self.0.peek().unwrap_or(&'\0') == '\n' {
                    self.0.next();
                    '\n'
                } else {
                    '\r'
                }
            } else {
                c
            }
        })
    }
}

#[test]
fn string_eq_ignore_newline_repr_test() {
    assert!(string_eq_ignore_newline_repr("", ""));
    assert!(!string_eq_ignore_newline_repr("", "abc"));
    assert!(!string_eq_ignore_newline_repr("abc", ""));
    assert!(string_eq_ignore_newline_repr("a\nb\nc\rd", "a\nb\r\nc\rd"));
    assert!(string_eq_ignore_newline_repr("a\r\n\r\n\r\nb", "a\n\n\nb"));
    assert!(!string_eq_ignore_newline_repr("a\r\nbcd", "a\nbcdefghijk"));
}
