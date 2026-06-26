#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! freedesktop_desktop_entry = "0.7"
//! ```

use std::env;

fn main() {
    println!("=== Icon Lookup Test ===\n");

    // Test 1: Check XDG_DATA_DIRS
    println!("1. XDG_DATA_DIRS:");
    if let Ok(dirs) = env::var("XDG_DATA_DIRS") {
        println!("   {}", dirs);
        for dir in dirs.split(':') {
            let icons_path = format!("{}/icons", dir);
            println!("   - Checking: {}", icons_path);
            if std::path::Path::new(&icons_path).exists() {
                println!("     ✓ exists");
            } else {
                println!("     ✗ does not exist");
            }
        }
    } else {
        println!("   Not set (using defaults)");
    }

    println!("\n2. Looking for Steam icon files:");
    let search_paths = [
        format!("{}/.local/share/flatpak/exports/share/icons", env::var("HOME").unwrap()),
        "/var/lib/flatpak/exports/share/icons".to_string(),
        format!("{}/.local/share/icons", env::var("HOME").unwrap()),
        "/usr/share/icons".to_string(),
    ];

    for base_path in &search_paths {
        println!("\n   Searching in: {}", base_path);
        let steam_icons = std::fs::read_dir(format!("{}/hicolor", base_path))
            .ok()
            .and_then(|entries| {
                let mut found = Vec::new();
                for entry in entries.flatten() {
                    if let Ok(apps) = std::fs::read_dir(entry.path().join("apps")) {
                        for app in apps.flatten() {
                            let name = app.file_name();
                            let name_str = name.to_string_lossy();
                            if name_str.contains("Steam") || name_str.contains("steam") {
                                found.push(app.path());
                            }
                        }
                    }
                }
                Some(found)
            });

        if let Some(icons) = steam_icons {
            if !icons.is_empty() {
                println!("   ✓ Found {} Steam icon(s):", icons.len());
                for icon in icons.iter().take(3) {
                    println!("     - {}", icon.display());
                }
            } else {
                println!("   ✗ No Steam icons found");
            }
        } else {
            println!("   ✗ Directory not accessible");
        }
    }

    println!("\n3. Testing freedesktop_desktop_entry IconSource:");
    let test_icons = vec![
        "com.valvesoftware.Steam",
        "steam",
        "Steam",
        "/home/fred/.local/share/flatpak/exports/share/icons/hicolor/256x256/apps/com.valvesoftware.Steam.png",
    ];

    for icon_name in test_icons {
        println!("\n   Testing: '{}'", icon_name);
        println!("   - Type: {}", if icon_name.starts_with('/') { "absolute path" } else { "icon name" });

        // Check if file exists for paths
        if icon_name.starts_with('/') {
            if std::path::Path::new(icon_name).exists() {
                println!("   - File exists: ✓");
            } else {
                println!("   - File exists: ✗");
            }
        }
    }
}
