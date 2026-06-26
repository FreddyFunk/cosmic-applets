#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! cosmic-freedesktop-icons = { git = "https://github.com/pop-os/freedesktop-icons", rev = "689c60d4" }
//! ```

use std::env;

fn main() {
    println!("=== Direct freedesktop-icons Test ===\n");

    // Test the actual freedesktop-icons crate lookup
    let test_icons = vec![
        "com.valvesoftware.Steam",
        "steam",
        "application-x-executable",
        "firefox",
    ];

    for icon_name in test_icons {
        println!("\nTesting lookup: '{}'", icon_name);

        // Try default lookup (hicolor theme, size 24, scale 1)
        let result = cosmic_freedesktop_icons::lookup(icon_name).find();
        match result {
            Some(path) => {
                println!("  ✓ Found: {}", path.display());
                if path.exists() {
                    println!("    File exists: ✓");
                } else {
                    println!("    File exists: ✗ (path returned but file missing!)");
                }
            }
            None => {
                println!("  ✗ Not found by freedesktop-icons");

                // Try with different sizes
                println!("  Trying different sizes...");
                for size in [16, 24, 32, 48, 64, 128, 256] {
                    if let Some(path) = cosmic_freedesktop_icons::lookup(icon_name)
                        .with_size(size)
                        .find()
                    {
                        println!("    ✓ Found at size {}: {}", size, path.display());
                        break;
                    }
                }
            }
        }
    }

    println!("\n=== XDG_DATA_DIRS ===");
    if let Ok(dirs) = env::var("XDG_DATA_DIRS") {
        println!("{}", dirs);
    } else {
        println!("Not set");
    }
}
