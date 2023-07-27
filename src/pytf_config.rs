use std::{collections::HashMap, hash::{Hash, Hasher}};
use num::integer::Integer;
use anyhow::Result;


#[serde_with::skip_serializing_none]
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct PytfConfig {
    name: Option<String>,
    work_directory: Option<String>,
    n_cycles: Option<usize>,

    #[serde(serialize_with = "serialize_f32_1dec")]
    run_time: f32,

    #[serde(serialize_with = "serialize_f32_2dec")]
    deposition_velocity: f32,

    mixture: HashMap<String, MixtureComponent>,
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
        let mut keys = self.mixture.iter().filter_map(|(k, v)| {if v.ratio > 0 {Some(k)} else {None}}).collect::<Vec<&String>>();
        keys.sort();
        for k in keys {
            name.push_str(&format!("_{}-{:x}", k, self.mixture.get(k).unwrap().ratio));
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
    pub fn canonicalize_ratios(&mut self) -> usize {
        // Get all non-zero ratios
        let ratios: Vec<usize> = self.mixture.values().filter_map(|v| if v.ratio > 0 {Some(v.ratio)} else {None}).collect();

        // Return early if the configuration is invalid (no non-zero elements in the mixture)
        if ratios.len() == 0 { return 0; }

        // Canonicalize
        let gcd = ratios.iter().fold(
                *(ratios.iter().max().unwrap()),
                |acc, v| acc.gcd(v)
            );
        for v in self.mixture.values_mut() {
            v.ratio /= gcd;
        }
        gcd
    }

    pub fn prefill(&mut self) {
        if self.name.is_none() {
            self.name = Some(self.get_name());
        }
        self.work_directory = Some(format!("work_{}", self.name.as_ref().unwrap()));
        for (k, v) in self.mixture.iter_mut() {
            if v.ratio > 0 {
                v.fill_with_name(&k);
            }
        }
        self.n_cycles = Some(50); // TODO: calculate this from atom counts and ratios
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct MixtureComponent {
    res_name: Option<String>,
    pdb_file: Option<String>,
    itp_file: Option<String>,
    ratio: usize
}

// TODO: Make resources/ a configurable directory
impl MixtureComponent {
    fn fill_with_name<S: AsRef<str>>(&mut self, name: S) {
        self.res_name = Some(name.as_ref().to_owned());
        self.pdb_file = Some(format!("resources/molecules/{}.pdb", name.as_ref()));
        self.itp_file = Some(format!("resources/molecules/{}.itp", name.as_ref()));
    }
}

