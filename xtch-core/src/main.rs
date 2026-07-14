//! CLI harness for validating the pipeline without the full Tauri build.
//! Usage: xtch-cli <input_folder_or_cbz> <output.xtch> [portrait|landscape] [none|half|thirds] [fit|stretch]

use std::path::Path;
use xtch_core::{convert_input, Orientation, Settings, Split};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: xtch-cli <input> <output.xtch> [portrait|landscape] [none|half|thirds] [fit|stretch]");
        std::process::exit(2);
    }
    let input = &args[1];
    let output = &args[2];
    let orientation = match args.get(3).map(|s| s.as_str()) {
        Some("landscape") => Orientation::Landscape,
        _ => Orientation::Portrait,
    };
    let split = match args.get(4).map(|s| s.as_str()) {
        Some("half") => Split::Half,
        Some("thirds") => Split::Thirds,
        _ => Split::None,
    };
    let preserve_ratio = !matches!(args.get(5).map(|s| s.as_str()), Some("stretch"));
    let manga_mode = !matches!(args.get(6).map(|s| s.as_str()), Some("ltr"));

    let settings = Settings {
        orientation,
        split,
        preserve_ratio,
        manga_mode,
        ..Default::default()
    };

    match convert_input(Path::new(input), &settings) {
        Ok(bytes) => {
            std::fs::write(output, &bytes).expect("write output");
            println!(
                "OK: wrote {} ({} bytes), {:?}/{:?}, preserve_ratio={}",
                output,
                bytes.len(),
                orientation,
                split,
                preserve_ratio
            );
        }
        Err(e) => {
            eprintln!("ERROR: {}", e);
            std::process::exit(1);
        }
    }
}
