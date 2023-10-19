use std::{hash::{Hash, Hasher}, env::Args, sync::OnceLock};
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
    pub fn from_cli_or_default(mut args: Args) -> Self {
        // TODO: proper error handling
        let mut mols_file = "resources/name_map.json".into();
        while let Some(arg) = args.next() {
            if arg == "-m" || arg == "--molecules" {
                mols_file = args.next().expect("Missing argument for -m/--molecules. Please provide a json file.");
                break;
            }
        }
        let mut molecules: MoleculeResources = serde_json::from_str(
            &std::fs::read_to_string(&mols_file)
                .expect(&format!("Failed to read molecules json file: {}", &mols_file))
        ).expect("Failed to parse molecules json file");
        for mol in molecules.molecules.iter_mut() {
            mol.atoms = Some(
                pdb2xyz(format!("resources/molecules/{}.pdb", mol.res_name))
                    .expect(&format!("Failed to parse pdb file for {}", mol.res_name))
            );
        }
        molecules
    }
}

// TODO: Make this configurable
pub static AVAILABLE_MOLECULES: OnceLock<MoleculeResources> = OnceLock::new();
pub const INSERTIONS_PER_RUN: usize = 4;
pub const DEPOSITION_STEPS:   usize = 36;
pub const TARGET_ATOMS_TOTAL: usize = 1300;
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
        let mut name = String::with_capacity(config.mixture.len()*15+10);
        name.push_str(&format!("{:.1}_{:.2}", run_time, config.deposition_velocity));

        // Mixture is normalised and sorted by canonicalize_ratios(), so names should be consistent
        // for the same config.
        for mol in &config.mixture {
            name.push_str(&format!("_{}-{:x}", mol.res_name, mol.ratio));
        }
        let work_directory = format!("work_{name}"); // TODO: Pass in work directory root and use here
        Self {
            name,
            work_directory,
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
    ratio: usize
}

// TODO: Make resources/ a configurable directory
impl MixtureComponent {
    fn fill_fields(&mut self) {
        self.pdb_file = Some(format!("resources/molecules/{}.pdb", &self.res_name));
        self.itp_file = Some(format!("resources/molecules/{}.itp", &self.res_name));
    }
}

