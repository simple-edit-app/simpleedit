//! The language servers SimpleEdit knows, and how to install them from the
//! Preferences. Nothing is installed by default: until a server is there,
//! go to definition falls back to the pattern search (see `definition`).
//!
//! Servers installed from here live in the application's own folder
//! (`<data dir>/simpleedit/lsp`), so no administrator rights are needed and
//! nothing else on the machine is touched (except Rust's, which rustup
//! manages).

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

/// How a server gets installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installer {
    /// `npm install -g` of these packages.
    Npm(&'static [&'static str]),
    /// `rustup component add`.
    Rustup(&'static str),
    /// `go install` of this package.
    Go(&'static str),
    /// `dotnet tool install` of this tool.
    Dotnet(&'static str),
    /// Only the system's package manager can: the hint says how.
    Manual(&'static str),
}

/// A language server offered in the Preferences.
#[derive(Debug, Clone, Copy)]
pub struct ServerInfo {
    /// The program that is run (and looked for).
    pub binary: &'static str,
    /// What the user sees.
    pub label: &'static str,
    /// The file extensions it serves.
    pub extensions: &'static [&'static str],
    pub installer: Installer,
}

pub const SERVERS: &[ServerInfo] = &[
    ServerInfo {
        binary: "rust-analyzer",
        label: "Rust",
        extensions: &["rs"],
        installer: Installer::Rustup("rust-analyzer"),
    },
    ServerInfo {
        binary: "pyright-langserver",
        label: "Python",
        extensions: &["py", "pyi"],
        installer: Installer::Npm(&["pyright@1"]),
    },
    ServerInfo {
        binary: "typescript-language-server",
        label: "TypeScript / JavaScript",
        extensions: &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"],
        installer: Installer::Npm(&["typescript-language-server@6", "typescript@5"]),
    },
    ServerInfo {
        binary: "gopls",
        label: "Go",
        extensions: &["go"],
        installer: Installer::Go("golang.org/x/tools/gopls@v0.24.0"),
    },
    ServerInfo {
        binary: "bash-language-server",
        label: "Shell",
        extensions: &["sh", "bash"],
        installer: Installer::Npm(&["bash-language-server@5"]),
    },
    ServerInfo {
        binary: "intelephense",
        label: "PHP",
        extensions: &["php"],
        installer: Installer::Npm(&["intelephense@1"]),
    },
    ServerInfo {
        binary: "csharp-ls",
        label: "C#",
        extensions: &["cs"],
        installer: Installer::Dotnet("csharp-ls"),
    },
    ServerInfo {
        binary: "clangd",
        label: "C / C++",
        extensions: &["c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx"],
        installer: Installer::Manual("apt install clangd · brew install llvm"),
    },
    ServerInfo {
        binary: "jdtls",
        label: "Java",
        extensions: &["java"],
        installer: Installer::Manual("brew install jdtls · eclipse.jdt.ls"),
    },
    ServerInfo {
        binary: "lua-language-server",
        label: "Lua",
        extensions: &["lua"],
        installer: Installer::Manual("brew install lua-language-server"),
    },
    ServerInfo {
        binary: "ruby-lsp",
        label: "Ruby",
        extensions: &["rb"],
        installer: Installer::Manual("gem install ruby-lsp"),
    },
    ServerInfo {
        binary: "zls",
        label: "Zig",
        extensions: &["zig"],
        installer: Installer::Manual("brew install zls"),
    },
    ServerInfo {
        binary: "kotlin-language-server",
        label: "Kotlin",
        extensions: &["kt", "kts"],
        installer: Installer::Manual("brew install kotlin-language-server"),
    },
    ServerInfo {
        binary: "sourcekit-lsp",
        label: "Swift",
        extensions: &["swift"],
        installer: Installer::Manual("Swift toolchain"),
    },
];

/// Where the servers installed from the Preferences live.
pub fn managed_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("simpleedit").join("lsp"))
}

/// The folder holding their programs.
pub fn managed_bin_dir() -> Option<PathBuf> {
    managed_dir().map(|d| d.join("bin"))
}

/// The servers of [`SERVERS`] that can be run now (blocking: it runs
/// `rust-analyzer --version`, see `lsp::usable`).
pub fn ready() -> HashSet<&'static str> {
    SERVERS
        .iter()
        .filter(|s| crate::lsp::usable(s.binary))
        .map(|s| s.binary)
        .collect()
}

