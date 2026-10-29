// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut width = None;
    let mut scale = 1.0_f32;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scale" => {
                scale = args.next().ok_or("--scale requires a value")?.parse()?;
            }
            _ if arg.starts_with("--scale=") => {
                scale = arg["--scale=".len()..].parse()?;
            }
            _ if !arg.starts_with('-') && width.is_none() => {
                width = Some(arg.parse::<u32>()?);
            }
            _ => return Err(format!("unexpected argument: {arg}").into()),
        }
    }
    for line in isr_bench::run_standard(width.unwrap_or(412), scale, &[])? {
        println!("{line}");
    }
    Ok(())
}
