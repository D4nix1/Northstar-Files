# Northstar Files

**Experimental open-source file manager for Windows.** Built in Rust with a native egui interface. This project is still under active development and may contain bugs; please send feedback and reproducible bug reports.

## Current features

- Tabs, optional dual-pane browsing, common Windows locations, and file operations.
- Custom application and file-type icons; Portuguese/English interface selection from the system locale (English fallback for other locales).
- Folder-size scans, content-hash duplicate detection, and folder comparison.
- Advanced name, extension, size, and creation-date filters, plus saved filters.
- Recursive text search for UTF-8 files up to 8 MiB per file.
- Batch rename preview, PowerShell/CMD/WSL launch in the current folder, and Rhai action plugins.
- Windows installed-program list that launches each app's registered uninstaller.

## Experimental warning

Back up important files before testing. The uninstaller delegates to the command registered by each application; Northstar does not claim it can detect or safely remove every leftover file. Large transfer queue controls, universal previews, NTFS permission editing, and version history are not available yet.

## Install on Windows

Download `Northstar-Files-Setup.exe` from GitHub Releases. The installer places the app under Program Files, adds a Start menu shortcut, and offers a Desktop shortcut. The portable executable is also available in the release for evaluation.

## Build from source

Requires stable Rust, the MSVC linker, and the Windows SDK resource compiler.

```powershell
cargo run -p northstar-files
cargo build -p northstar-files --release
```

To build the Windows installer, compile the release binary and open `apps/desktop/installer/Northstar-Files.iss` with Inno Setup Compiler.

## Feedback and bugs

Please include Windows version, Northstar version, steps to reproduce, expected behavior, and what happened. Remove personal information from screenshots and logs.

## License

MIT. See [LICENSE-MIT](LICENSE-MIT).
