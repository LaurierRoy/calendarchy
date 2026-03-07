use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

const PLIST_LABEL: &str = "com.calendarchy.hotkey";

struct Terminal {
    name: &'static str,
    app: &'static str,
    launch_fmt: &'static str, // {} is replaced with the calendarchy path
}

const TERMINALS: &[Terminal] = &[
    Terminal { name: "Ghostty", app: "Ghostty.app", launch_fmt: "open -na Ghostty.app --args -e {}" },
    Terminal { name: "iTerm2", app: "iTerm.app", launch_fmt: "open -na iTerm.app --args {}" },
    Terminal { name: "Alacritty", app: "Alacritty.app", launch_fmt: "open -na Alacritty.app --args -e {}" },
    Terminal { name: "Kitty", app: "kitty.app", launch_fmt: "open -na kitty.app --args {}" },
    Terminal { name: "WezTerm", app: "WezTerm.app", launch_fmt: "open -na WezTerm.app --args start -- {}" },
    Terminal { name: "Terminal", app: "Terminal.app", launch_fmt: "open -a Terminal {}" },
];

fn detect_terminals() -> Vec<&'static Terminal> {
    TERMINALS.iter().filter(|t| {
        PathBuf::from(format!("/Applications/{}", t.app)).exists()
    }).collect()
}

fn find_hotkey_binary() -> Option<PathBuf> {
    // Check next to the current executable first
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe.parent().unwrap().join("calendarchy-hotkey");
        if sibling.exists() {
            return Some(sibling);
        }
    }
    // Check PATH
    if let Ok(output) = Command::new("which").arg("calendarchy-hotkey").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(PathBuf::from(path));
            }
        }
    }
    None
}

fn plist_path() -> PathBuf {
    dirs::home_dir().unwrap()
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", PLIST_LABEL))
}

fn calendarchy_path() -> String {
    std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("/opt/homebrew/bin/calendarchy"))
        .to_string_lossy()
        .to_string()
}

pub fn run_setup() -> Result<(), Box<dyn std::error::Error>> {
    println!("Calendarchy Hotkey Setup");
    println!("========================\n");

    // Find hotkey binary
    let hotkey_bin = find_hotkey_binary().ok_or(
        "calendarchy-hotkey binary not found. Make sure it's installed (brew reinstall calendarchy)."
    )?;
    println!("Found hotkey helper: {}\n", hotkey_bin.display());

    // Detect terminals
    let terminals = detect_terminals();
    if terminals.is_empty() {
        return Err("No supported terminals found in /Applications.".into());
    }

    // Ask user to pick
    println!("Available terminals:");
    for (i, t) in terminals.iter().enumerate() {
        println!("  {}. {}", i + 1, t.name);
    }
    print!("\nSelect terminal [1]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let choice: usize = input.trim().parse().unwrap_or(1);
    if choice < 1 || choice > terminals.len() {
        return Err("Invalid selection.".into());
    }

    let terminal = terminals[choice - 1];
    let cal_path = calendarchy_path();
    let launch_cmd = terminal.launch_fmt.replace("{}", &cal_path);

    println!("\nUsing {} with shortcut Cmd+Shift+J", terminal.name);
    println!("Launch command: {}\n", launch_cmd);

    // Generate LaunchAgent plist
    let plist_dir = dirs::home_dir().unwrap().join("Library/LaunchAgents");
    std::fs::create_dir_all(&plist_dir)?;

    let plist_content = format!(
r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>{command}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>"#,
        label = PLIST_LABEL,
        binary = hotkey_bin.display(),
        command = launch_cmd,
    );

    let plist = plist_path();

    // Unload existing agent if present
    if plist.exists() {
        let _ = Command::new("launchctl").args(["unload", &plist.to_string_lossy()]).output();
    }

    std::fs::write(&plist, &plist_content)?;
    println!("Created {}", plist.display());

    // Load the agent
    let output = Command::new("launchctl")
        .args(["load", &plist.to_string_lossy()])
        .output()?;

    if output.status.success() {
        println!("Hotkey agent loaded successfully!\n");
        println!("Press Cmd+Shift+J from anywhere to launch Calendarchy.");
        println!("\nNote: macOS will ask for Accessibility permission on first use.");
        println!("Grant it in System Settings > Privacy & Security > Accessibility.");
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to load agent: {}", err).into());
    }

    Ok(())
}

pub fn remove_setup() -> Result<(), Box<dyn std::error::Error>> {
    let plist = plist_path();

    if !plist.exists() {
        println!("No hotkey agent found. Nothing to remove.");
        return Ok(());
    }

    // Unload
    let _ = Command::new("launchctl")
        .args(["unload", &plist.to_string_lossy()])
        .output();

    // Delete plist
    std::fs::remove_file(&plist)?;

    println!("Hotkey agent removed.");
    Ok(())
}
