use std::ops::Add;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use strum_macros::Display;
use tokio::sync::mpsc::Receiver;
use tokio::time::{Instant, timeout_at};
use tracing::{error, info};

use crate::innodb::innodb0types::Lsn;
use crate::log::log0flusher_loop::LogFlushNotification::PoisonPillReceived;
use crate::log::log0log::Log;

/// A trait for receiving notifications about log flush progress or errors.
///
/// Types implementing this trait can be used to observe or react to events
/// emitted by the log flusher loop, such as successful flushes up to a
/// particular LSN or unexpected errors during flushing.
///
/// This trait is required to be `Send + Sync` so that implementors can be
/// safely shared across threads (e.g., wrapped in an `Arc<Mutex<dyn LogFlushNotifier>>`).
///
pub trait LogFlushNotifier: Send + Sync {
    fn notify(&mut self, notification: LogFlushNotification);
}

/// Types of flush notification.
pub enum LogFlushNotification {
    /// Flusher has completed writing upto this lsn.
    SuccessfullyWrittenUpto(Lsn),
    PoisonPillReceived,
    /// Error string.
    Err(String),
}

/// Trigger for a log flush.
/// The sender can send messages that will be polled by the flusher to flush logs.
#[derive(Display, Debug)]
pub enum LogFlushTrigger {
    /// Lsn to write up to.
    WriteUpTo(Lsn),
    /// PoisonPill to stop the loop.
    PoisonPill,
}

pub struct LogFlusherLoop {
    /// A reference to the log system.
    log_sys: Arc<Mutex<Log>>,
    /// Receiver channel for receiving trigger messages.
    receiver: Receiver<LogFlushTrigger>,
    /// Max duration to wait for messages from the receiver channel.
    max_delay: Duration,
    /// Max batch size of messages to wait before flushing.
    max_batch_size: usize,
    /// Flag to stop the loop.
    loop_stop: AtomicBool,
    /// Notifier to send events.
    log_flush_loop_notifier: Arc<Mutex<dyn LogFlushNotifier>>,
}

impl LogFlusherLoop {
    /// Create
    pub fn new(
        log_sys: Arc<Mutex<Log>>,
        receiver: Receiver<LogFlushTrigger>,
        log_flush_loop_notifier: Arc<Mutex<dyn LogFlushNotifier>>,
    ) -> LogFlusherLoop {
        Self {
            log_sys,
            receiver,
            max_delay: Duration::from_millis(100),
            max_batch_size: 100,
            loop_stop: AtomicBool::from(false),
            log_flush_loop_notifier,
        }
    }

