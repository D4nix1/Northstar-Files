//! Lists uninstall commands registered by Windows. Removal is delegated to each
//! application's own uninstaller; Northstar never guesses at files to delete.

#[derive(Debug, Clone)]
pub struct InstalledProgram {
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub uninstall_command: String,
}

#[cfg(windows)]
pub fn list_installed() -> Vec<InstalledProgram> {
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
    };

    let locations = [
        (
            HKEY_CURRENT_USER,
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ),
    ];
    let mut programs = Vec::new();
    for (hive, path) in locations {
        let Ok(root) = RegKey::predef(hive).open_subkey(path) else {
            continue;
        };
        for key_name in root.enum_keys().filter_map(Result::ok) {
            let Ok(key) = root.open_subkey(&key_name) else {
                continue;
            };
            let Ok(name) = key.get_value::<String, _>("DisplayName") else {
                continue;
            };
            let name = name.trim();
            if name.is_empty() || key.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1 {
                continue;
            }
            let command = key
                .get_value::<String, _>("UninstallString")
                .or_else(|_| key.get_value::<String, _>("QuietUninstallString"));
            let Ok(uninstall_command) = command else {
                continue;
            };
            programs.push(InstalledProgram {
                name: name.to_owned(),
                publisher: key.get_value("Publisher").unwrap_or_default(),
                version: key.get_value("DisplayVersion").unwrap_or_default(),
                uninstall_command,
            });
        }
    }
    programs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    programs.dedup_by(|a, b| {
        a.name.eq_ignore_ascii_case(&b.name) && a.publisher.eq_ignore_ascii_case(&b.publisher)
    });
    programs
}

#[cfg(windows)]
pub fn launch_uninstaller(command: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let (program, args) = split_windows_command(command)?;
    let program = expand_environment_variables(&program);
    let mut process = std::process::Command::new(program);
    process
        .args(args)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW; the app's uninstaller owns its UI.
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Não foi possível abrir o desinstalador registrado: {error}"))
}

#[cfg(windows)]
fn split_windows_command(command: &str) -> Result<(String, Vec<String>), String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("O Windows não registrou um comando de desinstalação".into());
    }
    let (program, remainder) = if let Some(quoted) = command.strip_prefix('"') {
        let end = quoted
            .find('"')
            .ok_or("O comando registrado contém aspas inválidas")?;
        (quoted[..end].to_owned(), &quoted[end + 1..])
    } else {
        let lower = command.to_ascii_lowercase();
        let end = [".exe", ".msi", ".com", ".bat", ".cmd"]
            .iter()
            .filter_map(|extension| lower.find(extension).map(|index| index + extension.len()))
            .min()
            .unwrap_or_else(|| command.find(char::is_whitespace).unwrap_or(command.len()));
        (command[..end].trim().to_owned(), &command[end..])
    };
    if program.is_empty() {
        return Err("O comando registrado não informa o executável".into());
    }
    Ok((program, parse_windows_arguments(remainder)))
}

#[cfg(windows)]
fn parse_windows_arguments(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let mut slashes = 1;
                while chars.peek() == Some(&'\\') {
                    chars.next();
                    slashes += 1;
                }
                if chars.peek() == Some(&'"') {
                    chars.next();
                    current.extend(std::iter::repeat_n('\\', slashes / 2));
                    if slashes % 2 == 1 {
                        current.push('"');
                    } else {
                        quoted = !quoted;
                    }
                } else {
                    current.extend(std::iter::repeat_n('\\', slashes));
                }
            }
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[cfg(windows)]
fn expand_environment_variables(value: &str) -> String {
    let mut output = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        output.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let name = &after[..end];
        if let Some(value) = std::env::var_os(name) {
            output.push_str(&value.to_string_lossy());
        } else {
            output.push_str(&rest[start..start + end + 2]);
        }
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    output
}

#[cfg(not(windows))]
pub fn list_installed() -> Vec<InstalledProgram> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn launch_uninstaller(_command: &str) -> Result<(), String> {
    Err("O desinstalador integrado está disponível no Windows".into())
}
