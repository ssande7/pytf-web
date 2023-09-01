use std::{io::Write, path::PathBuf, fs::File};
use actix::{Actor, Context, Message, Handler, Addr, ActorContext, AsyncContext};
use awc::ws;
use serde::{Deserialize, Serialize};
use bincode;

use crate::{
    pytf::*,
    pytf_config::PytfConfig,
    worker_client::{
        PytfServer, WsMessage,
        PAUSE_HEADER, FAILED_HEADER, DONE_HEADER
    },
    pytf_frame::{SegmentProcessor, SegToProcess}
};

/// Actor to handle running a single deposition simulation.
/// Reports results back to the `PytfServer` instance that spawned it.
///
/// Receives:
/// * Stop signal
/// * Cycle signal (from self)
///
/// Sends:
/// * Cycle signal (to self)
/// * Done signal
/// * Failed signal
/// * Pause data
#[derive(Debug)]
pub struct PytfRunner {
    pytf: Pytf,
    config: PytfConfig,
    socket: Addr<PytfServer>,
    segment_proc: Addr<SegmentProcessor>,
}

impl Actor for PytfRunner {
    type Context = Context<Self>;
    fn stopping(&mut self, _ctx: &mut Self::Context) -> actix::Running {
        // If simulation isn't finished, forward on pause data to socket,
        // or send failed signal if we can't pack them for some reason.
        let run_id = self.pytf.run_id();
        if run_id < self.pytf.final_run_id() {
            match PytfPauseFiles::new(
                &self.config.work_directory,
                &self.config.name,
                self.pytf.last_finished_run()
            ).and_then(|p| Ok(p.pack()))
            {
                Ok(Ok(pause_files)) =>
                    self.socket.do_send(WsMessage(ws::Message::Binary([
                            PAUSE_HEADER,
                            format!("{}\0", self.config.name).as_bytes(),
                            &pause_files
                        ].concat().into()
                    ))),
                e => {
                        eprintln!("Failed to pack pause data: {e:?}");
                        self.send_failed();
                    }
            };
        }
        actix::Running::Stop
    }
}


/// Signal sent to PytfRunner to prevent next cycle from starting.
/// Current cycle will still be completed.
#[derive(Message)]
#[rtype(result="()")]
pub struct PytfStop {
    pub jobname: Option<String>
}

/// Signal sent to PytfRunner to initiate next deposition cycle
#[derive(Message)]
#[rtype(result="()")]
pub struct PytfCycle {}

/// Files necessary for restarting a deposition simulation from part-way through
/// Can be packed to `Bytes` to send over network, and unpacked when received.
#[derive(Serialize, Deserialize)]
pub struct PytfPauseFiles {
    pub run_id: u32,
    pub log: String,
    pub coords: String,
}

impl PytfPauseFiles {
    /// Load pause file contents into memory
    pub fn new<S: AsRef<str>>(workdir: S, jobname: S, jobid: u32) -> std::io::Result<Self> {
        let workdir = workdir.as_ref();
        let jobname = jobname.as_ref();

        // Only need last 10 lines of log file, so disregard the rest
        let log = std::fs::read_to_string(
                PytfFile::Log.path(workdir, jobname, jobid)
            )?;
        let mut log = log.rsplit('\n').take(10).collect::<Vec<&str>>();
        log.reverse();
        let log = log.join("\n");

        // Package up log and final-coordinates files
        Ok(Self {
            run_id: jobid,
            log,
            coords: std::fs::read_to_string(
                PytfFile::FinalCoords.path(workdir, jobname, jobid)
            )?,
        })

    }

    /// Pack pause file contents into a buffer
    pub fn pack(&self) -> bincode::Result<Vec<u8>> {
        bincode::serialize(self)
    }

    /// Unpack pause file data from a `STEAL_HEADER` buffer.
    /// Assumes any headers have been stripped off
    /// (i.e. no `STEAL_HEADER` or job config)
    pub fn unpack(bytes: &[u8]) -> bincode::Result<Self> {
        bincode::deserialize(bytes)
    }

