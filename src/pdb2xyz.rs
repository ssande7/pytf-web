use std::path::Path;
use anyhow::anyhow;
use crate::{pytf_frame::{ATOM_NAME_MAP, AtomNameMap}, pytf_config::Atom};


/// Read atom x, y, z coordinates + types form .pdb file.
/// .pdb format documetation found here:
/// [http://www.wwpdb.org/documentation/file-format-content/format33/sect9.html#HETATM]
/// Byte output format is (all values little-endian):
/// {num_atoms: u32 little-endian}, num_atoms * {x: f32, y: f32, z: f32, type: u8}
pub fn pdb2xyz<P: AsRef<Path>>(pdbfile: P) -> anyhow::Result<Vec<Atom>> {
    let pdb = std::fs::read_to_string(pdbfile)?;
    pdbstr2xyz(&pdb)
}

/// Convert pdb file contents to x,y,z,type message
fn pdbstr2xyz(pdb: &str) -> anyhow::Result<Vec<Atom>> {
    let mut out = Vec::with_capacity(32);
    let lines = pdb.lines().skip_while(|line| !line.starts_with("HETATM"));
    let name_map = ATOM_NAME_MAP.get_or_init(
        || AtomNameMap::from_cli_or_default(std::env::args()));

    for line in lines {
        if !line.starts_with("HETATM") { break }
        let x: f32 = line.get(30..38).ok_or(anyhow!("x coordinate not found"))?.trim().parse()?;
        let y: f32 = line.get(38..46).ok_or(anyhow!("y coordinate not found"))?.trim().parse()?;
        let z: f32 = line.get(46..54).ok_or(anyhow!("z coordinate not found"))?.trim().parse()?;
        let typ = line.get(76..78)
            .and_then(|s| {
                name_map.map.get(
                    s.trim()
                     .to_ascii_uppercase()
                     .trim_end_matches(char::is_numeric)
                )
            })
            .ok_or(anyhow!("Atom type not found"))?;
        out.push(Atom { x, y, z, typ: *typ });
    }
    if out.len() > 0 {
        Ok(out)
    } else { Err(anyhow!("No atom coordinates found")) }
}
