use std::{io::{Read, BufReader}, sync::OnceLock, collections::HashMap, fs::File, env::Args, ffi::CStr};

use actix::prelude::*;
use actix_web::web::Bytes;
use anyhow::anyhow;
use awc::ws;
use xdrfile::{XDRFile, access_mode};

use crate::{
    worker_client::{PytfServer, SEGMENT_HEADER, WsMessage},
    pytf::PytfFile
};


/// One deposition cycle of a trajectory.
/// Binary data stored as little endian
/// FORMAT:
/// - {segment_id: u32}
/// - {num_frames: u32}
/// - {num_particles: u32}
/// - [num_particles x {particle_type: u8}]
/// - [num_frames x [num_particles x {x: f32}{y: f32}{z: f32}]]
#[derive(Debug, Clone)]
pub struct TrajectorySegment {
    data: Bytes
}

pub struct AtomNameMap {
    map: HashMap<String, u8>,
}
pub static ATOM_NAME_MAP: OnceLock<AtomNameMap> = OnceLock::new();
impl AtomNameMap {
    pub fn from_cli_or_default(mut args: Args) -> Self {
        let mut atom_map_file = "resources/atom_types.json".into();
        while let Some(arg) = args.next() {
            if arg == "--atom-map" {
                let Some(fname) = args.next() else {
                    panic!("No atom map file specified with --atom-map flag");
                };
                atom_map_file = fname;
            }
        }
        Self {
            map: serde_json::from_reader(
                    BufReader::new(
                        File::open(atom_map_file)
                            .expect("Couldn't open atom map file: {atom_map_file}")))
                .expect("Failed to read atom map file: {atom_map_file}")
        }
    }
}

impl TrajectorySegment {
    /// Store trajectory segment from raw bytes message. Assumes message contains correct data.
    pub fn new(raw_data: Bytes) -> Self {
        Self { data: raw_data }
    }

    /// Get a reference to the data.
    /// Might be replaced with something that loads from disk if memory requirement is too high.
    pub fn data(&self) -> Bytes {
        self.data.clone()
    }

    fn from_files<R: Read>(xtcfile: XDRFile<access_mode::Read>, mut grofile: BufReader<R>, segment_id: u32) -> anyhow::Result<Self> {
        let Ok(natoms) = xtcfile.read_xtc_natoms() else { Err(anyhow!("Failed to read natoms from xtc file"))? };
        let mut out: Vec<u8> = Vec::with_capacity(12 + 250 * natoms * 13); // Pre-allocating for up to 250 frames
        out.extend_from_slice(&segment_id.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());     // Reserve 4 bytes to write in number of frames
        out.extend_from_slice(&(natoms as u32).to_le_bytes());
        {
            // Read atom types from gro file
            let mut gro_buf = String::new();
            grofile.read_to_string(&mut gro_buf)?;
            let mut lines = gro_buf.lines().skip(1); // Skip title line
            // Get number of atoms
            let Some(gro_natoms) = lines.next() else { Err(anyhow!("Missing natoms line in .gro file"))? };
            let gro_natoms = gro_natoms.trim().parse::<usize>()?;
            if natoms != gro_natoms { Err(anyhow!("Number of atoms doesn't match between .xtc and .gro files"))? }
            out.extend(
                lines.take(natoms).map(|line| {
                    // Atom name takes 5 characters, starting at index 10.
                    // Strip numeric suffix to get plain atom type
                    line.get(10..15)
                        .and_then(|s| {
                            ATOM_NAME_MAP.get().unwrap().map.get(
                                s.trim().trim_end_matches(char::is_numeric)
                            )
                        })
                        .unwrap_or(&0u8)
                })
            );
            if out.len() != 12 + natoms { Err(anyhow!("Atoms missing from .gro file"))? }
            println!("Extracted {natoms} atoms from .gro file to pack segment");
        }
        let mut nframes: u32 = 0;
        while let Ok(frame) = xtcfile.read_xtc(natoms) {
            out.reserve(natoms * 12);
            out.extend(frame.x.iter()
                .flat_map(|xyz| xyz.0.map(
                    |v| v.to_le_bytes()
                )).flatten()
            );
            nframes += 1;
        }
        out[4..8].copy_from_slice(&nframes.to_le_bytes());
        println!("Wrote {nframes} frames to segment");
        Ok(Self { data: out.into() })
    }

    pub fn pack_for_ws(&self, jobname: impl AsRef<str>) -> Vec<u8> {
        [
            SEGMENT_HEADER,
            format!("{}\0", jobname.as_ref()).as_bytes(),
            self.data.as_ref()
        ].concat()
    }
}

#[derive(Message)]
#[rtype(result="anyhow::Result<()>")]
pub struct SegToProcess {
    workdir: String,
    jobname: String,
    segment_id: u32,
}
impl SegToProcess {
    pub fn new(workdir: String, jobname: String, segment_id: u32) -> Self {
        Self { workdir, jobname, segment_id }
    }
}


#[derive(Debug)]
pub struct SegmentProcessor {
    socket: Addr<PytfServer>
}

impl Actor for SegmentProcessor {
    type Context = Context<Self>;
}

impl SegmentProcessor {
    pub fn new(socket: Addr<PytfServer>) -> Self {
        Self { socket }
    }
}

impl Handler<SegToProcess> for SegmentProcessor {
    type Result = anyhow::Result<()>;
    fn handle(&mut self, msg: SegToProcess, _ctx: &mut Self::Context) -> Self::Result {
        let xtc_path = format!("{}\0",
            PytfFile::Trajectory.path(
                &msg.workdir,
                &msg.jobname,
                msg.segment_id
            )
        );
        let xtcfile = match XDRFile::<access_mode::Read>::open(
            unsafe {CStr::from_bytes_with_nul_unchecked(xtc_path.as_bytes()) })
        {
            Ok(xtcfile) => xtcfile,
            Err(e) => {
                return Err(anyhow!("{e:?}"))
            }
        };
        let grofile = BufReader::new(
            File::open(PytfFile::InputCoords.path(
                &msg.workdir,
                &msg.jobname,
                msg.segment_id
            ))?
        );
        self.socket.do_send(WsMessage(ws::Message::Binary(
            TrajectorySegment::from_files(xtcfile, grofile, msg.segment_id)?
                .pack_for_ws(&msg.jobname).into()
        )));
        Ok(())
    }
}