    /// Write pause files to disk ready to be resumed from
    pub fn to_disk(&self, workdir: impl AsRef<str>, jobname: impl AsRef<str>) -> std::io::Result<()> {
        std::fs::create_dir_all(format!("{}/{}", workdir.as_ref(), PytfFile::Log))?;
        std::fs::create_dir_all(format!("{}/{}", workdir.as_ref(), PytfFile::FinalCoords))?;
        std::fs::create_dir_all(format!("{}/{}", workdir.as_ref(), PytfFile::InputCoords))?;
        File::options().write(true).create(true)
            .open(PytfFile::Log.path(&workdir, &jobname, self.run_id))?
            .write(&self.log.as_bytes())?;
        File::options().write(true).create(true)
            .open(PytfFile::FinalCoords.path(&workdir, &jobname, self.run_id))?
            .write(&self.coords.as_bytes())?;
        // Input coordinates file of this run just needs to exist
        File::options().append(true).create(true)
            .open(PytfFile::InputCoords.path(&workdir, &jobname, self.run_id))?
            .write(b"")?;
        Ok(())
    }
}


impl Handler<PytfStop> for PytfRunner {
    type Result = ();
    /// Received stop signal, so make sure it's for my current job and stop if so
    fn handle(&mut self, msg: PytfStop, ctx: &mut Self::Context) -> Self::Result {
        if msg.jobname.as_ref() == Some(&self.config.name) || msg.jobname.is_none() {
            ctx.stop(); // Pause data packed while stopping
        } else {
            println!("Received stop signal for different job: {}", msg.jobname.unwrap());
        }
    }
}

impl Handler<PytfCycle> for PytfRunner {
    type Result = ();
    fn handle(&mut self, _: PytfCycle, ctx: &mut Self::Context) -> Self::Result {
        if let Err(e) = self.pytf.cycle() {
            eprintln!("Error while performing deposition cycle: {e}");
            self.send_failed();
            return
        }
        if self.pytf.run_id() >= self.pytf.final_run_id() {
            println!("Completed final cycle. Exiting.");
            self.send_done();
        } else {
            let run_id = self.pytf.last_finished_run();
            println!("Completed cycle {run_id} successfully. Queing next cycle.");

            // Create trajectory packer to send out segment
            // as run id + jobname + workdir to be processed and sent
            self.segment_proc.do_send(
                SegToProcess::new(
                    self.config.work_directory.clone(),
                    self.config.name.clone(),
                    run_id,
                )
            );

            // Send a cycle message to myself to start the next cycle
            // This allows a PytfStop signal to get through and stop the
            // next cycle from happening if necessary.
            ctx.address().do_send(PytfCycle {});
        }
    }
}

impl PytfRunner {
    /// Set up an actor with a `Pytf` python instance ready to run a simulation
    pub fn new(config: PytfConfig, socket: Addr<PytfServer>, segment_proc: Addr<SegmentProcessor>) -> anyhow::Result<Self> {
        // Get yaml string to append to config
        let yml = serde_yaml::to_string(&config)?;

        // Create working directory if it doesn't already exist
        let mut config_yml = PathBuf::from(&config.work_directory);
        if !config_yml.is_dir() {
            std::fs::create_dir(&config_yml)?;
        }

        // Create config.yml in working directory if it doesn't already exist
        config_yml.push("config.yml");
        if !config_yml.is_file() {
            std::fs::copy("resources/base_config.yml", &config_yml)?;

            // Write config.yml to jobname directory
            let mut config_file = std::fs::OpenOptions::new()
                .write(true)
                .append(true)
                .open(&config_yml)?;

            // Fill in config file
            writeln!(config_file, "\n{}", yml)?;
        }

        Ok(Self {
            pytf: Pytf::new(config_yml)?,
            config,
            socket,
            segment_proc,
        })
    }

    /// Send a done message for this job to the main server
    fn send_done(&self) {
        self.socket.do_send(WsMessage(ws::Message::Binary(
            [DONE_HEADER, self.config.name.as_bytes()].concat().into()
        )));
    }

    /// Send a failed message for this job to the main server
    fn send_failed(&self) {
        self.socket.do_send(WsMessage(ws::Message::Binary(
            [FAILED_HEADER, self.config.name.as_bytes()].concat().into()
        )));
    }
}

