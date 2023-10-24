import { Particles } from 'omovi';
import React, { useEffect, useState } from 'react';
import CollapseIndicator from './CollapseIndicator';
import MolList from './MolList';
import SubmitButton from './SubmitButton';
import { MixtureComponentDetailed, MixtureComponentWith3D, PytfConfig } from './types';

interface IComposition {
  socket: React.MutableRefObject<WebSocket | null>,
  socket_connected: boolean,
  running: boolean,
  setRunning: React.Dispatch<React.SetStateAction<boolean>>,
  resetTrajectory: () => void,
  submit_waiting: boolean,
  setSubmitWaiting: React.Dispatch<React.SetStateAction<boolean>>,
}

const Composition: React.FC<IComposition>
  = ({socket, socket_connected, running, setRunning, resetTrajectory, submit_waiting, setSubmitWaiting}) =>
{
  const [molecules, setMolecules] = useState<Array<MixtureComponentWith3D>>([]);
  const [config, setConfig] = useState<PytfConfig>({deposition_velocity: 0.35, mixture: []});

  // Get the list of available molecules on load
  useEffect(() => {
    let abort = new AbortController();
    const fetchMolecules = async () => {
      const mols: {molecules: Array<MixtureComponentDetailed>} =
        await fetch("/molecules", abort).then(data => data.json());
      // console.log("Got molecules: " + JSON.stringify(mols))
      setMolecules(mols.molecules.map((mol) => {
        const natoms = mol.atoms.length;
        const particles = new Particles(natoms);
        for (let i = 0; i < natoms; i++) {
          particles.add(
            mol.atoms[i].y / 10.,
            mol.atoms[i].z / 10.,
            mol.atoms[i].x / 10.,
            mol.atoms[i].typ,
            mol.atoms[i].typ
          );
        }
        return {...mol, particles: particles};
      }))
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
      <div className="collapsible no-hover">
        <b>Protocol</b>
      </div>
      <div className="collapsible-content">
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

export default Composition;
