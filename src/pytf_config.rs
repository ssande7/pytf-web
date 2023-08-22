use std::{hash::{Hash, Hasher}, sync::OnceLock, env::Args};
use num::integer::Integer;
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixtureComponentDetailed {
    res_name: String,
    name: String,
    formula: String,
    natoms: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoleculeResources{
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
        serde_json::from_str(
            &std::fs::read_to_string(&mols_file)
                .expect(&format!("Failed to read molecules json file: {}", &mols_file))
        ).expect("Failed to parse molecules json file")
    }
}

pub static AVAILABLE_MOLECULES: OnceLock<MoleculeResources> = OnceLock::new();

#[serde_with::skip_serializing_none]
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PytfConfig {
    name: Option<String>,
    work_directory: Option<String>,
    n_cycles: Option<usize>,

    /// Duration of each deposition step
    // TODO: calculate this later based on deposition velocity? Or allow as user input?
    #[serde(serialize_with = "serialize_f32_1dec", default)]
    run_time: f32,

    #[serde(serialize_with = "serialize_f32_2dec")]
    deposition_velocity: f32,

    mixture: Vec<MixtureComponent>,
}


fn serialize_f32_1dec<S: serde::Serializer>(x: &f32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("{x:.1}"))
}
fn serialize_f32_2dec<S: serde::Serializer>(x: &f32, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("{x:.2}"))
}

impl PytfConfig {
    /// NOTE: Should call `canonicalize_ratios() first to make sure equivalent deposition ratios
    ///       map to the same name.
    pub fn get_name(&self) -> String {
        let mut name = String::with_capacity(512);
        name.push_str(&format!("{:.1}_{:.2}", self.run_time, self.deposition_velocity));

        // Sort keys and filter out zero values so equivalent systems have the same name/hash
        // let mut keys = self.mixture.iter().filter_map(|(k, v)| {if v.ratio > 0 {Some(k)} else {None}}).collect::<Vec<&String>>();
        // keys.sort();

        // Mixture is normalised and sorted by canonicalize_ratios(), so names should be consistent
        // for the same config.
        for mol in &self.mixture {
            name.push_str(&format!("_{}-{:x}", mol.res_name, mol.ratio));
        }
        name
    }

    pub fn calc_name(&mut self) {
        self.name = Some(self.get_name());
    }

    pub fn name(&self) -> Option<&String> {
        self.name.as_ref()
    }

    pub fn workdir(&self) -> Option<&String> {
        self.work_directory.as_ref()
    }

    /// Calculates the greatest common denominator of all ratio values and returns it.
    /// If the GCD is > 0, ratios are divided by that value before returning.
    /// If the GCD == 0, there are no non-zero ratio values in the mixture.
    pub fn canonicalize(&mut self) -> usize {
        // Remove zero ratios
        self.mixture.retain(|v| v.ratio > 0);

        // Return early if the configuration is invalid (no non-zero elements in the mixture)
        if self.mixture.len() == 0 { return 0; }

        // Canonicalize
        self.mixture.sort_by(|a, b| (&a.res_name).cmp(&b.res_name));
        let gcd = self.mixture.iter().fold(
                self.mixture.iter().map(|v| v.ratio).max().unwrap(),
                |acc, v| acc.gcd(&v.ratio)
            );
        for mol in self.mixture.iter_mut() {
            mol.ratio /= gcd;
        }
        gcd
    }

    pub fn prefill(&mut self) {
        if self.name.is_none() {
            self.name = Some(self.get_name());
        }
        self.work_directory = Some(format!("work_{}", self.name.as_ref().unwrap()));
        for v in self.mixture.iter_mut() {
            v.fill_fields();
        }
        self.n_cycles = Some(50); // TODO: calculate this from atom counts and ratios
        // TODO: calculate self.run_time
    }
}

impl Hash for PytfConfig {
    fn hash<H: Hasher>(&self, state: &mut H) {
        if let Some(name) = &self.name {
            name.hash(state);
        } else {
            self.get_name().hash(state);
        }
    }
}

impl PartialEq for PytfConfig {
    fn eq(&self, other: &Self) -> bool {
        match (&self.name, &other.name) {
            (Some(n), Some(o)) => n == o,
            (Some(n), None) => *n == other.get_name(),
            (None, Some(o)) => self.get_name() == *o,
            (None, None) => self.get_name() == other.get_name(),
        }
    }
    fn ne(&self, other: &Self) -> bool {
        match (&self.name, &other.name) {
            (Some(n), Some(o)) => n != o,
            (Some(n), None) => *n != other.get_name(),
            (None, Some(o)) => self.get_name() != *o,
            (None, None) => self.get_name() != other.get_name(),
        }
    }
}

// Client sends res_name and ratio. We fill in other data
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MixtureComponent {
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

