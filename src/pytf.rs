use std::path::Path;
use anyhow::Result;
use pyo3::prelude::*;
use xdrfile::prelude::*;
// NOTE: pyo3 requires python3-dev installed (included in Arch python3, but maybe not others)

/// Error type for anyhow compatibility
#[derive(Debug, Copy, Clone)]
pub enum PytfError {
    CycleFailed,
}
impl std::fmt::Display for PytfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CycleFailed => f.write_str("PyTF cycle failed")
        }
    }
}
impl std::error::Error for PytfError {}


/// Manager for a PyThinFilm deposition instance
#[derive(Debug)]
pub struct Pytf {
    deposition: Py<PyAny>,
    run_id: i32,
    final_run_id: i32,
    trajectory: Vec<XTCFrame>,
    // traj_writer: Option<Thread>,
}

// TODO: make this a cfg switch
const PYTF_DEBUG: bool = true;

impl Pytf {
    /// Create a PyThinFilm.deposition.Deposition object from a pytf config file
    pub fn new<P: AsRef<Path>>(config: P) -> PyResult<Self> {
        Python::with_gil(|py| -> PyResult<Self> {
            let pytf = py.import("PyThinFilm.deposition")?
                .getattr("Deposition")?
                .call1((config.as_ref().as_os_str(), PYTF_DEBUG))?;
            let run_id: i32 = pytf.getattr("run_ID")?.extract()?;
            let final_run_id: i32 = pytf.getattr("last_run_ID")?.extract()?;
            Ok(Self {
                deposition: pytf.into(),
                run_id,
                final_run_id,
                trajectory: Vec::with_capacity(final_run_id as usize),
                // traj_writer: None,
            })
        })
    }

    /// Perform one run cycle. Returns `Ok(Some(deposition.run_ID))` if successful.
    pub fn cycle(&mut self) -> Result<()> {
        Python::with_gil(|py| -> Result<()> {
            let success = self.deposition.call_method0(py, "cycle")?.extract(py)?;
            // if let Some(writer) = self.traj_writer {
            //     writer.join();
            // }
            if success {
                self.run_id = self.deposition.getattr(py, "run_ID")?.extract(py)?;
                Ok(())
            } else {
                Err(PytfError::CycleFailed.into())
            }
        })
    }

    #[inline(always)]
    pub fn run_id(&self) -> i32 {
        self.run_id
    }

    #[inline(always)]
    pub fn final_run_id(&self) -> i32 {
        self.final_run_id
    }
}

