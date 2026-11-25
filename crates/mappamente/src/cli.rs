//! The `mappamente` console binary: subcommands plus GUI fallback.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use mappamente_core::export::svg::{self, SvgOptions};
use mappamente_core::io::{self, json};
use mappamente_core::{LayoutKind, MindMap};

#[derive(Debug, Parser)]
#[command(
    name = "mappamente",
    version,
    about = "Lightweight, open-source mind maps",
    long_about = "Create, convert and render .mappa mind maps.\n\
                  With no subcommand, opens the editor window."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a new .mappa file
    New(NewArgs),
    /// Convert between .mappa, .md and .svg
    Convert(ConvertArgs),
    /// Render a map to SVG
    Render(RenderArgs),
    /// Open the editor window
    Gui(GuiArgs),
}

#[derive(Debug, Args)]
struct NewArgs {
    /// Destination file (`.mappa` is added when the extension is missing)
    path: PathBuf,

    /// Map title
    #[arg(long, default_value = "Untitled map")]
    title: String,

    /// Text of the root node
    #[arg(long, default_value = "Central idea")]
    root: String,

    /// Layout algorithm
    #[arg(long, value_enum, default_value = "radial")]
    layout: LayoutArg,

    /// Overwrite an existing file
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct ConvertArgs {
    /// Input file (.mappa, .json, .md, .markdown)
    input: PathBuf,

    /// Output file (.mappa, .json, .md, .markdown, .svg)
    output: PathBuf,

    /// Override the layout stored in the map
    #[arg(long, value_enum)]
    layout: Option<LayoutArg>,

    /// Override the theme stored in the map
    #[arg(long)]
    theme: Option<String>,

    /// Overwrite an existing file
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct RenderArgs {
    /// Input file (.mappa, .json, .md, .markdown)
    input: PathBuf,

    /// Output SVG file (`.svg` is added when the extension is missing)
    output: PathBuf,

    /// Override the layout stored in the map
    #[arg(long, value_enum)]
    layout: Option<LayoutArg>,

    /// Theme: dark, light, paper or ocean
    #[arg(long)]
    theme: Option<String>,

    /// Padding around the drawing, in pixels
    #[arg(long, default_value_t = 40.0)]
    margin: f32,

    /// Overwrite an existing file
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
struct GuiArgs {
    /// Map to open (.mappa, .json, .md, .markdown)
    file: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum LayoutArg {
    Tree,
    Radial,
    Logical,
}

impl From<LayoutArg> for LayoutKind {
    fn from(value: LayoutArg) -> Self {
        match value {
            LayoutArg::Tree => Self::Tree,
            LayoutArg::Radial => Self::Radial,
            LayoutArg::Logical => Self::Logical,
        }
    }
}

/// Parses arguments and runs the requested command.
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        None => crate::gui::run(None),
        Some(Command::Gui(args)) => crate::gui::run(args.file),
        Some(Command::New(args)) => new_map(args),
        Some(Command::Convert(args)) => convert(args),
        Some(Command::Render(args)) => render(args),
    }
}

fn new_map(args: NewArgs) -> Result<()> {
    let path = with_default_extension(args.path, "mappa");
    if !matches!(io::extension(&path).as_deref(), Some("mappa" | "json")) {
        bail!("{}: `mappamente new` writes .mappa files", path.display());
    }
    ensure_writable(&path, args.force)?;
    create_parent(&path)?;

    let mut map = MindMap::new(args.title, args.root);
    map.set_layout(args.layout.into());
    json::save(&path, &map).with_context(|| format!("cannot write {}", path.display()))?;
    println!("Created {}", path.display());
    Ok(())
}

fn convert(args: ConvertArgs) -> Result<()> {
    let mut map = io::load_path(&args.input)
        .with_context(|| format!("cannot read {}", args.input.display()))?;
    if let Some(layout) = args.layout {
        map.set_layout(layout.into());
    }
    if let Some(theme) = args.theme {
        map.theme = theme;
    }

    let output = with_default_extension(args.output, "mappa");
    ensure_supported_output(&output)?;
    ensure_writable(&output, args.force)?;
    create_parent(&output)?;

    if io::extension(&output).as_deref() == Some("svg") {
        let svg = svg::to_svg(
            &map,
            &SvgOptions {
                margin: 40.0,
                ..SvgOptions::default()
            },
        );
        json::write_atomic(&output, svg.as_bytes())
            .with_context(|| format!("cannot write {}", output.display()))?;
    } else {
        io::save_path(&output, &map)
            .with_context(|| format!("cannot write {}", output.display()))?;
    }

    println!("Converted {} -> {}", args.input.display(), output.display());
    Ok(())
}

fn render(args: RenderArgs) -> Result<()> {
    let map = io::load_path(&args.input)
        .with_context(|| format!("cannot read {}", args.input.display()))?;
    let output = with_default_extension(args.output, "svg");
    if io::extension(&output).as_deref() != Some("svg") {
        bail!(
            "{}: `mappamente render` writes .svg files",
            output.display()
        );
    }
    ensure_writable(&output, args.force)?;
    create_parent(&output)?;

    let options = SvgOptions {
        layout: args.layout.map(Into::into),
        theme: args.theme,
        margin: args.margin.max(0.0),
        ..SvgOptions::default()
    };
    let svg = svg::to_svg(&map, &options);
    json::write_atomic(&output, svg.as_bytes())
        .with_context(|| format!("cannot write {}", output.display()))?;
    println!("Rendered {}", output.display());
    Ok(())
}

fn ensure_supported_output(path: &Path) -> Result<()> {
    match io::extension(path).as_deref() {
        Some("mappa" | "json" | "md" | "markdown" | "svg") => Ok(()),
        Some(other) => bail!(
            "{}: unsupported output format .{other} (expected .mappa, .json, .md, .markdown or .svg)",
            path.display()
        ),
        None => bail!("{}: the output file has no extension", path.display()),
    }
}

fn ensure_writable(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !force {
        bail!(
            "{} already exists; pass --force to overwrite",
            path.display()
        );
    }
    Ok(())
}

fn create_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    Ok(())
}

fn with_default_extension(mut path: PathBuf, extension: &str) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension(extension);
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_extensions_only_when_missing() {
        assert_eq!(
            with_default_extension(PathBuf::from("map"), "mappa"),
            PathBuf::from("map.mappa")
        );
        assert_eq!(
            with_default_extension(PathBuf::from("map.md"), "mappa"),
            PathBuf::from("map.md")
        );
    }

    #[test]
    fn layout_argument_converts() {
        assert_eq!(LayoutKind::from(LayoutArg::Tree), LayoutKind::Tree);
        assert_eq!(LayoutKind::from(LayoutArg::Radial), LayoutKind::Radial);
        assert_eq!(LayoutKind::from(LayoutArg::Logical), LayoutKind::Logical);
    }
}
