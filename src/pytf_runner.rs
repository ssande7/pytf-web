use std::{sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, Condvar}, path::PathBuf};
use crate::pytf::*;

#[derive(Debug)]
pub struct PytfRunner {
    next_config: Arc<(Mutex<Option<PathBuf>>, Condvar)>,
    pytf: Option<Pytf>,
    stop: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub struct PytfHandle {
    next_config: Arc<(Mutex<Option<PathBuf>>, Condvar)>,
    stop: Arc<AtomicBool>,
}

impl PytfHandle {
    pub fn new_config(&self, config: Option<PathBuf>) {
        *(self.next_config.0.lock().unwrap()) = config;
        self.next_config.1.notify_one();
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}

impl PytfRunner {
    pub fn new() -> Self {
        Self {
            next_config: Arc::new((Mutex::new(None),
                Condvar::new())),
            pytf: None,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn get_handle(&self) -> PytfHandle {
        PytfHandle { next_config: self.next_config.clone(), stop: self.stop.clone() }
    }

    pub fn start(mut self) {
        loop {
            // Check for a new configuration to run
            let new_config = { self.next_config.0.lock().unwrap().take() };
            
            // Switch to new configuration if it's there
            if let Some(new_config) = new_config {
                self.pytf = match Pytf::new(new_config) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        eprintln!("Error creating pytf object: {e}");
                        None
                    },
                }
            }

            // Perform the next cycle
            if let Some(pytf) = &mut self.pytf {
                let result =  pytf.cycle();
                if let Err(e) = &result {
                    eprintln!("Error while performing deposition cycle: {e}");
                }
                // TODO: better reporting here for failed vs stopped vs finished.
                if result.is_err() || pytf.run_id() >= pytf.final_run_id() ||
                    self.stop.load(Ordering::Acquire)
                {
                    self.stop.store(false, Ordering::Release);
                    self.pytf = None;
                }
            }

            // If there's nothing left to do, wait for a new config
            if self.pytf.is_none() {
                drop(self.next_config.1.wait(self.next_config.0.lock().unwrap()).unwrap());
            }
        }
    }
}

