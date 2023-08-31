use std::{path::Path, fmt::Display};
use anyhow::Result;
use pyo3::prelude::*;
// NOTE: pyo3 requires python3-dev installed
// (included in Arch python3, but maybe not others)

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
    /// ID of the _next_ run to be performed
    run_id: i32,
    final_run_id: i32,
}

// TODO: make this a cfg switch
const PYTF_DEBUG: bool = cfg!(pytf_debug);

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
            })
        })
    }

    /// Perform one deposition cycle.
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

    pub fn last_finished_run(&self) -> u32 {
        if self.run_id > 0 { self.run_id as u32 - 1 } else { 0 }
    }

    #[inline(always)]
    pub fn final_run_id(&self) -> i32 {
        self.final_run_id
    }
}

pub enum PytfFile {
    Log,
    InputCoords,
    FinalCoords,
    Trajectory,
}
impl PytfFile {
    pub fn ext(&self) -> &'static str {
        match self {
            Self::Log => "log",
            Self::InputCoords
                | Self::FinalCoords
                => "gro",
            Self::Trajectory => "xtc",
        }
    }

    pub fn content(&self) -> &'static str {
        match self {
            Self::Log => "log",
            Self::InputCoords => "input-coordinates",
            Self::FinalCoords => "final-coordinates",
            Self::Trajectory => "trajectory",
        }
    }

    pub fn path(&self, workdir: impl AsRef<str>, jobname: impl AsRef<str>, run_id: u32) -> String {
        let workdir = workdir.as_ref();
        let jobname = jobname.as_ref();
        let out = format!("{workdir}/{self}/{jobname}_{self}_{run_id}.{}", self.ext());
        println!("Generated path: {out}");
        out
    }
}
impl Display for PytfFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.content())
    }
}

