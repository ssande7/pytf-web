import React, { useEffect, useState, useRef } from 'react';
import { Particles, Visualizer } from 'omovi'
import './App.css';

interface VisProps {
  numParticles: number;
}

const Vis: React.FC<VisProps> = ({ numParticles }) => {
  const [vis, setVis] = useState<Visualizer | null>(null);
  const [loadingVis, setLoadingVis] = useState(false);
  const [loadingParticles, setLoadingParticles] = useState(false);
  const domElement = useRef<HTMLDivElement | null>(null);
  const [particles, setParticles] = useState<Particles | undefined>(undefined);

  useEffect(() => {
    if (!particles && !loadingParticles) {
      setLoadingParticles(true)
      const new_particles = new Particles(numParticles)
      for (let i = 0; i < numParticles; i++) {
        new_particles.add(
          120 * (Math.random() - 0.5),
          120 * (Math.random() - 0.5),
          120 * (Math.random() - 0.5),
          i,
          1
        )
      }
      setParticles(new_particles)
      setLoadingParticles(false)
    }
  }, [numParticles, particles, setParticles, loadingParticles, setLoadingParticles])

  useEffect(() => {
    if (domElement.current && !loadingVis && !vis) {
      setLoadingVis(true);
      const new_vis = new Visualizer({
        domElement: domElement.current
      })
      setVis(new_vis)
      setLoadingVis(false);
    }
  }, [vis, domElement, loadingVis])

  const prevParticlesRef = useRef<Particles>()
  useEffect(() => {
    prevParticlesRef.current = particles
  })
  const prevParticles = prevParticlesRef.current

  useEffect(() => {
    if (!vis) { return }
    if (prevParticles && prevParticles !== particles) {
      vis.remove(prevParticles)
      prevParticles.dispose()
    }
    if (particles) {
      vis.add(particles)
    }
  }, [particles, prevParticles, vis])

  useEffect(() => {
    return () => {
      if (vis) {
        vis.dispose()
      }
    }
  }, [vis])

  return (
    <>
      <div id="canvas-container" style={{ height: '100%', width: '100%'}}>
        <div style={{ height: '70vh', width: '100%'  }} ref={domElement}> 
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
          <Vis numParticles={1000} />
        </div>
      </div>
    </>
  );
}
    // <div className="App">
    //   <header className="App-header">
    //     <p>
    //       Edit <code>src/App.tsx</code> and save to reload.
    //     </p>
    //     <a
    //       className="App-link"
    //       href="https://reactjs.org"
    //       target="_blank"
    //       rel="noopener noreferrer"
    //     >
    //       Learn React
    //     </a>
    //     <Vis />
    //   </header>
    // </div>

export default App;