/// The server worth offering for a file of this extension: one that can be
/// installed from here, while no server for the language can be run.
pub fn offer_for(extension: &str) -> Option<&'static ServerInfo> {
    let info = SERVERS
        .iter()
        .find(|s| s.extensions.contains(&extension))
        .filter(|s| !matches!(s.installer, Installer::Manual(_)))?;
    let (_, candidates) = crate::lsp::spec_for(extension)?;
    let any_ready = candidates.iter().any(|(name, _)| crate::lsp::usable(name));
    (!any_ready).then_some(info)
}

/// Whether the program is in the folder SimpleEdit installs into.
fn is_managed(binary: &str) -> bool {
    managed_bin_dir().is_some_and(|dir| {
        ["", ".exe", ".cmd", ".bat"]
            .iter()
            .any(|suffix| dir.join(format!("{binary}{suffix}")).is_file())
    })
}

/// The ready servers that SimpleEdit can remove: the ones it installed (and
/// Rust's, which rustup manages). A server found elsewhere is not touched.
pub fn removable() -> HashSet<&'static str> {
    ready()
        .into_iter()
        .filter(|b| is_managed(b) || *b == "rust-analyzer")
        .collect()
}

const INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Installs a server; the error is what the tool said.
pub async fn install(binary: &'static str) -> Result<(), String> {
    let info = SERVERS
        .iter()
        .find(|s| s.binary == binary)
        .ok_or_else(|| format!("unknown server {binary}"))?;
    let bin = managed_bin_dir().ok_or("no data folder")?;
    let root = managed_dir().ok_or("no data folder")?;
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;

    let mut command = match info.installer {
        Installer::Npm(packages) => {
            let mut c = tool("npm")?;
            c.args(["install", "-g", "--ignore-scripts", "--prefix"])
                .arg(&root)
                .args(packages);
            c
        }
        Installer::Rustup(component) => {
            let mut c = tool("rustup")?;
            c.args(["component", "add", component]);
            c
        }
        Installer::Go(package) => {
            let mut c = tool("go")?;
            c.args(["install", package]).env("GOBIN", &bin);
            c
        }
        Installer::Dotnet(name) => {
            let mut c = tool("dotnet")?;
            c.args(["tool", "install", "--tool-path"])
                .arg(&bin)
                .arg(name);
            c
        }
        Installer::Manual(hint) => return Err(hint.to_string()),
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let run = command.output();
    let output = tokio::time::timeout(INSTALL_TIMEOUT, run)
        .await
        .map_err(|_| "timeout".to_string())?
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(tail(&String::from_utf8_lossy(&output.stderr)));
    }
    // The tool can succeed without leaving a program that runs.
    crate::lsp::forget_usable();
    let usable = tokio::task::spawn_blocking(move || crate::lsp::usable(binary))
        .await
        .unwrap_or(false);
    if usable {
        Ok(())
    } else {
        Err(format!("{binary} not found after the installation"))
    }
}

