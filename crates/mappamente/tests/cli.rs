//! End-to-end tests for the `mappamente` console binary.

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use mappamente_core::io;
use mappamente_core::model::LayoutKind;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_mappamente")
}

fn temporary_directory(label: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "mappamente-cli-{label}-{}-{}",
        std::process::id(),
        fastrand::u64(..)
    ));
    fs::create_dir_all(&directory).unwrap();
    directory
}

fn run<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(binary()).args(args).output().unwrap()
}

fn assert_success(output: Output) -> String {
    assert!(
        output.status.success(),
        "command failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn assert_failure(output: Output, needle: &str) {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains(&needle.to_lowercase()),
        "stderr did not contain {needle:?}: {stderr}"
    );
}

#[test]
fn new_creates_a_map_and_protects_existing_files() {
    let directory = temporary_directory("new");
    let path = directory.join("demo.mappa");

    let stdout = assert_success(run([
        OsStr::new("new"),
        path.as_os_str(),
        OsStr::new("--title"),
        OsStr::new("Demo"),
        OsStr::new("--root"),
        OsStr::new("Central"),
        OsStr::new("--layout"),
        OsStr::new("tree"),
    ]));
    assert!(stdout.contains("Created"));

    let map = io::json::load(&path).unwrap();
    assert_eq!(map.title, "Demo");
    assert_eq!(map.root.text, "Central");
    assert_eq!(map.layout, LayoutKind::Tree);

    let without_force = run([OsStr::new("new"), path.as_os_str()]);
    assert_failure(without_force, "already exists");

    let forced = run([
        OsStr::new("new"),
        path.as_os_str(),
        OsStr::new("--title"),
        OsStr::new("Replaced"),
        OsStr::new("--force"),
    ]);
    assert_success(forced);
    assert_eq!(io::json::load(&path).unwrap().title, "Replaced");

    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn new_adds_the_native_extension_and_creates_parent_directories() {
    let directory = temporary_directory("extension");
    let path = directory.join("nested").join("map");
    let expected = path.with_extension("mappa");

    assert_success(run([OsStr::new("new"), path.as_os_str()]));
    assert!(expected.exists());

    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn convert_handles_markdown_json_and_svg() {
    let directory = temporary_directory("convert");
    let markdown_path = directory.join("input.md");
    fs::write(
        &markdown_path,
        "# Converted map\n\n- Root\n  - Alpha\n    - Detail\n  - Beta\n",
    )
    .unwrap();

    let mappamente_path = directory.join("output.mappa");
    assert_success(run([
        OsStr::new("convert"),
        markdown_path.as_os_str(),
        mappamente_path.as_os_str(),
    ]));

    let map = io::json::load(&mappamente_path).unwrap();
    assert_eq!(map.title, "Converted map");
    assert_eq!(map.outline(), "- Root\n  - Alpha\n    - Detail\n  - Beta");

    let back_path = directory.join("back.md");
    assert_success(run([
        OsStr::new("convert"),
        mappamente_path.as_os_str(),
        back_path.as_os_str(),
    ]));
    let markdown = fs::read_to_string(&back_path).unwrap();
    assert!(markdown.starts_with("# Converted map"));
    assert!(markdown.contains("    - Detail"));

    let svg_path = directory.join("preview.svg");
    assert_success(run([
        OsStr::new("convert"),
        mappamente_path.as_os_str(),
        svg_path.as_os_str(),
        OsStr::new("--theme"),
        OsStr::new("light"),
    ]));
    let svg = fs::read_to_string(&svg_path).unwrap();
    roxmltree::Document::parse(&svg).expect("valid SVG");
    assert!(svg.contains("Alpha"));

    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn convert_applies_layout_override() {
    let directory = temporary_directory("convert-layout");
    let source = directory.join("source.md");
    fs::write(&source, "# Layout\n\n- Root\n  - A\n  - B\n").unwrap();
    let target = directory.join("target.mappa");

    assert_success(run([
        OsStr::new("convert"),
        source.as_os_str(),
        target.as_os_str(),
        OsStr::new("--layout"),
        OsStr::new("logical"),
    ]));

    assert_eq!(io::json::load(&target).unwrap().layout, LayoutKind::Logical);
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn render_writes_standalone_svg() {
    let directory = temporary_directory("render");
    let map_path = directory.join("map.mappa");
    let mut map = mappamente_core::MindMap::new("Render", "Root label");
    map.add_child(&map.root.id.clone(), "A branch & more")
        .unwrap();
    io::json::save(&map_path, &map).unwrap();

    let svg_without_extension = directory.join("drawing");
    let stdout = assert_success(run([
        OsStr::new("render"),
        map_path.as_os_str(),
        svg_without_extension.as_os_str(),
        OsStr::new("--theme"),
        OsStr::new("ocean"),
        OsStr::new("--layout"),
        OsStr::new("radial"),
    ]));
    assert!(stdout.contains("Rendered"));

    let svg_path = svg_without_extension.with_extension("svg");
    let svg = fs::read_to_string(&svg_path).unwrap();
    roxmltree::Document::parse(&svg).expect("valid SVG");
    assert!(svg.starts_with("<?xml"));
    assert!(svg.contains("Root label"));
    assert!(svg.contains("A branch &amp; more"));
    assert!(svg.contains("#071a23"), "ocean theme background");

    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn errors_are_clear_for_unsupported_formats() {
    let directory = temporary_directory("errors");
    let source = directory.join("notes.txt");
    fs::write(&source, "hello").unwrap();
    let target = directory.join("out.mappa");

    assert_failure(
        run([
            OsStr::new("convert"),
            source.as_os_str(),
            target.as_os_str(),
        ]),
        "unsupported input format",
    );

    let map_path = directory.join("map.mappa");
    assert_success(run([OsStr::new("new"), map_path.as_os_str()]));
    let bad_output = directory.join("bad.png");
    assert_failure(
        run([
            OsStr::new("convert"),
            map_path.as_os_str(),
            bad_output.as_os_str(),
        ]),
        "unsupported output format",
    );

    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn help_and_version_work() {
    let help = assert_success(run([OsStr::new("--help")]));
    assert!(help.contains("Usage"));
    assert!(help.contains("new"));
    assert!(help.contains("convert"));
    assert!(help.contains("render"));

    let version = assert_success(run([OsStr::new("--version")]));
    assert!(version.contains("mappa"));
}
