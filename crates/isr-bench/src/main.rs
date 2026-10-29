// Copyright 2026 g1BSsQ. Licensed under Apache-2.0.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let width = std::env::args().nth(1)
        .map(|value| value.parse::<u32>())
        .transpose()?.unwrap_or(412);
    for line in isr_bench::run_standard(width, 1.0, &[])? {
        println!("{line}");
    }
    Ok(())
}