    /// Async loop that keeps of pull trigger events from the queue,
    /// and triggers a flush event based on the max lsn received.
    /// If a poison pill is received, we complete flushing of the current max lsn, and then stop the loop.
    pub async fn start_loop(&mut self) {
        let mut counter = 0u64;
        loop {
            counter += 1;
            info!("in the loop, counter={}", counter);
            if self.loop_stop.load(Ordering::Relaxed) {
                break;
            }

            if self.receiver.is_closed() {
                info!("the channel is closed");
                self.loop_stop.store(true, Ordering::Relaxed);
            } else {
                info!("the channel is active");
            }

            let mut batch = Vec::<LogFlushTrigger>::new();
            let deadline = Instant::now().add(self.max_delay);

            while deadline >= Instant::now()
                && batch.len() < self.max_batch_size
            {
                info!("waiting for message");
                match timeout_at(deadline, self.receiver.recv()).await {
                    Ok(Some(msg)) => {
                        info!("received message: {}", msg);
                        batch.push(msg);
                    }
                    Ok(None) => {}
                    Err(err) => {
                        error!(
                            "encountered error receiving log message: {}",
                            err
                        );

                        self.log_flush_loop_notifier.lock().unwrap().notify(
                            LogFlushNotification::Err(err.to_string()),
                        );
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            // We have collected enough or exceeded the deadline. Not trigger the lsn flush.
            let max_lsn = batch
                .iter()
                .map(|x| match x {
                    LogFlushTrigger::WriteUpTo(lsn) => *lsn,
                    LogFlushTrigger::PoisonPill => {
                        info!("received poison pill");
                        self.log_flush_loop_notifier
                            .lock()
                            .unwrap()
                            .notify(PoisonPillReceived);
                        self.loop_stop.store(true, Ordering::Relaxed);
                        return Lsn::MIN;
                    }
                })
                .max();

            if let Some(lsn) = max_lsn {
                if lsn != Lsn::MIN {
                    info!("writing logs upto lsn={}", lsn);
                    // self.log_sys.lock().unwrap().write_up_to(
                    //     lsn,
                    //     LogWaitMode::LogNoWait,
                    //     true,
                    // );

                    self.log_flush_loop_notifier.lock().unwrap().notify(
                        LogFlushNotification::SuccessfullyWrittenUpto(lsn),
                    );
                } else {
                    info!("writing logs upto lsn={}", lsn);
                }
            } else {
                info!("nothing to write :(")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::max;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::sync::mpsc;

    use crate::fil::fil0fil::Fil;
    use crate::innodb::innodb0types::{Lsn, UNIV_PAGE_SIZE};
    use crate::log::log0flusher_loop::{
        LogFlushNotification, LogFlushNotifier, LogFlushTrigger,
        LogFlusherLoop,
    };
    use crate::log::log0log::Log;
    use crate::os::os0io::OsIo;
    use crate::os::os0runtime::create_runtime;
    use crate::ut::ut0ut::GLOBAL_RUNTIME;

    struct MockLogFlushNotifier {
        err: usize,
        last_written_lsn: Lsn,
        poison_pill: bool,
    }

    impl MockLogFlushNotifier {
        pub fn new() -> Self {
            Self { err: 0, last_written_lsn: 0, poison_pill: false }
        }
    }

    impl LogFlushNotifier for MockLogFlushNotifier {
        fn notify(&mut self, notification: LogFlushNotification) {
            match notification {
                LogFlushNotification::SuccessfullyWrittenUpto(lsn) => {
                    self.last_written_lsn = max(self.last_written_lsn, lsn);
                }
                LogFlushNotification::PoisonPillReceived => {
                    self.poison_pill = true;
                }
                LogFlushNotification::Err(_) => {
                    self.err += 1;
                }
            }
        }
    }

    #[test]
    fn test_log_flusher_loop() {
        let runtime = create_runtime(1);
        let os_io = Arc::new(Mutex::new(OsIo::new(runtime.clone())));
        let fil = Arc::new(Mutex::new(Fil::new(100, os_io, runtime.clone())));
        // Total log size of 16 MB.
        let log_sys =
            Arc::new(Mutex::new(Log::new(UNIV_PAGE_SIZE * 100, fil)));
        let (tx, rx) = mpsc::channel(100);
        let notifier = Arc::new(Mutex::new(MockLogFlushNotifier::new()));

        let log_sys_clone = log_sys.clone();
        let notifier_clone = notifier.clone();
        let join_handle = GLOBAL_RUNTIME.spawn(async move {
            let mut flusher_loop =
                LogFlusherLoop::new(log_sys_clone, rx, notifier_clone);
            flusher_loop.start_loop().await
        });

        assert_eq!(log_sys.lock().unwrap().get_lsn(), 0);
        let data = b"hello world!".to_vec();
        let result = {
            log_sys.lock().unwrap().reserve_and_write_fast(data.as_slice())
        };
        assert!(result.is_ok());
        let write_result = result.unwrap();
        assert_eq!(write_result.start_lsn, 0);
        assert_eq!(write_result.end_lsn, data.len() as Lsn);

        // Send the message to flush
        tx.blocking_send(LogFlushTrigger::WriteUpTo(data.len() as Lsn))
            .unwrap();

        // By this time, the write-operation should be complete.
        std::thread::sleep(Duration::from_millis(200));

        // send the poison pill.
        tx.blocking_send(LogFlushTrigger::PoisonPill).unwrap();

        runtime.block_on(join_handle).unwrap();

        assert_eq!(
            notifier.lock().unwrap().last_written_lsn,
            data.len() as Lsn
        );
        assert_eq!(notifier.lock().unwrap().poison_pill, true);
    }
}
