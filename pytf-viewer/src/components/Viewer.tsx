import React, { useEffect, useState, useRef, useMemo, MutableRefObject } from 'react';
import { Particles, Visualizer, AtomTypes } from 'omovi'
import { Vector3, Color, LineLoop, Mesh, BufferGeometry, BufferAttribute, MeshBasicMaterial } from 'three'
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
import CollapseIndicator from './CollapseIndicator';

// Box size in nm, from graphene substrate .pdb file, accounting for rotation mapping x,y,z -> y,z,x.
// TODO: have the server send this info
const Lz = 4.2600 // x -> z
const Lx = 3.9352 // y -> x

function heatMapColor(value: number){
  var h = (1.0 - value) * 240. / 360
  return hsl2Color(h, 1, 0.5);
}
function hue2rgb(p: number, q: number, t: number) {
    if (t < 0) {
        t += 1;
    } else if (t > 1) {
        t -= 1;
    }

    if (t >= 0.66) {
        return p;
    } else if (t >= 0.5) {
        return p + (q - p) * (0.66 - t) * 6;
    } else if (t >= 0.33) {
        return q;
    } else {
        return p + (q - p) * 6 * t;
    }
};
function hsl2Color (h: number, s: number, l: number) {
    var r, g, b, q: number, p: number;
    if (s === 0) {
        r = g = b = l;
    } else {
        q = l < 0.5 ? l * (1 + s) : l + s - l * s;
        p = 2 * l - q;
        r = hue2rgb(p, q, h + 0.33);
        g = hue2rgb(p, q, h);
        b = hue2rgb(p, q, h - 0.33);
    }

    return new Color(r, g, b); // (x << 0) = Math.floor(x)
};

const atom_types = [
    AtomTypes.H,
    AtomTypes.H, AtomTypes.He,
    AtomTypes.Li, AtomTypes.Be, AtomTypes.B, AtomTypes.C, AtomTypes.N, AtomTypes.O, AtomTypes.F,
    AtomTypes.Ne, AtomTypes.Na, AtomTypes.Mg, AtomTypes.Al, AtomTypes.Si, AtomTypes.P, AtomTypes.S,
    AtomTypes.Cl, AtomTypes.Ar, AtomTypes.K, AtomTypes.Ca,
  ];

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
  const [config, setConfig] = useState<PytfConfig>({deposition_velocity: 0.35, mixture: []});
  const [submit_waiting, setSubmitWaiting] = useState(false);
  const [protocol_visible, setProtocolVisible] = useState(true);

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
      <MolList
        running={running}
        molecules={molecules}
        config={config} setConfig={setConfig}
      />
      <div className="collapsible"
        onClick={() => setProtocolVisible((prev) => !prev)}
      >
        <b>Protocol</b>
        <CollapseIndicator visible={protocol_visible} />
      </div>
      <div className="collapsible-content"
        style={{ display: protocol_visible ? "flex" : "none" }}
      >
          <div className="flex-row">
            <div style={{marginRight: 'auto'}}>Deposition velocity:</div>
            <div>{config.deposition_velocity} nm/ps</div>
          </div>
          <input type="range"
            min={10} max={100}
            defaultValue={config.deposition_velocity*100}
            disabled={running}
            onChange = {
              (e) => setConfig({
                ...config,
                deposition_velocity: e.target.valueAsNumber/100.0
              })
            }
          />
      </div>
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
  height_map: Float32Array | null,
  show_height_map: boolean,
  num_bins: number,
  roughness: number | null,
  mean_height: number | null,
  new_roughness: boolean,
  setNewRoughness: React.Dispatch<React.SetStateAction<boolean>>,
}

