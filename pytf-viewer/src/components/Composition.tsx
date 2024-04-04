import { Particles } from 'omovi';
import React, { useEffect, useState } from 'react';
import InputRange from './InputRange';
import MolList from './MolList';
import SubmitButton from './SubmitButton';
import { InputConfig, MixtureComponentDetailed, MixtureComponentWith3D, PytfConfig } from './types';

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
  const [input_config, setInputConfig] = useState<Map<string, InputConfig>>(new Map([]));
  const [config, setConfig] = useState<PytfConfig>({settings: new Map([]), mixture: []});

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

  // Get configuration options
  useEffect(() => {
    let abort = new AbortController();
    const fetchInputConfig = async () => {
      const cfg: Map<string, InputConfig> = new Map(Object.entries(await fetch("/input-config", abort).then(data => data.json())));
      setConfig((config) => {
        cfg.forEach((v, k) => {
          config.settings.set(k, v.default);
          if (v.display_name === null) { v.display_name = k }
        });
        return config;
      })

      // Display protocol settings in alphabetical order
      const keys = Array.from(cfg.keys());
      keys.sort((a, b) => {
        var ia = cfg.get(a)?.display_name;
        if (!ia) { ia = a; }
        var ib = cfg.get(b)?.display_name;
        if (!ib) { ib = b; }
        if (ia < ib) { return -1; }
        if (ia > ib) { return 1; }
        return 0;
      });
      setInputConfig(cfg);
      setProtocolKeys(keys);
    };
    fetchInputConfig();
    return () => abort.abort();
  }, [setInputConfig]);

  // Set up base config with everything zeroed
  useEffect(() => {
    setConfig((config) => {return {
      ...config,
      mixture: molecules.map((mol) => {return {res_name: mol.res_name, ratio: 0}}),
    }});
  }, [molecules, setConfig]);

  const [protocol_keys, setProtocolKeys] = useState<Array<string>>([]);

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
        {
          protocol_keys.map((k) => {
            const cfg = input_config.get(k);
            return cfg ? 
              <>
                <InputRange
                  value={config.settings.get(k)}
                  config={cfg}
                  setConfigValue={(v: number) => setConfig((config) => {
                    config.settings.set(k, v);
                    return config
                  })}
                  disabled={running}
                />
                {k === protocol_keys[protocol_keys.length-1] ? null : <div className="flex-row" style={{minHeight: '10px'}}/>}
              </>
            : null
          })
        }
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
