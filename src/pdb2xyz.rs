use std::{path::Path, io::{BufReader, BufRead}};
use anyhow::anyhow;
use crate::{pytf_frame::{ATOM_NAME_MAP, AtomNameMap}, pytf_config::Atom};


/// Read atom x, y, z coordinates + types form .pdb file.
/// .pdb format documetation found here:
/// [http://www.wwpdb.org/documentation/file-format-content/format33/sect9.html#HETATM]
/// Byte output format is (all values little-endian):
/// {num_atoms: u32 little-endian}, num_atoms * {x: f32, y: f32, z: f32, type: u8}
pub fn pdb2xyz<P: AsRef<Path>>(pdbfile: P) -> anyhow::Result<Vec<Atom>> {
    let mut pdb = BufReader::new(std::fs::OpenOptions::new().read(true).open(pdbfile)?);
    let mut out = Vec::with_capacity(32);
    let mut line = String::with_capacity(80);

    loop {
        if pdb.read_line(&mut line)? == 0 {
            return Err(anyhow!("No atom definitions found in pdb file"));
        }
        if line.starts_with("HETATM") { break }
        line.clear();
    }

    let name_map = ATOM_NAME_MAP.get_or_init(AtomNameMap::create);
    loop {
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
        line.clear();
        if pdb.read_line(&mut line)? == 0 { break }
    }

    if out.len() > 0 {
        Ok(out)
    } else { Err(anyhow!("No atom coordinates found")) }
}