const Vis: React.FC<IVis> = ({socket, running, particles, num_frames, height_map, show_height_map, num_bins, roughness, mean_height, new_roughness, setNewRoughness }) => {
  const [vis, setVis] = useState<Visualizer | null>(null);
  const [loadingVis, setLoadingVis] = useState(false);
  const domElement = useRef<HTMLDivElement | null>(null);
  const camPositionInit = new Vector3( 6.2,  3, -2.5);
  const camTargetInit   = new Vector3( 2,  1.5,  2);
  const [frame, setFrame] = useState(0);
  const [paused, setPaused] = useState(false);
  const [loop, setLoop] = useState(false);

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
  }, [particles, particles.length, frame, num_frames])

  const prevParticles = prevParticlesRef.current
  useEffect(() => {
    if (!vis) { return }
    console.log("Particles cleanup check triggered");
    if (prevParticles && (particles.length === 0 || prevParticles !== particles[frame])) {
      console.log("Cleaning up particles");
      vis.remove(prevParticles)
    }
    if (frame < particles.length) {
      vis.add(particles[frame])
    } else {
      // Reset frame to fix looping when new simulation started
      setFrame(0);
    }
  }, [particles, particles.length, prevParticles, frame, vis])

  const animationSlider = useRef<HTMLInputElement | null>(null);
  const frameRef = useRef(frame);
  const [animationTimer, setAnimationTimer] = useState<NodeJS.Timer | null>(null);
  const [fps, setFps] = useState(15)
  const loopRef = useRef(loop);

  useEffect(() => {
    console.log("Triggered frame update");
    frameRef.current = frame;
    if (animationSlider.current) {
      animationSlider.current.value = String(frame)
    }
  }, [frame])

  function startAnimation() {
    setAnimationTimer(setInterval(() => {
      var new_frame = frameRef.current + 1;
      if (loopRef.current) {
        new_frame = particles.length > 0 ? new_frame % particles.length : 0;
      } else if (new_frame >= particles.length) {
        return
      }
      frameRef.current = new_frame
      setFrame(new_frame)
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

  const [height_map_disp, setHeightMapDisp] = useState<Array<Mesh>>([]);

  // Calculate tiles to display heat map
  useEffect(() => {
    if (!vis) { return }
    if (height_map_disp.length > 0) {
      console.log("Cleaning up height map");
      for (var tile = 0; tile < height_map_disp.length; tile++) {
        vis.scene.remove(height_map_disp[tile])
      }
    }
    height_map_disp.length = 0;
    if (show_height_map && height_map !== null && mean_height !== null && roughness !== null) {
      const vertices = new Float32Array([
        0, 0, 0,
        Lx/num_bins, 0, 0,
        Lx/num_bins, 0, Lz/num_bins,
        0, 0, Lz/num_bins,
      ]);
      const indices = [
        0,1,2,
        2,3,0,
        0,2,1,
        3,2,0,
      ];
      const square = new BufferGeometry();
      square.setIndex(indices);
      square.setAttribute('position', new BufferAttribute(vertices, 3));
      loopRef.current = false;
      for (var x = 0; x < num_bins; x++) {
        for (var z = 0; z < num_bins; z++) {
          // Colour based on height relative to mean.
          // Min. value at -1.5 std. dev, max at +1.5
          const y = height_map[x*num_bins+z];
          var col = roughness > 0 ? (y - mean_height) / (1.5*roughness) : 0;
          if (col < -1) { col = -1 }
          if (col >  1) { col =  1 }
          col = (col + 1)/2;
          const material = new MeshBasicMaterial({ color: heatMapColor(col) });
          const tile = new Mesh(square, material);
          tile.translateX(x * Lx / num_bins);
          tile.translateZ(z * Lz / num_bins);
          tile.translateY(y);
          height_map_disp.push(tile);
          vis.scene.add(height_map_disp[x*num_bins+z])
        }
      }
    }
    setHeightMapDisp(height_map_disp);

    // Deliberately not reacting on num_bins change
  }, [height_map, show_height_map, height_map_disp, mean_height, roughness, setHeightMapDisp, particles, setFrame, setLoop]);

  // Jump to final frame and disable looping
  // if we just calculated a height map
  useEffect(() => {
    if (!new_roughness) return;
    setNewRoughness(false);
    if (show_height_map) {
      setLoop(false);
      loopRef.current = false;
      const final_frame = particles.length - 1;
      frameRef.current = final_frame;
      setFrame(final_frame);
    }
  }, [new_roughness, setNewRoughness]);

  return (
    <div className="MD-vis" >
      <div
        style={{
          height: '400pt', minHeight: '200pt', maxHeight: '80vh',
          backgroundColor: '0x606160'
        }}
        ref={domElement}>
      </div>
      <div id="controls" className="MD-vis-controls"
        style={{width: '100%', padding: 0}}>
        <div style={{padding: '12pt', display: 'flex', flexDirection: 'column', alignContent: 'middle', height: '16pt'}}>
          <button className={paused ? "PlayButton play" : "PlayButton pause"} onClick={toggleAnimation} />
        </div>
        <input type="range" min="0" max={particles.length > 0 ? particles.length-1 : 0} defaultValue='0' ref={animationSlider}
          style={{verticalAlign: 'middle', flexGrow: 8}}
          onInput={(e) => {
            if (!paused) {stopAnimation()}
            const new_frame = e.currentTarget.valueAsNumber
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
          <div title="Playback speed"
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
  );
}

interface IRoughness {
  particles: Particles | null,
  num_bins: number,
  setNumBins: React.Dispatch<React.SetStateAction<number>>,
  roughness: number | null,
  setRoughness: React.Dispatch<React.SetStateAction<number | null>>,
  mean_height: number | null,
  setMeanHeight: React.Dispatch<React.SetStateAction<number | null>>,
  setHeightMap: React.Dispatch<React.SetStateAction<Float32Array | null>>,
  setShowHeightMap: React.Dispatch<React.SetStateAction<boolean>>,
  setNewRoughness: React.Dispatch<React.SetStateAction<boolean>>,
}
const Roughness: React.FC<IRoughness> = ({ particles, num_bins, setNumBins, roughness, setRoughness, mean_height, setMeanHeight, setHeightMap, setShowHeightMap, setNewRoughness }) => {
  const [min_height, setMinHeight] = useState<number>(0);
  function calcRoughness() {
    if (particles === null) {
      setHeightMap(null);
      setMeanHeight(null);
      setMinHeight(0);
      setRoughness(null);
      return;
    }
    console.log("Calculating roughness");
    const bins_sq = num_bins * num_bins;
    var height_map = new Float32Array(bins_sq).fill(0);
    var min_ht = Number.MAX_SAFE_INTEGER;
    var i, mean_y = 0;
    for (i = 0; i < particles.count; i++) {
      mean_y += particles.getPosition(i).y;
    }
    mean_y /= particles.count;
    for (i = 0; i < particles.count; i++) {
      const pos = particles.getPosition(i);
      if (pos.y > mean_y*5) { continue } // Skip atoms that are obviously in gas phase
      const rad = atom_types[particles.getType(i)].radius / 10;
      var bx = Math.floor(pos.x * num_bins / Lx);
      var bz = Math.floor(pos.z * num_bins / Lz);
      while (bx < 0) { bx += num_bins }
      while (bx >= num_bins) { bx -= num_bins }
      while (bz < 0) { bz += num_bins }
      while (bz >= num_bins) { bz -= num_bins }
      const idx = bx*num_bins + bz;
      // Use atom position + radius for better heat map tile placement
      const ht = pos.y + rad;
      if (height_map[idx] < ht) { height_map[idx] = ht }
      if (ht < min_ht) { min_ht = ht }
    }
    const mean_height = height_map.reduce((a, b) => a + b) / bins_sq;
    const sqvar = height_map
      .map(h => (h - mean_height)*(h - mean_height))
      .reduce((a, b) => a + b);
    const roughness = Math.sqrt(sqvar / bins_sq);
    console.log("Height map is: ", height_map);
    setHeightMap(height_map);
    setMeanHeight(mean_height);
    setMinHeight(min_ht);
    setRoughness(roughness);
    setNewRoughness(true);
  }

  return (<div className="MD-param-group">
    <div className="collapsible no-hover">
      <b>Roughness</b>
    </div>
    <div className="collapsible-content">
      <div className="flex-row">
        <div style={{marginRight: 'auto'}}>Bins per side:</div>
        <div>{num_bins}</div>
      </div>
      <input type="range" min={1} max={20} defaultValue={10}
        onChange = {
          (e) => setNumBins(e.target.valueAsNumber)
        }
      />
      <br/>
      <div className="flex-row" style={{marginTop: '10pt'}}>
        <div style={{marginRight: 'auto'}}>Show height map:</div>
        <label className="toggle-slider">
          <input type="checkbox" defaultChecked={true}
            onChange = {(e) => setShowHeightMap(e.target.checked)}
          />
          <span className="slider"></span>
        </label>
      </div>
      <div className="flex-row" style={{marginTop: '20pt'}}>
        <div style={{marginRight: 'auto'}}>Mean film thickness:</div>
        <div>{mean_height !== null ? (mean_height - min_height).toFixed(3) + ' nm' : '???'}</div>
      </div>
      <div className="flex-row">
        <div style={{marginRight: 'auto'}}>Roughness:</div>
        <div>{roughness !== null ? roughness.toFixed(3) + ' nm' : '???'}</div>
      </div>
    </div>
    <button className="submit-button roughness"
      onClick={calcRoughness}
    >
      <b>Calculate Roughness</b>
    </button>
  </div>);
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
  const [sim_done, setSimDone] = useState<boolean>(false);

  const [particles_roughness, setParticlesRoughness] = useState<Particles | null>(null);
  const [roughness_ready, setRoughnessReady] = useState<boolean>(false);
  const [num_bins, setNumBins] = useState<number>(10); // bin size in nm
  const [roughness, setRoughness] = useState<number | null>(null);
  const [mean_height, setMeanHeight] = useState<number | null>(null);
  const [height_map, setHeightMap] = useState<Float32Array | null>(null);
  const [show_height_map, setShowHeightMap] = useState(true);
  const [new_roughness, setNewRoughness] = useState(false);

  const [current_tab, setCurrentTab] = useState(0);

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
          // Wait 0.25s before requesting more frames to avoid laggy rendering from
          // constant refreshes of `particles`
          setTimeout(() => {
            socket.current?.send((segment_id + 1).toString());
            console.log("Requested segment ", segment_id+1);
          }, 250);
        } else {
          setWaitForSegment(false);
          if (sim_done) {
            setRunning(false);
            setRoughnessReady(true);
            setParticlesRoughness(particles[particles.length-1]);
            setCurrentTab(1);
          }
        }
      }).catch(console.error);

    } else if (last_message.data.startsWith("new_frames") || last_message.data.startsWith("done")) {
      const done = last_message.data.startsWith("done");
      console.log("Received trajectory ping: ", last_message.data);
      const latest_segment = Number.parseInt(last_message.data.slice(done ? 4 : 10));
      if (done) { setSimDone(true); }
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
      next_segment, setNextSegment,
      particles, sim_done,
    ]);

  const tabs = [
    {
      name: "Simulation",
      enable: true,
      content:
        <Composition
          socket={socket} socket_connected={socket_connected}
          running={running} setRunning={setRunning}
          resetTrajectory={() => {
            console.log("Resetting trajectory");
            setSimDone(false);
            setNextSegment(1);
            setLatestSegment(0);
            setLastFrame(0);
            setWaitForSegment(false);
            particles.map((p) => p.dispose());
            particles.length = 0;
            setParticles(particles);
            setRoughness(null);
            setMeanHeight(null);
            setHeightMap(null);
            setRoughnessReady(false);
          }}
        />
      },
      {
        name: "Roughness",
        enable: roughness_ready && particles_roughness,
        content:
          <Roughness
            particles={particles_roughness}
            num_bins={num_bins} setNumBins={setNumBins}
            roughness={roughness} setRoughness={setRoughness}
            mean_height={mean_height} setMeanHeight={setMeanHeight}
            setHeightMap={setHeightMap} setShowHeightMap={setShowHeightMap}
            setNewRoughness={setNewRoughness}
          />
    },
    {
      name: "Help",
      enable: true,
      content: "TODO"
    },
  ];

  return (
    <>
      <div className="App">
        <div className="App-header">
          <div className="header-text">
            <b>Vacuum Deposition</b>
          </div>
          <div className="header-button-container">
            <div className="header-button"
                onClick={() => {
                  logout({ token });
                  setToken(null);
                }}
              >
                Sign Out ({JSON.parse(token).token})
              </div>
          </div>
        </div>
        <div className="view-container">
          <div className="tab-container">
            <div className="tab-buttons">
              { tabs.map((tab, i) => { return (
                <button className={"tab-button" +
                  (i === current_tab ? " tab-button-selected" : "")}
                  onClick={() => setCurrentTab(i)}
                  disabled={!tab.enable}
                >
                  <b>{tab.name}</b>
                </button>)
              })}
            </div>
            { tabs.length === 0 ? null : tabs.map((tab, i) => {
              return <div className="MD-params"
                style={{display: i === current_tab ? 'flex' : 'none' }}
              >
                {tab.content}
              </div>
              })
            }
          </div>
          <div className="vis-container">
            <Vis
              socket={socket} running={running}
              particles={particles} num_frames={last_frame}
              height_map={height_map} show_height_map={show_height_map}
              num_bins={num_bins} mean_height={mean_height}
              roughness={roughness} new_roughness={new_roughness}
              setNewRoughness={setNewRoughness}
            />
          </div>
        </div>
      </div>
    </>
  );
}

export default Viewer;
