//! The load a stage measurement runs under: clients that send translation
//! requests to a llama-server without a pause, so that the GPU is shared with
//! a second process while the stages are timed.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::error::EvalError;
use crate::translate_call::{ChatClient, translate_body};

/// Clients that keep a llama-server busy until they are stopped.
pub struct Load {
    stop: Arc<AtomicBool>,
    done: Arc<AtomicU64>,
    failed: Arc<AtomicU64>,
    threads: Vec<JoinHandle<()>>,
}

/// How many requests the clients finished and how many failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadCount {
    pub done: u64,
    pub failed: u64,
}

impl Load {
    /// Starts `clients` threads that cycle through `sentences` and send each as a translation request.
    pub fn start(url: &str, sentences: &[String], clients: u32) -> Result<Load, EvalError> {
        if sentences.is_empty() {
            return Err(EvalError::Invalid(String::from(
                "the load has no sentences to send",
            )));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicU64::new(0));
        let mut threads = Vec::new();
        for client in 0..clients {
            let bodies: Vec<String> = sentences
                .iter()
                .cycle()
                .skip(client as usize)
                .take(sentences.len())
                .map(|text| translate_body(text))
                .collect::<Result<_, _>>()?;
            let chat = ChatClient::new(url, Duration::from_secs(60));
            let (stop, done, failed) = (Arc::clone(&stop), Arc::clone(&done), Arc::clone(&failed));
            threads.push(std::thread::spawn(move || {
                for body in bodies.iter().cycle() {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    if chat.complete(body).is_ok() {
                        done.fetch_add(1, Ordering::Relaxed);
                    } else {
                        failed.fetch_add(1, Ordering::Relaxed);
                        // A server that is down must not turn the client into a busy loop.
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }));
        }
        Ok(Load {
            stop,
            done,
            failed,
            threads,
        })
    }

    /// Stops the clients, waits for their last request and returns what they did.
    pub fn stop(self) -> Result<LoadCount, EvalError> {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads {
            thread
                .join()
                .map_err(|_| EvalError::Invalid(String::from("a load client panicked")))?;
        }
        Ok(LoadCount {
            done: self.done.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_load_without_sentences_is_an_error() {
        assert!(matches!(
            Load::start("http://127.0.0.1:1", &[], 2),
            Err(EvalError::Invalid(_))
        ));
    }

    #[test]
    fn clients_that_cannot_connect_count_failures_and_stop() -> Result<(), EvalError> {
        let sentences = vec![String::from("あ")];
        let load = Load::start("http://127.0.0.1:1", &sentences, 2)?;
        std::thread::sleep(Duration::from_millis(50));
        let count = load.stop()?;
        assert_eq!(count.done, 0);
        assert!(count.failed > 0);
        Ok(())
    }
}
