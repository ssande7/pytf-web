use std::{hash::{Hash, Hasher}, sync::OnceLock, path::{Path, PathBuf}};
use num::integer::Integer;
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::pdb2xyz::pdb2xyz;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Atom {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub typ: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixtureComponentDetailed {
    res_name: String,
    name: String,
    formula: String,
    smiles: String,
    atoms: Option<Vec<Atom>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoleculeResources {
    molecules: Vec<MixtureComponentDetailed>
}

impl MoleculeResources {
    pub fn load(file: impl AsRef<Path>) -> anyhow::Result<Self> {
        let mut molecules: MoleculeResources =
            serde_json::from_str(&std::fs::read_to_string(&file)?)?;
        log::debug!("Beginning parsing .pdb files");
        let path = RESOURCES_DIR.get().unwrap().join("molecules");
        for mol in molecules.molecules.iter_mut() {
            mol.atoms = Some({
                pdb2xyz(path.join(format!("{}.pdb", mol.res_name)))
                    .expect(&format!("Failed to parse pdb file for {}", mol.res_name))
            });
        }
        log::debug!("Done parsing .pdb files");
        Ok(molecules)
    }
}

/// Working directory to store PyThinFilm data
pub static WORK_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Resources directory containing config files and molecules directory with .pdb and .itp files
pub static RESOURCES_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Molecules available for deposition.
/// Parsed from JSON and filled with molecule 3D structure from .pdb file.
pub static AVAILABLE_MOLECULES: OnceLock<MoleculeResources> = OnceLock::new();

// TODO: Make this configurable
pub const INSERTIONS_PER_RUN: usize = 4;
pub const DEPOSITION_STEPS:   usize = 36;
pub const PS_PER_FRAME: f32 = 100. * 0.0025; // nstout * dt
// pub const TARGET_ATOMS_TOTAL: usize = 1300;
pub const INSERT_DISTANCE:  f32 = 2f32;
pub const RUN_TIME_MINIMUM: f32 = 18f32;
pub const DEFAULT_DEPOSITION_VELOCITY: f32 = 0.35;

/// Full information about simulation to be appended
/// to base config file.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PytfConfig {
    pub name: String,
    pub work_directory: String,
    pub n_cycles: usize,

    /// Duration of each deposition step
    #[serde(serialize_with = "serialize_f32_1dec", default)]
    pub run_time: f32,

    #[serde(serialize_with = "serialize_f32_2dec")]
    pub deposition_velocity: f32,

    pub mixture: Vec<MixtureComponent>,
}

impl PytfConfig {
    pub fn archive_name(&self) -> String {
        format!("{}.archive", self.name)
    }

    /// Set the working directory to be a sub-directory with the
    /// same name as the job's name under the global `WORK_DIR` directory.
    /// If successful, returns `Some(self)` with the modified `work_directory` member.
    ///
    /// # Errors
    /// Returns `None` if the full working directory is not valid UTF-8.
    ///
    /// # Panics
    /// If `WORK_DIR` has not been set.
    pub fn set_work_dir(mut self) -> Option<Self> {
        self.work_directory = WORK_DIR.get().unwrap().join(&self.name)
        .to_str()?.to_owned();
        Some(self)
    }
}

/// Minimal config information sent from
/// client to be filled into full PytfConfig
#[derive(Deserialize, Clone, Debug)]
pub struct PytfConfigMinimal {
    deposition_velocity: f32,
    mixture: Vec<MixtureComponent>,
}

impl From<PytfConfigMinimal> for PytfConfig {
    fn from(mut config: PytfConfigMinimal) -> Self {
        // Remove zero ratios
        config.mixture.retain(|v| v.ratio > 0);

        // Avoid multiple versions of empty job
        if config.mixture.is_empty() {
            config.deposition_velocity = DEFAULT_DEPOSITION_VELOCITY;
        }

        // Sort by res_name for consistent ordering
        config.mixture.sort_by(|a, b| (&a.res_name).cmp(&b.res_name));

        // Normalise ratios and calculate atoms per step
        let gcd = config.mixture.iter().fold(
                config.mixture.iter().map(|v| v.ratio).max().unwrap_or(1),
                |acc, v| acc.gcd(&v.ratio)
            );
        // let mut ratio_tot = 0;
        // let mut atoms_per_step = 0;
        for mol in config.mixture.iter_mut() {
            mol.fill_fields();
            mol.ratio /= gcd;
            // ratio_tot += mol.ratio;
            // let natoms = AVAILABLE_MOLECULES.get().unwrap()
            //     .molecules
            //     .iter().find_map(|m| {
            //         if m.res_name == mol.res_name {
            //             Some(m.natoms)
            //         } else { None }
            //     }).unwrap_or(0);
            // atoms_per_step += mol.ratio * natoms;
        }
        // let atoms_per_step = if ratio_tot > 0 {
        //     (INSERTIONS_PER_RUN * atoms_per_step) as f32 / ratio_tot as f32
        // } else { 1f32 };
        let n_cycles = DEPOSITION_STEPS; //(TARGET_ATOMS_TOTAL as f32 / atoms_per_step).ceil() as usize;
        let run_time = (INSERT_DISTANCE / config.deposition_velocity) + RUN_TIME_MINIMUM;
        // Avoid weird steps in time between trajectory frames
        let run_time = (run_time / PS_PER_FRAME).ceil() * PS_PER_FRAME;
        let mut name = String::with_capacity(config.mixture.len()*15+10);
        name.push_str(&format!("{:.1}_{:.2}", run_time, config.deposition_velocity));

        // Mixture is normalised and sorted by canonicalize_ratios(), so names should be consistent
        // for the same config.
        for mol in &config.mixture {
            name.push_str(&format!("_{}-{:x}", mol.res_name, mol.ratio));
        }
        Self {
            name,
            work_directory: "".into(), // Placeholder work_directory to be filled by worker
            n_cycles,
            run_time,
            deposition_velocity: config.deposition_velocity,
            mixture: config.mixture,
        }
    }
}

impl Default for PytfConfigMinimal {
    fn default() -> Self {
        Self { deposition_velocity: DEFAULT_DEPOSITION_VELOCITY, mixture: Vec::new() }
    }
}


fn serialize_f32_1dec<S: serde::Serializer>(x: &f32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f32((x*10.).round()/10.)
}
fn serialize_f32_2dec<S: serde::Serializer>(x: &f32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f32((x*100.).round()/100.)
}

impl Default for PytfConfig {
    fn default() -> Self {
        PytfConfigMinimal::default().into()
    }
}


impl Hash for PytfConfig {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl PartialEq for PytfConfig {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
    fn ne(&self, other: &Self) -> bool {
        self.name != other.name
    }
}

// Client sends res_name and ratio. We fill in other data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixtureComponent {
    res_name: String,
    pdb_file: Option<String>,
    itp_file: Option<String>,
    #[serde(deserialize_with = "deserialize_usize")]
    ratio: usize
}

fn deserialize_usize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<usize, D::Error> {
    let num = f64::deserialize(d)?;
    Ok(f64::trunc(num) as usize)
}

impl MixtureComponent {
    fn fill_fields(&mut self) {
        let mut path = RESOURCES_DIR.get().unwrap().join("molecules");
        self.pdb_file = Some(path.join(format!("{}.pdb", &self.res_name))
            .to_str().expect("Non UTF-8 file path!").to_owned());
        path.push(format!("{}.itp", &self.res_name));
        self.itp_file = Some(path.to_str().expect("Non UTF-8 file path!").to_owned());
    }
}

