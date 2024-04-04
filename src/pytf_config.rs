use std::{
    hash::{Hash, Hasher},
    sync::OnceLock,
    path::{Path, PathBuf},
    collections::HashMap,
    fmt::Display,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use num::integer::Integer;

use crate::{
    pdb2xyz::pdb2xyz,
    input_config::{ValueRange, ConfigSettings, ConfigSettingsValue},
};


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

/// Default number of deposition cycles if not specified
pub const DEFAULT_N_CYCLES:   usize = 36;


/// Full information about simulation to be appended
/// to base config file.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PytfConfig {
    pub name: String,
    pub work_directory: String,
    pub n_cycles: usize,

    #[serde(flatten)]
    pub config: PytfConfigMinimal,
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
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PytfConfigMinimal {
    mixture: Vec<MixtureComponent>,
    #[serde(flatten)]
    settings: HashMap<String, serde_json::Value>,
}

impl PytfConfigMinimal {
    /// Sanitize settings using `base_config` to generate a full configuration
    pub fn build(mut self, base_config: &ConfigSettings) -> PytfConfig {
        // Sort by res_name for consistent ordering
        self.mixture.sort_by(|a, b| (&a.res_name).cmp(&b.res_name));

        // Normalise ratios and calculate atoms per step
        let gcd = self.mixture.iter().fold(
                self.mixture.iter().map(|v| v.ratio).max().unwrap_or(1),
                |acc, v| acc.gcd(&v.ratio)
            );
        for mol in self.mixture.iter_mut() {
            mol.fill_fields();
            mol.ratio /= gcd;
        }

        // Mixture is normalised and sorted by canonicalize_ratios(), so names should be consistent
        // for the same config.
        let mut name = String::with_capacity(self.mixture.len()*15 + self.settings.len()*10);
        let mut first = true;
        for mol in &self.mixture {
            if mol.ratio == 0 { continue }
            if first { first = false; } else { name.push_str("_"); }
            name.push_str(&mol.res_name);
            name.push_str("-");
            name.push_str(&mol.ratio.to_string());
        }

        // Get keys for independent settings variables. All others set by base_config, so either
        // constant or derived.
        let mut keys: Vec<String> = self.settings.keys().map(|k| k.to_owned()).collect();
        keys.sort();

        // Apply base config to settings to insert any extra properties and sanitize values
        base_config.apply(&mut self.settings);
        for key in keys {
            name.push_str("_");
            let val = &self.settings[&key];
            if val.is_f64() {
                if let Some(ConfigSettingsValue::FloatRange(
                    ValueRange { dec_places: Some(d), .. }
                )) = base_config.settings.get(key.as_str()) {
                    format!("{:.1$}", val.as_f64().unwrap(), *d as usize);
                }
            }
            name.push_str(&self.settings[&key].to_string());
        }

        // Extract number of cycles for easy future access, or set it to the default
        // if not present
        let n_cycles = match self.settings.remove("n_cycles").and_then(|n| n.as_u64()) {
            Some(n) => n as usize,
            None => DEFAULT_N_CYCLES
        };

        PytfConfig {
            name,
            work_directory: "".into(), // Placeholder work_directory to be filled by worker
            n_cycles,
            config: self,
        }
    }
}

impl Display for PytfConfigMinimal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("{ mixture: { ")?;
        let mut first = true;
        for mol in &self.mixture {
            if mol.ratio == 0 { continue }
            if first {
                first = false;
                f.write_fmt(format_args!("{}", mol))?;
            } else {
                f.write_fmt(format_args!(", {}", mol))?;
            }
        }
        f.write_fmt(format_args!(" }}, protocol: {:?} }}", self.settings))
    }
}

impl Default for PytfConfig {
    fn default() -> Self {
        let config = PytfConfigMinimal::default();
        config.build(&ConfigSettings::default())
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

impl Display for MixtureComponent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}: {}", self.res_name, self.ratio))
    }
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
