import React, { useEffect, useState, useRef, useMemo } from 'react';
import { Particles, Visualizer } from 'omovi'
import { Vector3 } from 'three'
import './App.css';

const Composition: React.FC = () => {
  const dummy_list = ['foo', 'bar', 'baz'].map((value) => { return <li> {value} </li>})
  return (
    <div className="MD-param-group">
      <h3>Composition</h3>
      <p>
        <ul>
          <li>{dummy_list}</li>
        </ul>
      </p>
    </div>
  );
}

const Protocol: React.FC = () => {
  return (
    <div className="MD-param-group">
      <h3>Protocol</h3>
      <p>Deposition rate:</p>
      <p>Deposition velocity:</p>
    </div>
  );
}

interface VisProps {
  numParticles: number;
}

const Vis: React.FC<VisProps> = ({ numParticles }) => {
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

  // Adjustable animation speed
  interface FPSLimits {
    min: number;
    max: number;
  }
  const FpsSlider: React.FC<FPSLimits> = ({min, max}) => {
    // &#x1F3CE; = 🏎
    return (
      <div className="MD-vis-controls" style={{width: '12vh', fontSize: '2vh'}}>
        <p style={{fontSize: '2vh', width: '4vh', verticalAlign: 'middle'}}>&#x1F3CE;</p>
        <input type="range" min={min} max={max} style={{flexGrow: 8, verticalAlign: 'middle'}} defaultValue={fps}
          onChange={(e) => {
            if (e.target.value) {
              setFps(Number(e.target.value))
            }
          }}
        />
      </div>
    );
  }

  function resetCamera() {
    if (vis && camTargetInit && camPositionInit) {
      vis.setCameraTarget(camTargetInit)
      vis.setCameraPosition(camPositionInit)
    }
  }

  return (
    <>
      <div id="canvas-container" style={{ height: '100%', width: '100%'}}>
        <div style={{ height: '77vh', width: '100%', border: 'medium solid grey', backgroundColor: 'black'}} ref={domElement}>
        </div>
        <div id="controls" className="MD-vis-controls" style={{width: '100%'}}>
          <button className={paused ? "PlayButton" : "PauseButton"} onClick={toggleAnimation} />
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
          &emsp;
          <FpsSlider min={1} max={30} />
          <button className="App-button" onClick={resetCamera} style={{width: '12vh'}}>
            Reset Camera
          </button>
        </div>
      </div>
    </>
  );
}

const App: React.FC = () => {
  return (
    <>
      <div className="App">
        <div className="App-header">
          <h1>Vacuum Deposition</h1>
        </div>
        <div className="MD-container">
          <div className="MD-params" id="input-container">
            <div style={{display: 'grid', alignItems: 'left'}}>
              <Composition />
              <Protocol />
              <div className="PlayButton"></div>
              <div className="PauseButton"></div>
            </div>
          </div>
          <div className="MD-vis" >
            <Vis numParticles={1000} />
          </div>
        </div>
      </div>
    </>
  );
}

export default App;
