import React, { useEffect, useState, useRef, useMemo, MutableRefObject } from 'react';
import { Particles, Visualizer, AtomTypes } from 'omovi'
import { Vector3, Color } from 'three'
import RepeatIcon from '@mui/icons-material/Repeat';
import RepeatOnIcon from '@mui/icons-material/RepeatOn';
import SpeedIcon from '@mui/icons-material/Speed';
import VisibilityOutlinedIcon from '@mui/icons-material/VisibilityOutlined';
import SaveOutlinedIcon from '@mui/icons-material/SaveOutlined';
import { logout } from './Login'
import { PytfConfig, MixtureComponentDetailed } from './types';
import MolList from './MolList';
import SubmitButton from './SubmitButton';
import '../App.css';

interface IComposition {
  socket: React.MutableRefObject<WebSocket | null>,
  socket_connected: boolean,
  running: boolean,
  setRunning: React.Dispatch<React.SetStateAction<boolean>>,
  resetTrajectory: () => void,
}

const Composition: React.FC<IComposition>
  = ({socket, socket_connected, running, setRunning, resetTrajectory}) =>
{
  const [molecules, setMolecules] = useState<Array<MixtureComponentDetailed>>([]);
  const [config, setConfig] = useState<PytfConfig>({deposition_velocity: 0.1, mixture: []});
  const [submit_waiting, setSubmitWaiting] = useState(false);

  // Get the list of available molecules on load
  useEffect(() => {
    let abort = new AbortController();
    const fetchMolecules = async () => {
      const mols: {molecules: Array<MixtureComponentDetailed>} =
        await fetch("/molecules", abort).then(data => data.json());
      console.log("Got molecules: " + JSON.stringify(mols))
      setMolecules(mols.molecules)
    };
    fetchMolecules();
    return () => abort.abort();
  }, [setMolecules]);

  // Set up base config with everything zeroed
  useEffect(() => {
    setConfig((config) => {return {
      ...config,
      mixture: molecules.map((mol) => {return {res_name: mol.res_name, ratio: 0}}),
    }});
  }, [molecules, setConfig]);

  return (
    <div className="MD-param-group">
      <h3>Composition</h3>
      <MolList
        running={running}
        molecules={molecules}
        config={config} setConfig={setConfig}
      />
      <h3>Protocol</h3>
      <div className="MD-vis-controls">
        <div>Deposition velocity:</div>
        <div className="HorizontalSpacer"/>
        <div>{config.deposition_velocity} nm/ps</div>
      </div>
      <input type="range" min={1} max={50} defaultValue={10}
        disabled={running}
        onChange = {
          (e) => setConfig({
            ...config,
            deposition_velocity: e.target.valueAsNumber/100.0
          })
        }
      />
      <p/>
      <SubmitButton
        socket={socket} socket_connected={socket_connected}
        config={config}
        running={running} setRunning={setRunning}
        waiting={submit_waiting} setWaiting={setSubmitWaiting}
        resetTrajectory={resetTrajectory}
      />
    </div>
  );
}

interface IVis {
  socket: React.MutableRefObject<WebSocket | null>,
  running: boolean,
  particles: Array<Particles>,
  num_frames: number,
}

