// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
use clap::{Parser, Subcommand};
use isr_core::{render, RenderOptions};
use std::{fs, path::PathBuf};

#[derive(Parser)]
#[command(about = "Offscreen script-free HTML/CSS renderer")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Render {
        input: PathBuf,
        #[arg(long, default_value_t = 412, value_parser = clap::value_parser!(u32).range(1..=8192))]
        width: u32,
        #[arg(long, required = true)]
        height_auto: bool,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        layout: PathBuf,
    },
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Command::Render { input, width, out, layout, .. } = Cli::parse().command;
    let rendered = render(&fs::read_to_string(input)?, RenderOptions { width })?;
    for path in [&out, &layout] {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
    }
    let mut encoder = png::Encoder::new(fs::File::create(out)?, rendered.width, rendered.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&rendered.rgba)?;
    writer.finish()?;
    fs::write(layout, serde_json::to_vec_pretty(&rendered.layout())?)?;
    Ok(())
}
