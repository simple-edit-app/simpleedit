//! `ssh -L` for the drivers that reach a server over the network: started
//! for the length of the session, stopped with it. (The PostgreSQL driver
//! has the same, in its own file.)

use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::protocol::{SshAuth, Target};

/// `ssh -L`, started for the length of the operation.
pub struct Tunnel {
    child: Child,
    pub port: u16,
    askpass: Option<PathBuf>,
}

impl Tunnel {
    pub fn open(target: &Target) -> Result<Tunnel, String> {
        let ssh = &target.ssh;
        if ssh.host.trim().is_empty() {
            return Err("SSH: no host".to_string());
        }
        let local = match ssh.local_port.trim() {
            "" => free_port()?,
            port => {
                let port = port
                    .parse::<u16>()
                    .map_err(|_| format!("SSH: bad local port {port}"))?;
                // Whatever answers on that port must be our tunnel, never a
                // service that was already there.
                TcpListener::bind(("127.0.0.1", port))
                    .map_err(|e| format!("SSH: the local port {port} is in use: {e}"))?;
                port
            }
        };
        let destination = if ssh.user.trim().is_empty() {
            ssh.host.trim().to_string()
        } else {
            format!("{}@{}", ssh.user.trim(), ssh.host.trim())
        };
        let mut command = Command::new("ssh");
        command
            .arg("-N")
            .args([
                "-L",
                &format!("127.0.0.1:{local}:{}:{}", target.host, target.port),
            ])
            .args(["-p", &ssh.port.to_string()])
            .args(["-o", "ExitOnForwardFailure=yes"])
            .args(["-o", "ConnectTimeout=10"])
            .args(["-o", "StrictHostKeyChecking=accept-new"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        die_with_parent(&mut command);
        let mut askpass = None;
        match ssh.auth {
            SshAuth::Agent => {
                command.args(["-o", "BatchMode=yes"]);
            }
            SshAuth::KeyPair => {
                if !ssh.key_file.trim().is_empty() {
                    command.args(["-i", ssh.key_file.trim(), "-o", "IdentitiesOnly=yes"]);
                }
                askpass = Some(secret_script(&mut command, &target.ssh_secret)?);
            }
            SshAuth::Password => {
                command.args([
                    "-o",
                    "PreferredAuthentications=password,keyboard-interactive",
                    "-o",
                    "PubkeyAuthentication=no",
                ]);
                askpass = Some(secret_script(&mut command, &target.ssh_secret)?);
            }
        }
        command.arg(&destination);
        let child = command
            .spawn()
            .map_err(|e| format!("SSH: cannot run ssh: {e}"))?;
        let mut tunnel = Tunnel {
            child,
            port: local,
            askpass,
        };
        // Wait until the local end accepts connections.
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = tunnel.child.try_wait().map_err(|e| e.to_string())? {
                let mut said = String::new();
                if let Some(mut err) = tunnel.child.stderr.take() {
                    let _ = err.read_to_string(&mut said);
                }
                let said = said.trim().lines().last().unwrap_or("").to_string();
                return Err(format!("SSH: {status}: {said}"));
            }
            if TcpStream::connect_timeout(
                &([127, 0, 0, 1], local).into(),
                Duration::from_millis(200),
            )
            .is_ok()
            {
                // Only ssh can be behind that port; if it is gone, it was
                // not ssh that answered.
                if tunnel
                    .child
                    .try_wait()
                    .map_err(|e| e.to_string())?
                    .is_some()
                {
                    return Err("SSH: the tunnel stopped as it came up".to_string());
                }
                // Nobody reads stderr from here on: ssh would block once the
                // pipe is full (about 64 KB of messages).
                if let Some(err) = tunnel.child.stderr.take() {
                    drain(err);
                }
                return Ok(tunnel);
            }
            if Instant::now() > deadline {
                return Err("SSH: the tunnel did not come up in time".to_string());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Reads and drops what `reader` yields, on a thread, until it ends.
fn drain<R: Read + Send + 'static>(mut reader: R) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut reader, &mut std::io::sink());
    })
}

/// A script `ssh` runs to ask for the password: it prints the secret, which
/// reaches it through the environment (never the command line).
fn secret_script(command: &mut Command, secret: &str) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!("simpleedit-askpass-{}", std::process::id()));
    std::fs::write(
        &path,
        "#!/bin/sh\nprintf '%s\\n' \"$SIMPLEEDIT_SSH_SECRET\"\n",
    )
    .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    command
        .env("SSH_ASKPASS", &path)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("SIMPLEEDIT_SSH_SECRET", secret)
        .env(
            "DISPLAY",
            std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into()),
        );
    Ok(path)
}

/// `ssh` must not outlive the driver, even if the driver is killed: it is
/// told to die with its parent (Linux).
fn die_with_parent(command: &mut Command) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: only `prctl`, which is async-signal-safe, runs between the
        // fork and the exec.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = command;
}

fn free_port() -> Result<u16, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    listener
        .local_addr()
        .map(|a| a.port())
        .map_err(|e| e.to_string())
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(path) = &self.askpass {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_reads_a_pipe_to_its_end() {
        let mut child = Command::new("sh")
            .args(["-c", "head -c 300000 /dev/zero >&2"])
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let handle = drain(child.stderr.take().unwrap());
        // Without the drain, the child would block on a full pipe.
        assert!(child.wait().unwrap().success());
        handle.join().unwrap();
    }

    #[test]
    fn a_local_port_already_in_use_is_refused_before_ssh_is_run() {
        let taken = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut target = Target::default();
        target.ssh.host = "unused.invalid".into();
        target.ssh.local_port = taken.local_addr().unwrap().port().to_string();
        let error = Tunnel::open(&target).err().expect("refused");
        assert!(error.contains("in use"), "{error}");
    }
}