/// Removes a server installed with [`install`]; the error is what the tool said.
pub async fn uninstall(binary: &'static str) -> Result<(), String> {
    let info = SERVERS
        .iter()
        .find(|s| s.binary == binary)
        .ok_or_else(|| format!("unknown server {binary}"))?;
    let bin = managed_bin_dir().ok_or("no data folder")?;
    let root = managed_dir().ok_or("no data folder")?;
    let mut command = match info.installer {
        Installer::Npm(packages) => {
            let mut c = tool("npm")?;
            c.args(["uninstall", "-g", "--prefix"]).arg(&root);
            // `typescript@5` is installed as `typescript`.
            for package in packages {
                c.arg(package_name(package));
            }
            c
        }
        Installer::Rustup(component) => {
            let mut c = tool("rustup")?;
            c.args(["component", "remove", component]);
            c
        }
        Installer::Dotnet(name) => {
            let mut c = tool("dotnet")?;
            c.args(["tool", "uninstall", "--tool-path"])
                .arg(&bin)
                .arg(name);
            c
        }
        // `go install` leaves a single program.
        Installer::Go(_) => {
            return ["", ".exe"]
                .iter()
                .map(|suffix| bin.join(format!("{binary}{suffix}")))
                .filter(|path| path.is_file())
                .try_for_each(std::fs::remove_file)
                .map_err(|e| e.to_string());
        }
        Installer::Manual(hint) => return Err(hint.to_string()),
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(INSTALL_TIMEOUT, command.output())
        .await
        .map_err(|_| "timeout".to_string())?
        .map_err(|e| e.to_string())?;
    crate::lsp::forget_usable();
    if output.status.success() {
        Ok(())
    } else {
        Err(tail(&String::from_utf8_lossy(&output.stderr)))
    }
}

/// `typescript@5` is installed as `typescript` (a scope's `@` stays).
fn package_name(spec: &str) -> &str {
    match spec.get(1..).and_then(|rest| rest.find('@')) {
        Some(at) => &spec[..at + 1],
        None => spec,
    }
}

/// A command for an installer tool, or why it can't run.
fn tool(name: &str) -> Result<Command, String> {
    let path = crate::lsp::find_executable(name)
        .ok_or_else(|| format!("{name} not found: install it first"))?;
    Ok(Command::new(path))
}

/// The end of a tool's complaints: that is where the reason is.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(3);
    let joined = lines[start..].join(" · ");
    let chars: Vec<char> = joined.chars().collect();
    if chars.len() > 240 {
        format!("…{}", chars[chars.len() - 240..].iter().collect::<String>())
    } else {
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_extension_of_the_lsp_table_has_an_installable_entry() {
        // The table of programs per extension (lsp) and this list agree.
        for info in SERVERS {
            for ext in info.extensions {
                let (_, candidates) = crate::lsp::spec_for(ext)
                    .unwrap_or_else(|| panic!("{ext} has no server in lsp"));
                assert!(
                    candidates.iter().any(|c| c.0 == info.binary)
                        || matches!(info.installer, Installer::Manual(_)),
                    "{ext}: {} not among its servers",
                    info.binary
                );
            }
        }
    }

    #[test]
    fn a_tools_complaint_is_cut_to_its_end() {
        assert_eq!(tail("a\n\nb\nc\nd\n"), "b · c · d");
        assert!(tail(&"x".repeat(1000)).chars().count() <= 241);
    }

    #[test]
    fn manual_servers_cannot_be_installed_here() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let error = runtime.block_on(install("clangd")).unwrap_err();
        assert!(error.contains("clangd"));
    }

    #[test]
    fn a_version_is_dropped_from_the_package_name_to_remove() {
        assert_eq!(package_name("typescript@5"), "typescript");
        assert_eq!(package_name("pyright"), "pyright");
        assert_eq!(package_name("@scope/tool@2"), "@scope/tool");
        assert_eq!(package_name("@scope/tool"), "@scope/tool");
    }

    /// Installs then removes a server: `cargo test real_uninstall -- --ignored`.
    #[test]
    #[ignore]
    fn real_uninstall_removes_what_install_added() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(install("bash-language-server")).unwrap();
        assert!(removable().contains("bash-language-server"));
        runtime.block_on(uninstall("bash-language-server")).unwrap();
        assert!(!ready().contains("bash-language-server"));
    }

    #[test]
    fn only_installable_languages_without_a_server_are_offered() {
        // Plain text and the manual-only servers are never offered.
        assert!(offer_for("txt").is_none());
        assert!(offer_for("zig").is_none());
        // Every offered server is one the Preferences can install.
        for info in SERVERS {
            if let Some(offered) = info.extensions.first().and_then(|e| offer_for(e)) {
                assert!(!matches!(offered.installer, Installer::Manual(_)));
            }
        }
    }

    /// A project without `node_modules/typescript` must use the copy installed
    /// with the server: `cargo test real_typescript -- --ignored`.
    #[test]
    #[ignore]
    fn real_typescript_without_local_typescript() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(install("typescript-language-server"))
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        let file = dir.path().join("a.tsx");
        let text = "const Card = () => <div/>;\nexport const App = () => <Card/>;\n";
        std::fs::write(&file, text).unwrap();
        let ask = || runtime.block_on(crate::lsp::definition("tsx", &file, text, 1, 30));
        let _ = ask();
        let outcome = ask();
        assert!(
            matches!(outcome, crate::lsp::Outcome::Found(ref h) if h.line == 0 && h.col == 6),
            "{outcome:?}"
        );
    }

    /// Really installs and uses a server: `cargo test real_install -- --ignored`.
    #[test]
    #[ignore]
    fn real_install_then_definition() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(install("bash-language-server")).unwrap();
        assert!(ready().contains("bash-language-server"));
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.sh");
        let text = "greet() {\n  echo hi\n}\ngreet\n";
        std::fs::write(&file, text).unwrap();
        let ask = || runtime.block_on(crate::lsp::definition("sh", &file, text, 3, 2));
        // A fresh bash server answers "nothing" while it scans the folder;
        // the app waits for that, this test (with a short wait) asks again.
        let _ = ask();
        std::thread::sleep(std::time::Duration::from_secs(3));
        let outcome = ask();
        assert_eq!(
            outcome,
            crate::lsp::Outcome::Found(crate::definition::Hit {
                path: file,
                line: 0,
                col: 0,
                // This server gives the whole function as the range.
                len: 0
            })
        );
    }
}