const Vis: React.FC<IVis> = ({socket, running, particles, num_frames }) => {
  const [vis, setVis] = useState<Visualizer | null>(null);
  const [loadingVis, setLoadingVis] = useState(false);
  const domElement = useRef<HTMLDivElement | null>(null);
  const camPositionInit = new Vector3( 6.2,  3, -2.5);
  const camTargetInit   = new Vector3( 2,  1.5,  2);
  const [frame, setFrame] = useState(0);
  const [paused, setPaused] = useState(false);
  const [loop, setLoop] = useState(false);
  // probably need a useRef to work with animation timer

  const atom_types = [
      AtomTypes.H,
      AtomTypes.H,
      AtomTypes.He,
      AtomTypes.Li,
      AtomTypes.Be,
      AtomTypes.B,
      AtomTypes.C,
      AtomTypes.N,
      AtomTypes.O,
      AtomTypes.F,
      AtomTypes.Ne,
      AtomTypes.Na,
      AtomTypes.Mg,
      AtomTypes.Al,
      AtomTypes.Si,
      AtomTypes.P,
      AtomTypes.S,
      AtomTypes.Cl,
      AtomTypes.Ar,
      AtomTypes.K,
      AtomTypes.Ca,
    ];

  useEffect(() => {
    if (domElement.current && !loadingVis && !vis) {
      setLoadingVis(true);
      const new_vis = new Visualizer({
        domElement: domElement.current,
        initialColors: atom_types.map((atom) => atom.color),
      })
      atom_types.map((atom, idx) => new_vis.setRadius(idx, atom.radius/10.));
      new_vis.materials.particles.shininess = 50
      // new_vis.ambientLight.color = new Color(0x596164);
      // new_vis.ambientLight.intensity = 0.5
      new_vis.pointLight.intensity = 0.7
      new_vis.scene.background = new Color(0x606160);
      new_vis.setCameraPosition(camPositionInit);
      new_vis.setCameraTarget(camTargetInit);
      setVis(new_vis)
      setLoadingVis(false);
    }
    return () => {
      if (vis) {
        vis.dispose()
      }
    }
  }, [vis, domElement, loadingVis])

  const prevParticlesRef = useRef<Particles | null>()
  useEffect(() => {
    prevParticlesRef.current = frame < num_frames ? particles[frame] : null;
  }, [particles, frame, num_frames])

  const prevParticles = prevParticlesRef.current
  useEffect(() => {
    if (!vis) { return }
    if (prevParticles && prevParticles !== particles[frame]) {
      vis.remove(prevParticles)
      prevParticles.dispose()
    }
    if (frame < particles.length) {
      vis.add(particles[frame])
    }
  }, [particles, prevParticles, frame, vis])

  const animationSlider = useRef<HTMLInputElement | null>(null);
  const frameRef = useRef(frame);
  const [animationTimer, setAnimationTimer] = useState<NodeJS.Timer | null>(null);
  const [fps, setFps] = useState(15)
  const loopRef = useRef(loop);

  function startAnimation() {
    setAnimationTimer(setInterval(() => {
      var new_frame = frameRef.current + 1;
      if (loopRef.current) {
        new_frame = particles.length > 0 ? new_frame % particles.length : 0;
        frameRef.current = new_frame
        setFrame(new_frame)
      } else if (new_frame < particles.length) {
        frameRef.current = new_frame
        setFrame(new_frame);
      } else {
        return
      }
      if (animationSlider.current) {
        animationSlider.current.value = String(new_frame)
      }
    }, 1000.0/fps))
  }


  function stopAnimation() {
    if (animationTimer) {
      clearInterval(animationTimer)
      setAnimationTimer(null)
    }
  }

  function restartAnimation() {
    if (animationTimer) {
      clearInterval(animationTimer)
      startAnimation()
    }
  }

  function toggleAnimation() {
    if (paused) {
      startAnimation()
    } else {
      stopAnimation()
    }
    setPaused(!paused)
  }

  useEffect(restartAnimation, [particles.length, fps])

  function resetCamera() {
    if (vis && camTargetInit && camPositionInit) {
      vis.setCameraTarget(camTargetInit)
      vis.setCameraPosition(camPositionInit)
    }
  }

  function toggleLoop() {
    setLoop((loop) => !loop);
  }
  useEffect(() => {
    loopRef.current = loop
  }, [loop]);

  useEffect(() => {
    if (!paused) startAnimation();
  }, []);

  return (
    <>
      <div id="canvas-container" style={{ height: '100%', width: '100%'}}>
        <div
          style={{
            height: '400pt', minHeight: '200pt', maxHeight: '80vh',
            width: '100%', border: 'medium solid grey', backgroundColor: '0x606160'
          }}
          ref={domElement}>
        </div>
        <div id="controls" className="MD-vis-controls" style={{width: '100%', padding: 0}}>
          <div style={{padding: '12pt', display: 'flex', flexDirection: 'column', alignContent: 'middle', height: '16pt'}}>
            <button className={paused ? "PlayButton play" : "PlayButton pause"} onClick={toggleAnimation} />
          </div>
          <input type="range" min="0" max={particles.length > 0 ? particles.length-1 : 0} defaultValue='0' ref={animationSlider}
            style={{verticalAlign: 'middle', flexGrow: 8}}
            onInput={(e) => {
              if (!paused) {stopAnimation()}
              const new_frame = Number(e.currentTarget.value)
              frameRef.current = new_frame
              setFrame(new_frame)
              if (!paused) {startAnimation()}
            }}
          />
            <div className="HorizontalSpacer" style={{minWidth: '12pt', maxWidth: '12pt'}}/>
          <div className="MD-vis-controls" style={{flexGrow: 1, maxWidth: '15%', fontSize: '16pt'}}>
            <button className="App-button" style={{fontSize: '16pt'}} onClick={toggleLoop} title="Toggle playback loop">
              {loop ? <RepeatOnIcon/> : <RepeatIcon/>}
            </button>
            <div className="HorizontalSpacer" style={{minWidth: '5pt', maxWidth: '5pt'}}/>
            <div title="Animation speed"
              className="VertCenteredIcon"
              style={{cursor: 'default'}}
            >
              <SpeedIcon/>
            </div>
            <div className="HorizontalSpacer" style={{minWidth: '5pt'}}/>
            <input type="range" min={5} max={30}
              style={{flexGrow: 4, maxWidth: '80%', verticalAlign: 'middle'}}
              defaultValue={fps}
              onChange={(e) => {
                if (e.target.value) {
                  setFps(e.target.valueAsNumber)
                }
              }}
            />
          </div>
          <div className="HorizontalSpacer" />
          <button className="App-button" style={{maxWidth: '16pt'}}
            onClick={resetCamera} title="Reset camera to initial position">
            <VisibilityOutlinedIcon/>
          </button>
          <div className="HorizontalSpacer" style={{maxWidth: '5pt'}}/>
          <button className="App-button" style={{maxWidth: '16pt'}} title="Save deposition movie">
            <SaveOutlinedIcon/>
          </button>
        </div>
      </div>
    </>
  );
}

