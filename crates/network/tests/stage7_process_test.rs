use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Result};
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

struct Worker {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    address: String,
}

impl Worker {
    async fn send(&mut self, command: &str) -> Result<()> {
        self.stdin.write_all(command.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn shutdown(mut self) -> Result<()> {
        let _ = self.send("EXIT").await;
        let _ = timeout(Duration::from_secs(5), self.child.wait()).await??;
        Ok(())
    }
}

async fn next_line(worker: &mut Worker) -> Result<String> {
    timeout(Duration::from_secs(10), worker.lines.next_line())
        .await
        .map_err(|_| anyhow!("worker output timed out"))??
        .ok_or_else(|| anyhow!("worker exited before producing output"))
}

async fn wait_for_line(worker: &mut Worker, expected: &str) -> Result<String> {
    loop {
        let line = next_line(worker).await?;
        if line == expected || line.contains(expected) {
            return Ok(line);
        }
    }
}

async fn assert_no_line_containing(worker: &mut Worker, forbidden: &str) -> Result<()> {
    let result = timeout(Duration::from_secs(3), async {
        loop {
            let Some(line) = worker.lines.next_line().await? else {
                return Ok::<bool, std::io::Error>(false);
            };
            if line.contains(forbidden) {
                return Ok(true);
            }
        }
    })
    .await;
    match result {
        Err(_) => Ok(()),
        Ok(Ok(true)) => Err(anyhow!("worker emitted forbidden output {forbidden:?}")),
        Ok(Ok(false)) => Err(anyhow!("worker exited while checking forbidden output")),
        Ok(Err(error)) => Err(error.into()),
    }
}

fn peer_id_from_address(address: &str) -> PeerId {
    address
        .parse::<Multiaddr>()
        .expect("worker listen address is a valid multiaddr")
        .iter()
        .find_map(|protocol| match protocol {
            Protocol::P2p(peer_id) => Some(peer_id),
            _ => None,
        })
        .expect("worker listen address includes a PeerId")
}

async fn spawn_worker(role: &str, genesis_byte: u8, bootstrap: &[String]) -> Result<Worker> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stage7_worker"));
    command
        .arg(role)
        .arg(genesis_byte.to_string())
        .args(bootstrap)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("worker stdin unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("worker stdout unavailable"))?;
    let mut worker = Worker {
        child,
        stdin,
        lines: BufReader::new(stdout).lines(),
        address: String::new(),
    };
    worker.address = wait_for_line(&mut worker, "LISTEN ").await?;
    worker.address = worker
        .address
        .strip_prefix("LISTEN ")
        .ok_or_else(|| anyhow!("invalid worker listen output"))?
        .to_string();
    Ok(worker)
}

#[tokio::test]
async fn stage7_separate_processes_exchange_only_authenticated_validator_consensus() -> Result<()> {
    let mut validator = spawn_worker("validator", 1, &[]).await?;
    let validator_address = validator.address.clone();

    let mut peer = spawn_worker("validator", 1, &[validator_address.clone()]).await?;
    let peer_peer_id = peer_id_from_address(&peer.address);
    let mut full_node = spawn_worker("fullnode", 1, &[validator_address.clone()]).await?;
    let full_node_peer_id = peer_id_from_address(&full_node.address);

    wait_for_line(&mut validator, &format!("AUTH {peer_peer_id} Validator")).await?;
    wait_for_line(
        &mut validator,
        &format!("AUTH {full_node_peer_id} FullNode"),
    )
    .await?;

    peer.send("BROADCAST 1").await?;
    wait_for_line(&mut validator, "CONSENSUS").await?;

    full_node.send("BROADCAST 3").await?;
    assert_no_line_containing(&mut validator, "CONSENSUS").await?;

    let wrong_context = spawn_worker("validator", 2, &[validator_address]).await?;
    let wrong_context_peer_id = peer_id_from_address(&wrong_context.address);
    assert_no_line_containing(&mut validator, &format!("AUTH {wrong_context_peer_id}")).await?;

    wrong_context.shutdown().await?;
    full_node.shutdown().await?;
    peer.shutdown().await?;
    validator.shutdown().await?;
    Ok(())
}
