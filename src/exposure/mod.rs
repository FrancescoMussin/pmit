use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::polymarket::Trade;

// The model we use for computing exposure scores is in an asynchronous python worker process, so we
// need to define the communication protocol and the Rust-side engine to manage it using
// tokio::process.

/// Trade enriched with a continuous exposure score in [0.0, 1.0].
#[derive(Debug, Clone)]
pub struct ScoredTrade {
    pub trade: Trade,
    pub exposure_score: f64,
}

/// Struct representing the input to the exposure scoring worker for a single trade.
#[derive(Debug, Serialize)]
struct WorkerTradeInput {
    title: String,
    outcome: String,
}

/// Struct representing the request payload sent to the exposure worker for scoring a batch of
/// trades.
#[derive(Debug, Serialize)]
struct WorkerScoreRequest {
    trades: Vec<WorkerTradeInput>,
}
/// Struct representing the response from the exposure worker containing the computed scores.
#[derive(Debug, Deserialize)]
struct WorkerScoreResponse {
    scores: Vec<f64>,
}

/// Struct representing the Python exposure scoring worker process and its communication channels.
#[derive(Debug, Deserialize)]
struct WorkerReadyResponse {
    ready: bool,
}
/// Struct managing the lifecycle and communication with the Python exposure scoring worker process.
/// It provides methods to send batches of trades for scoring and receive the computed exposure
/// scores. The worker is expected to signal its readiness by sending a JSON message with `{"ready":
/// true}` on stdout when it is ready to receive requests.
pub struct ExposureEngine {
    /// Handle for the worker process
    worker_child: Child,
    /// Stdin handle for sending requests to the worker
    worker_stdin: ChildStdin,
    /// Stdout handle for reading responses from the worker, the BufReader makes reading more
    /// efficient by reading line by line.
    worker_stdout: BufReader<ChildStdout>,
}

impl ExposureEngine {
    /// Asynchronously initializes the exposure engine by starting the Python worker process and
    /// awaiting its readiness signal. The `exposure_temperature` parameter is passed to the
    /// worker as an environment variable to configure the softmax temperature used in scoring.
    /// The method returns an instance of `ExposureEngine` with the communication channels set
    /// up and ready for scoring requests. If the worker fails to start or does not signal
    /// readiness within a reasonable time, an error is returned. The worker process is expected
    /// to be defined in `python_modules/exposure_worker.py` and should be executable with
    /// either `python3` or a virtual environment Python at `.venv/bin/python`. The worker
    /// should print a JSON line with `{"ready": true}` to stdout when it is ready to receive
    /// scoring requests.
    pub async fn new(exposure_temperature: f64) -> Result<Self> {
        // we check if a python virtual environment exists at .venv/bin/python, otherwise we fall
        // back to python3 in PATH
        let python_bin = if Path::new(".venv/bin/python").exists() {
            ".venv/bin/python"
        } else {
            "python3"
        };

        // Start the Python worker process with correct .arg() and .env() calls (in the latter of
        // which we pass the exposure temperature), with piped stdin and stdout for communication
        // (which requires mutable access on the child process handle), and inherit stderr
        // so we can see any errors it prints directly in our console.
        let mut child = Command::new(python_bin)
            .arg("python_modules/exposure_worker.py")
            .env("EXPOSURE_TEMPERATURE", format!("{}", exposure_temperature))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .context("Failed to start Python exposure worker")?;

        // Capture the stdin and stdout handles for communication with the worker, and wrap stdout
        // in a BufReader for easier line-by-line reading. We will read lines from the worker's
        // stdout until we see the readiness signal or time out after a reasonable number of
        // attempts.
        let worker_stdin = child
            .stdin
            .take()
            .context("Failed to capture exposure worker stdin")?;
        let worker_stdout = child
            .stdout
            .take()
            .context("Failed to capture exposure worker stdout")?;
        let mut worker_stdout = BufReader::new(worker_stdout);

        // to read from the stdoutput we need a mutable string buffer to read lines into, and we
        // read line by line until we see the readiness signal or time out after 10 attempts.
        // If we read an empty line, we just ignore it and continue waiting. If we
        // read a non-empty line, we try to parse it as JSON into our
        // WorkerReadyResponse struct, and if it has ready=true, we break the loop and proceed.
        // If we exhaust all attempts without seeing the readiness signal, we return an error.
        let mut saw_ready = false;
        for _ in 0..10 {
            let mut line = String::new();
            // we read the worker's stdout line and append it to the line buffer
            worker_stdout
                .read_line(&mut line)
                .await
                .context("Failed to read readiness signal from exposure worker")?;

            // check if the line is empty (remove \n's and whitspaces)
            if line.trim().is_empty() {
                continue;
            }

            // if the line is not empty, we try to parse it as JSON into our WorkerReadyResponse
            // struct, and if it has ready=true, we set saw_ready to true and break the loop
            if let Ok(ready) = serde_json::from_str::<WorkerReadyResponse>(line.trim()) {
                if ready.ready {
                    saw_ready = true;
                    break;
                }
            }
        }

        // if we exhausted all attempts without seeing the readiness signal, we return an error
        if !saw_ready {
            return Err(anyhow::anyhow!(
                "Exposure worker did not signal readiness after multiple attempts"
            ));
        }

        Ok(Self {
            worker_child: child,
            worker_stdin,
            worker_stdout,
        })
    }

