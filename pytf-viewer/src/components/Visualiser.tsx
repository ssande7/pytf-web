import { Particles, Visualizer, AtomTypes } from 'omovi'
import { Vector3, Color, Mesh, BufferGeometry, BufferAttribute, MeshBasicMaterial } from 'three'
import React, { useEffect, useState, useRef } from 'react';
import RepeatIcon from '@mui/icons-material/Repeat';
import RepeatOnIcon from '@mui/icons-material/RepeatOn';
import SpeedIcon from '@mui/icons-material/Speed';
import VisibilityOutlinedIcon from '@mui/icons-material/VisibilityOutlined';
import SaveOutlinedIcon from '@mui/icons-material/SaveOutlined';

// Box size in nm, from graphene substrate .pdb file, accounting for rotation mapping x,y,z -> y,z,x.
// TODO: have the server send this info
export const Lz = 4.2600 // x -> z
export const Lx = 3.9352 // y -> x

// Omovi atom types in periodic table order
// Used to retrieve Omovi atom type from element number in data
export const atom_types = [
    AtomTypes.H,
    AtomTypes.H, AtomTypes.He,
    AtomTypes.Li, AtomTypes.Be, AtomTypes.B, AtomTypes.C, AtomTypes.N, AtomTypes.O, AtomTypes.F,
    AtomTypes.Ne, AtomTypes.Na, AtomTypes.Mg, AtomTypes.Al, AtomTypes.Si, AtomTypes.P, AtomTypes.S,
    AtomTypes.Cl, AtomTypes.Ar, AtomTypes.K, AtomTypes.Ca,
  ];

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


interface IVisualiser {
  particles: Array<Particles>,
  num_frames: number,
  height_map: Float32Array | null,
  show_height_map: boolean,
  num_bins: number,
  roughness: number | null,
  mean_height: number | null,
  new_roughness: boolean,
  setNewRoughness: React.Dispatch<React.SetStateAction<boolean>>,
  status_text: string,
}

const Visualiser: React.FC<IVisualiser> = ({
  particles, num_frames, height_map, show_height_map,
  num_bins, roughness, mean_height, new_roughness,
  setNewRoughness, status_text
}) => {
  const [vis, setVis] = useState<Visualizer | null>(null);
  const [loadingVis, setLoadingVis] = useState(false);
  const domElement = useRef<HTMLDivElement | null>(null);
  const camPositionInit = new Vector3( 6.2,  3, -2.5);
  const camTargetInit   = new Vector3( 2,  1.5,  2);
  const [frame, setFrame] = useState(0);
  const [paused, setPaused] = useState(false);
  const [loop, setLoop] = useState(false);

  // Viewport creation
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

  // Display current frame
  const prevParticlesRef = useRef<Particles | null>()
  useEffect(() => {
    prevParticlesRef.current = frame < num_frames ? particles[frame] : null;
  }, [particles, particles.length, frame, num_frames])

  const prevParticles = prevParticlesRef.current
  useEffect(() => {
    if (!vis) { return }
    if (prevParticles && (particles.length === 0 || prevParticles !== particles[frame])) {
      vis.remove(prevParticles)
    }
    if (frame < particles.length) {
      vis.add(particles[frame])
    } else {
      // Reset frame to fix looping when new simulation started
      setFrame(0);
    }
  }, [particles, particles.length, frame, vis])

  // Handle iteration between frames
  const animationSlider = useRef<HTMLInputElement | null>(null);
  const frameRef = useRef(frame);
  const [fps, setFps] = useState(15)
  const [seeking, setSeeking] = useState(false);
  const loopRef = useRef(loop);

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

  // Calculate tiles to display heat map
  const [height_map_disp, setHeightMapDisp] = useState<Array<Mesh>>([]);
  useEffect(() => {
    if (!vis) { return }
    if (height_map_disp.length > 0) {
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

  // Timer to update the frame
  useEffect(() => {
    if (!paused && !seeking && particles.length > 0) {
      const timer = setInterval(() => {
        let new_frame = frameRef.current + 1;
        if (loopRef.current) {
          new_frame = particles.length > 0 ? new_frame % particles.length : 0;
        } else if (new_frame >= particles.length) {
          new_frame = Math.max(particles.length - 1, 0);
        }
        frameRef.current = new_frame;
        setFrame(new_frame);
      }, 1000.0/fps);
      return () => {clearInterval(timer)};
    }
  }, [particles, particles.length, frameRef, setFrame, loopRef, fps, paused, seeking])

  // Update slider based on current frame
  useEffect(() => {
    frameRef.current = frame;
    if (animationSlider.current) {
      animationSlider.current.value = String(frame)
    }
  }, [frame])

  return (<>
    <div className="MD-vis" >
      <div
        style={{
          height: '400pt', minHeight: '200pt', maxHeight: '80vh',
          backgroundColor: '0x333'
        }}
        ref={domElement}>
      </div>
      <div className="MD-vis-controls">
        <div className="icon-button">
          <button className={paused ? "play-button play" : "play-button pause"}
            onClick={() => setPaused(prev => !prev)}
          />
        </div>
        <input type="range" min="0" max={particles.length > 0 ? particles.length-1 : 0} defaultValue='0' ref={animationSlider}
          style={{flexGrow: 8, marginRight: '12pt'}}
          onInput={(e) => {
            setSeeking(true);
            const new_frame = e.currentTarget.valueAsNumber
            frameRef.current = new_frame
            setFrame(new_frame)
            setSeeking(false);
          }}
        />
        <button className="icon-button"
          onClick={toggleLoop}
          title="Toggle playback loop"
        >
          {loop ? <RepeatOnIcon/> : <RepeatIcon/>}
        </button>
        <div title="Playback speed"
          className="icon-button display-only"
          style={{marginLeft: '10pt', marginRight: '2pt'}}
        >
          <SpeedIcon/>
        </div>
        <input type="range" min={5} max={30}
          style={{flexGrow: 4, maxWidth: '10%', marginRight: '12pt'}}
          defaultValue={fps}
          onChange={(e) => {
            if (e.target.value) {
              setFps(e.target.valueAsNumber)
            }
          }}
        />
        <button className="icon-button"
          title="Reset camera to initial position"
          onClick={resetCamera}
        >
          <VisibilityOutlinedIcon/>
        </button>
        <button className="icon-button"
          title="Save deposition movie"
          style={{marginLeft: '6pt'}}
        >
          <SaveOutlinedIcon/>
        </button>
      </div>
    </div>
    <div style={{color: 'white'}}>
      <b>Status: </b>{status_text}
    </div>
  </>);
}

export default Visualiser
