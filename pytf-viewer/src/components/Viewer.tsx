import React, { useEffect, useState, useRef, useMemo } from 'react';
import { Particles, Visualizer } from 'omovi'
import { Vector3 } from 'three'
import { logout } from './Login'
import { PytfConfig, MixtureComponentDetailed } from './types';
import MolList from './MolList';
import SubmitButton from './SubmitButton';
import '../App.css';

const TimerSymbol = <>&#x23F1;</>;
const SaveSymbol = <>&#x1F5AA;</>;
const ResetCameraSymbol = <>&#x1F441;</>;

interface IComposition {
  running: boolean,
  setRunning: React.Dispatch<React.SetStateAction<boolean>>,
}

const Composition: React.FC<IComposition>
  = ({running, setRunning}: IComposition) =>
{
  const [molecules, setMolecules] = useState<Array<MixtureComponentDetailed>>([]);
  const [config, setConfig] = useState<PytfConfig>({deposition_velocity: 0.1, mixture: []});

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
    setConfig({
      ...config,
      mixture: molecules.map((mol) => {return {res_name: mol.res_name, ratio: 0}}),
    });
  }, [molecules, setConfig]);

  return (
    <div className="MD-param-group">
      <h3>Composition</h3>
      <MolList running={running} molecules={molecules} config={config} setConfig={setConfig}/>
      <h3>Protocol</h3>
      <div className="MD-vis-controls">
        <div>Deposition velocity:</div>
        <div className="HorizontalSpacer"/>
        <div>{config.deposition_velocity} nm/ps</div>
      </div>
      <input type="range" min={1} max={50} defaultValue={10}
        onChange = {
          (e) => setConfig({
            ...config,
            deposition_velocity: e.target.valueAsNumber/100.0
          })
        }
      />
      <p/>
      <SubmitButton config = {config} running = {running} setRunning = {setRunning}/>
    </div>
  );
}

interface IVis {
  numParticles: number;
}

const Vis: React.FC<IVis> = ({ numParticles }) => {
  const [vis, setVis] = useState<Visualizer | null>(null);
  const [loadingVis, setLoadingVis] = useState(false);
  const domElement = useRef<HTMLDivElement | null>(null);
  const [camTargetInit, setCamTargetInit] = useState<Vector3 | null>(null);
  const [camPositionInit, setCamPositionInit] = useState<Vector3 | null>(null);
  const [frame, setFrame] = useState(0);
  const [paused, setPaused] = useState(true);

  const particles = useMemo(() => {
    const new_particles = [];
    for (let f = 0; f < 100; f++) {
      let pframe = new Particles(numParticles)
      for (let i = 0; i < numParticles; i++) {
        pframe.add(
          120 * (Math.random() - 0.5),
          120 * (Math.random() - 0.5),
          120 * (Math.random() - 0.5),
          i,
          1
        )
      }
      new_particles.push(pframe)
    }
    return new_particles;
  }, [numParticles])

  useEffect(() => {
    if (domElement.current && !loadingVis && !vis) {
      setLoadingVis(true);
      const new_vis = new Visualizer({
        domElement: domElement.current
      })
      setCamTargetInit(new_vis.getCameraTarget())
      setCamPositionInit(new_vis.getCameraPosition())
      setVis(new_vis)
      setLoadingVis(false);
    }
    return () => {
      if (vis) {
        vis.dispose()
      }
    }
  }, [vis, domElement, loadingVis])

  const prevParticlesRef = useRef<Particles>()
  useEffect(() => {
    prevParticlesRef.current = particles[frame]
  })
  const prevParticles = prevParticlesRef.current

  useEffect(() => {
    if (!vis) { return }
    if (prevParticles && prevParticles !== particles[frame]) {
      vis.remove(prevParticles)
      prevParticles.dispose()
    }
    if (particles) {
      vis.add(particles[frame])
    }
  }, [particles, prevParticles, frame, vis])

  const animationSlider = useRef<HTMLInputElement | null>(null);
  const frameRef = useRef(frame);
  const [animationTimer, setAnimationTimer] = useState<NodeJS.Timer | null>(null);
  const [fps, setFps] = useState(10)

  function startAnimation() {
    setAnimationTimer(setInterval(() => {
      const new_frame = (frameRef.current + 1) % particles.length
      frameRef.current = new_frame
      setFrame(new_frame)
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

  return (
    <>
      <div id="canvas-container" style={{ height: '100%', width: '100%'}}>
        <div style={{ height: '70vh', width: '100%', border: 'medium solid grey', backgroundColor: 'black'}} ref={domElement}>
        </div>
        <div id="controls" className="MD-vis-controls" style={{width: '100%', padding: 0}}>
          <div style={{padding: '0.2vh', display: 'flex', flexDirection: 'column', alignContent: 'middle', height: '3vh'}}>
            <button className={paused ? "PlayButton play" : "PlayButton pause"} onClick={toggleAnimation} />
          </div>
          <input type="range" min="0" max={particles.length-1} defaultValue='0' ref={animationSlider}
            style={{verticalAlign: 'middle', flexGrow: 8}}
            onInput={(e) => {
              if (!paused) {stopAnimation()}
              const new_frame = Number(e.currentTarget.value)
              frameRef.current = new_frame
              setFrame(new_frame)
              if (!paused) {startAnimation()}
            }}
          />
          <div className="HorizontalSpacer" />
          <div className="MD-vis-controls" style={{flexGrow: 1, maxWidth: '15%', fontSize: '2vh'}}>
            <div title="Animation speed" style={{cursor: 'default', fontSize: '2.5vh', flexGrow: 1, display: 'flex', flexDirection: 'column', alignContent: 'middle', height: '3vh'}}>
              {TimerSymbol}
            </div>
            <div className="HorizontalSpacer" style={{minWidth: '0.5vh'}}/>
            <input type="range" min={1} max={30} style={{flexGrow: 4, maxWidth: '80%', verticalAlign: 'middle'}} defaultValue={fps}
              onChange={(e) => {
                if (e.target.value) {
                  setFps(e.target.valueAsNumber)
                }
              }}
            />
          </div>
          <div className="HorizontalSpacer" />
          <button className="App-button" style={{fontSize: '3vh'}} onClick={resetCamera} title="Reset camera to initial position">
            {ResetCameraSymbol}
          </button>
          <div className="HorizontalSpacer" style={{maxWidth: '1vh'}}/>
          <button className="App-button" style={{fontSize: '3vh'}} title="Save deposition movie">
            {SaveSymbol}
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
  return (
    <>
      <div className="App">
        <div className="MD-container">
          <div className="MD-params" id="input-container">
            <div className="App-header">
              <h1>Vacuum Deposition</h1>
            </div>
            <div style={{display: 'grid', width: '100%', alignItems: 'left'}}>
              <Composition running={running} setRunning={setRunning} />
            </div>
          </div>
          <div className="MD-vis" >
            <Vis numParticles={1000} />
          </div>
        </div>
        <div style={{display: 'flex', flexDirection: 'row-reverse'}}>
          <button className="App-button" style={{paddingLeft: '0.5vh'}} onClick={() => {
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