interface IViewer {
  token: string;
  setToken: React.Dispatch<React.SetStateAction<string | null>>;
}
const Viewer: React.FC<IViewer> = ({ token, setToken }) => {
  const [running, setRunning] = useState(false);
  const socket = useRef<WebSocket | null>(null);
  const [socket_connected, setSocketConnected] = useState(false);
  const [last_message, setLastMessage] = useState<MessageEvent<any> | null>(null);

  const [last_frame, setLastFrame] = useState(0);
  const [next_segment, setNextSegment] = useState(1);
  const [particles, setParticles] = useState<Array<Particles>>([]);
  const [wait_for_segment, setWaitForSegment] = useState<boolean>(false);
  const [latest_segment, setLatestSegment] = useState<number>(0);

  useEffect(() => {
    let ws_url = window.location.href.replace(new RegExp("^http"), "ws");
    if (!ws_url.endsWith("/")) {
      ws_url += "/"
    }

    socket.current = new WebSocket(ws_url + "socket");
    console.log("Socket opened.");
    socket.current.onopen = () => setSocketConnected(true);
    socket.current.onclose = () => {
      console.log("socket closed");
      setSocketConnected(false)
    };
    socket.current.onmessage = (e) => setLastMessage(e);
    const current = socket.current;
    return () => {
      current.close();
    }
  }, []);

  // Process web socket messages
  useEffect(() => {
    if (last_message === null) return;
    setLastMessage(null);
    if (!running) {
      console.log("Unexpected message while not running");
      return;
    }
    if (last_message.data instanceof Blob) {
      if (!wait_for_segment) {
        console.log("Received segment while not waiting for one.");
        return;
      }
      last_message.data.arrayBuffer().then((buf) => {
        const buffer = new DataView(buf);
        const segment_id = buffer.getUint32(0, true);
        if (segment_id !== next_segment) {
          console.log("Expecting segment ", next_segment, ", but received ", segment_id);
          return
        }
        const num_frames    = buffer.getUint32(4, true);
        const num_particles = buffer.getUint32(8, true);
        console.log("Got new segment:\n\tid: ", segment_id, "\n\tframes: ", num_frames, "\n\tparticles: ", num_particles);
        const types = new Uint8Array(buffer.buffer, 12, num_particles);
        var offset = 12 + num_particles;
        for (let i = 0; i < num_frames; i++) {
          const frame = new Particles(num_particles)
          for (let j = 0; j < num_particles; j += 1) {
            // 12 bytes per particle position
            // Rotate x,y,z -> y,z,x since THREE wants y to be up by default
            frame.add(
              buffer.getFloat32(offset + j*12 + 4, true),
              buffer.getFloat32(offset + j*12 + 8, true),
              buffer.getFloat32(offset + j*12, true),
              types[j],
              types[j]
              )
          }
          offset += 12*num_particles;
          particles.push(frame);
        }
        console.log("Particles now contains ", particles.length, " frames.");
        setLastFrame((last_frame) => last_frame + num_frames);
        setParticles(particles);
        setNextSegment(segment_id + 1);
        if (segment_id < latest_segment && socket.current) {
          console.log("Done processing. Requesting next segment: ", segment_id + 1);
          // Wait 0.5s before requesting more frames to avoid laggy rendering from
          // constant refreshes of `particles`
          setTimeout(() => {
            socket.current?.send((segment_id + 1).toString());
            console.log("Requested segment ", segment_id+1);
          }, 500);
        } else {
          setWaitForSegment(false);
        }
      }).catch(console.error);

    } else if (last_message.data.startsWith("new_frames")) {
      console.log("Received trajectory ping: ", last_message.data);
      const latest_segment = Number.parseInt(last_message.data.slice(10));
      setLatestSegment((prev) => latest_segment > prev ? latest_segment : prev);
      setWaitForSegment((waiting) => {
        if (!waiting && latest_segment >= next_segment && socket.current) {
          console.log("Requesting segment ", next_segment);
          socket.current.send(next_segment.toString());
          return true
        } else { console.log("Already waiting, skipping request for ", next_segment) }
        return waiting
      })

    } else if (last_message.data.startsWith("no_seg")) {
      const seg = Number.parseInt(last_message.data.slice(6));
      console.log("Segment not available yet: ", seg);
      setWaitForSegment((waiting) => seg === next_segment ? false : waiting);

    } else if (last_message.data === "done") {
      // TODO: Enable histogram roughness analysis
      console.log("Job is finished");
      setRunning(false);
      // TODO: set flag for all segments available,
      // setRunning(false) when segment_id matches latest_segment
      //
      // setWaitForSegment(false);

    } else if (last_message.data === "failed") {
      // TODO: Show message about job failure
      console.log("Job failed!");
      setRunning(false);
      // setWaitForSegment(false);
    } else {
      console.log("Got unknown message: ", last_message.data);
    }
  }, [last_message, setLastMessage,
      running, setRunning,
      wait_for_segment, setWaitForSegment,
      latest_segment, setLatestSegment,
      setLastFrame, setParticles,
      next_segment, setNextSegment]);

  return (
    <>
      <div className="App">
        <div className="MD-container">
          <div className="MD-params" id="input-container">
            <div className="App-header">
              <h1>Vacuum Deposition</h1>
            </div>
            <div style={{display: 'grid', width: '100%', alignItems: 'left'}}>
              <Composition
                socket={socket} socket_connected={socket_connected}
                running={running} setRunning={setRunning}
                resetTrajectory={() => {
                  console.log("Resetting trajectory");
                  setNextSegment(1);
                  setLatestSegment(0);
                  setWaitForSegment(false);
                  particles.length = 0;
                  setParticles(particles);
                }}
              />
            </div>
          </div>
          <div className="MD-vis" >
            <Vis
              socket={socket} running={running}
              particles={particles} num_frames={last_frame}
            />
          </div>
        </div>
        <div style={{
          display: 'flex',
          flexDirection: 'row-reverse'
        }}>
          <button className="App-button"
            style={{
              paddingRight: '5pt',
              display: 'inline-block',
              flexGrow: 0,
            }}
            onClick={() => {
              logout({ token });
              setToken(null);
            }}
          >
            Sign Out ({JSON.parse(token).token})
          </button>
        </div>
      </div>
    </>
  );
}

export default Viewer;
