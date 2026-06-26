use std::env;
use std::path::PathBuf;
use cosmic::desktop::{fde, IconSourceExt};
use cosmic::widget::icon;

#[test]
fn test_xdg_data_dirs_contains_flatpak() {
    let xdg_dirs = env::var("XDG_DATA_DIRS").unwrap_or_default();
    println!("XDG_DATA_DIRS: {}", xdg_dirs);

    // At least on systems with flatpak, this should be true
    let has_flatpak = xdg_dirs.contains("flatpak/exports/share");
    println!("Contains flatpak exports: {}", has_flatpak);

    if has_flatpak {
        println!("✓ Flatpak exports are in XDG_DATA_DIRS");
    } else {
        println!("⚠ Flatpak exports NOT in XDG_DATA_DIRS - icons may not be found automatically");
    }
}

#[test]
fn test_steam_icon_exists() {
    let home = env::var("HOME").expect("HOME not set");
    let icon_path = format!(
        "{}/.local/share/flatpak/exports/share/icons/hicolor/256x256/apps/com.valvesoftware.Steam.png",
        home
    );

    println!("Checking: {}", icon_path);
    assert!(
        std::path::Path::new(&icon_path).exists(),
        "Steam icon should exist at {}",
        icon_path
    );
    println!("✓ Steam icon file exists");
}

#[test]
fn test_freedesktop_desktop_entry_icon_source() {
    let test_cases = vec![
        ("com.valvesoftware.Steam", "Steam flatpak icon"),
        ("firefox", "Firefox icon (if installed)"),
        ("application-x-executable", "Generic fallback icon"),
    ];

    for (icon_name, description) in test_cases {
        println!("\nTesting: {} ({})", icon_name, description);

        // This is how the code currently works
        let icon_source = fde::IconSource::from_unknown(icon_name);
        println!("  Created IconSource for: {}", icon_name);

        // Try to use as_cosmic_icon - this is where freedesktop-icons is used internally
        let _cosmic_icon = icon_source.as_cosmic_icon();
        println!("  ✓ Converted to cosmic icon (this uses freedesktop-icons internally)");

        // Note: We can't easily test if the icon was actually found without rendering it,
        // but we can verify the API works
    }
}

#[test]
fn test_icon_from_name_actually_finds_steam() {
    println!("\n=== Testing icon::from_name() lookup for Steam ===");

    let icon_name = "com.valvesoftware.Steam";
    println!("Looking up: {}", icon_name);

    // This is what the actual code uses
    let named_icon = icon::from_name(icon_name).size(128);

    // Try to get the actual resolved path
    // The Named struct has a path() method that uses freedesktop_icons::lookup()
    let icon_path = named_icon.path();

    println!("Result from icon::from_name().path():");
    match icon_path {
        Some(path) => {
            println!("  ✓ Found: {}", path.display());
            assert!(
                path.exists(),
                "Path returned but file doesn't exist: {}",
                path.display()
            );
            println!("  ✓ File exists");

            // Verify it's actually a Steam icon
            let path_str = path.to_string_lossy();
            assert!(
                path_str.contains("Steam") || path_str.contains("steam"),
                "Path doesn't appear to be a Steam icon: {}",
                path_str
            );
            println!("  ✓ Path contains 'Steam'");
        }
        None => {
            // If not found, let's try with different sizes to see what works
            println!("  ✗ Not found with size 128");
            println!("\nTrying different sizes:");

            let sizes = [16, 24, 32, 48, 64, 96, 128, 256, 512];
            let mut found_any = false;

            for size in sizes {
                let named = icon::from_name(icon_name).size(size);
                if let Some(path) = named.path() {
                    println!("  ✓ Found at size {}: {}", size, path.display());
                    found_any = true;
                }
            }

            if !found_any {
                println!("\nDEBUG: XDG_DATA_DIRS = {}", env::var("XDG_DATA_DIRS").unwrap_or_default());

                // Check if the file exists manually
                let home = env::var("HOME").expect("HOME not set");
                let manual_path = format!(
                    "{}/.local/share/flatpak/exports/share/icons/hicolor/256x256/apps/com.valvesoftware.Steam.png",
                    home
                );
                println!("Manual check: {}", manual_path);
                println!("Manual path exists: {}", std::path::Path::new(&manual_path).exists());
            }

            panic!("freedesktop-icons did not find Steam icon for '{}'", icon_name);
        }
    }
}

#[test]
fn test_steam_desktop_file_icon_field() {
    println!("\n=== Testing Steam desktop file parsing ===");

    let desktop_file_path =
        PathBuf::from(env::var("HOME").expect("HOME not set"))
            .join(".local/share/flatpak/exports/share/applications/com.valvesoftware.Steam.desktop");

    println!("Desktop file: {}", desktop_file_path.display());
    assert!(
        desktop_file_path.exists(),
        "Steam desktop file should exist at {}",
        desktop_file_path.display()
    );

    // Parse the desktop file
    let desktop_entry = fde::DesktopEntry::from_path::<String>(
        desktop_file_path.clone(),
        None,
    )
    .expect("Failed to parse desktop file");

    println!("Desktop entry ID: {}", desktop_entry.id());
    println!("Desktop entry name: {:?}", desktop_entry.name::<String>(&[]));

    // Check the icon field
    let icon = desktop_entry.icon();
    println!("Icon field from desktop file: {:?}", icon);

    match icon {
        Some(icon_name) => {
            println!("  ✓ Icon specified: {}", icon_name);

            // Verify it's the expected value
            assert_eq!(
                icon_name, "com.valvesoftware.Steam",
                "Icon should be 'com.valvesoftware.Steam'"
            );

            // Now check if this icon can be found by freedesktop-icons
            println!("\nTesting if this icon can be looked up:");
            let named_icon = icon::from_name(icon_name).size(128);
            let icon_path = named_icon.path();

            match icon_path {
                Some(path) => {
                    println!("  ✓ Icon lookup succeeded: {}", path.display());
                    assert!(path.exists(), "Icon path should exist");
                }
                None => {
                    panic!("Icon '{}' from desktop file could not be found by freedesktop-icons", icon_name);
                }
            }
        }
        None => {
            panic!("Desktop file should have an Icon field");
        }
    }
}

#[test]
fn test_icon_lookup_without_size_on_named() {
    println!("\n=== Testing if size() on Named is needed for lookup ===");

    let icon_name = "com.valvesoftware.Steam";

    // Test 1: With size on Named (what my test does)
    println!("\nTest 1: With .size() on Named");
    let with_size = icon::from_name(icon_name).size(128);
    let path_with_size = with_size.path();
    println!("  Result: {:?}", path_with_size.as_ref().map(|p| p.display().to_string()));

    // Test 2: Without size on Named (what the code might be doing)
    println!("\nTest 2: Without .size() on Named");
    let without_size = icon::from_name(icon_name);
    let path_without_size = without_size.path();
    println!("  Result: {:?}", path_without_size.as_ref().map(|p| p.display().to_string()));

    assert!(
        path_with_size.is_some() || path_without_size.is_some(),
        "At least one method should find the icon"
    );
}