    /// Score a batch of trades using the sentence-BERT Python worker.
    pub async fn score_batch(&mut self, trades: Vec<Trade>) -> Result<Vec<ScoredTrade>> {
        // If there are no trades to score, we can skip the worker roundtrip and just return an
        // empty vector. This also avoids potential issues with the worker if it doesn't handle
        // empty input gracefully.
        if trades.is_empty() {
            return Ok(Vec::new());
        }

        // Construct the request payload by mapping our Trade structs into the WorkerTradeInput
        // format expected by the worker. We take the title and outcome from each trade,
        // using default values if they are missing, and collect them into a vector.
        let request = WorkerScoreRequest {
            trades: trades
                .iter()
                .map(|trade| WorkerTradeInput {
                    title: trade.title.clone().unwrap_or_default(),
                    outcome: trade.outcome.clone().unwrap_or_default(),
                })
                .collect(),
        };

        // Serialize the request to JSON and send it to the worker's stdin
        let payload = serde_json::to_string(&request)
            .context("Failed to serialize exposure scoring request")?;

        // write the JSON payload to the worker's stdin, followed by a newline.
        let mut msg = payload;
        msg.push('\n');
        self.worker_stdin
            .write_all(msg.as_bytes())
            .await
            .context("Failed to write request to exposure worker")?;
        // since many i/o streams are buffered, we need to flush after writing to ensure the worker
        // receives the data immediately, rather than waiting for the buffer to fill up or the
        // process to end. If we don't flush, the worker might not receive the request in a timely
        // manner, leading to delays or timeouts in scoring. Flushing forces the buffer to be sent
        // to the worker right away.
        self.worker_stdin
            .flush()
            .await
            .context("Failed to flush exposure worker stdin")?;
        // we need a mutable string buffer to read the worker's response line into
        let mut response_line = String::new();
        // read a line from the worker's stdout.
        self.worker_stdout
            .read_line(&mut response_line)
            .await
            .context("Failed to read response from exposure worker")?;

        // check if the line is empty
        if response_line.trim().is_empty() {
            anyhow::bail!("Exposure worker returned an empty response");
        }

        // if the line is not empty, we try to parse it as JSON into our WorkerScoreResponse struct.
        let response: WorkerScoreResponse = serde_json::from_str(response_line.trim())
            .context("Failed to parse exposure worker response JSON")?;

        // we do a simple sanity check to make sure the number of scores matches the number of
        // trades we sent, and if not, we return an error. This helps catch any issues with the
        // worker's response format or logic.
        if response.scores.len() != trades.len() {
            anyhow::bail!(
                "Exposure worker score count mismatch: got {}, expected {}",
                response.scores.len(),
                trades.len()
            );
        }

        // Finally, we zip together the original trades and the received scores, clamp the scores
        // to the [0.0, 1.0] range for safety, and construct our final vector of ScoredTrade structs
        // to return.
        let scored = trades
            .into_iter()
            .zip(response.scores.into_iter())
            .map(|(trade, score)| ScoredTrade {
                trade,
                exposure_score: score.clamp(0.0, 1.0),
            })
            .collect();

        Ok(scored)
    }
}

// Custom Drop is removed since kill_on_drop(true) securely manages the child's lifetime.
/// Split scored trades into relevant/non-relevant buckets for routing.
pub fn split_by_threshold(
    scored_trades: Vec<ScoredTrade>,
    threshold: f64,
) -> (Vec<ScoredTrade>, Vec<ScoredTrade>) {
    let mut relevant = Vec::new();
    let mut deferred = Vec::new();

    for scored in scored_trades {
        if scored.exposure_score >= threshold {
            relevant.push(scored);
        } else {
            deferred.push(scored);
        }
    }

    (relevant, deferred)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_structures::{ContractId, Side, Trade, WalletAddress};

    fn mock_trade(hash: &str) -> Trade {
        Trade {
            maker_address: WalletAddress::new(
                "0xbf8d2192f319af779e87899b847017096bc7c93a".to_string(),
            ),
            side: Side::Buy,
            asset: ContractId::new(
                "32516959846448419995643742836043735992939289847692314265462730646982809895410"
                    .to_string(),
            ),
            title: None,
            outcome: None,
            size: 100.0,
            price: 1.0,
            timestamp: 1000,
            transaction_hash: hash.to_string(),
        }
    }

    #[test]
    fn test_split_by_threshold() {
        let trades = vec![
            ScoredTrade {
                trade: mock_trade("0x1"),
                exposure_score: 0.8,
            },
            ScoredTrade {
                trade: mock_trade("0x2"),
                exposure_score: 0.4,
            },
            ScoredTrade {
                trade: mock_trade("0x3"),
                exposure_score: 0.6,
            },
        ];

        let (relevant, deferred) = split_by_threshold(trades, 0.5);

        assert_eq!(relevant.len(), 2);
        assert_eq!(deferred.len(), 1);
        assert_eq!(relevant[0].trade.transaction_hash, "0x1");
        assert_eq!(relevant[1].trade.transaction_hash, "0x3");
        assert_eq!(deferred[0].trade.transaction_hash, "0x2");
    }

    #[test]
    fn test_split_by_threshold_exact() {
        let trades = vec![ScoredTrade {
            trade: mock_trade("0x1"),
            exposure_score: 0.5,
        }];

        let (relevant, deferred) = split_by_threshold(trades, 0.5);
        assert_eq!(relevant.len(), 1);
        assert_eq!(deferred.len(), 0);
    }

    #[test]
    fn test_scored_trade_clamping_logic() {
        // We test that the score_batch logic (which we'll simulate here) correctly clamps values.
        // Since we can't easily mock the whole score_batch without a worker, we test the
        // closure/logic used in it.
        let raw_scores = vec![-0.5, 0.5, 1.5];
        let clamped: Vec<f64> = raw_scores
            .into_iter()
            .map(|s: f64| s.clamp(0.0, 1.0))
            .collect();

        assert_eq!(clamped, vec![0.0, 0.5, 1.0]);
    }
}
