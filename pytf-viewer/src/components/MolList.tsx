import React from 'react';
import { PytfConfig, MixtureComponentDetailed } from './types';
import '../App.css';


interface IMolList {
  running: boolean,
  molecules: Array<MixtureComponentDetailed>,
  config: PytfConfig,
  setConfig: React.Dispatch<React.SetStateAction<PytfConfig>>,
}

const MolList: React.FC<IMolList> =
  ({running, molecules, config, setConfig}: IMolList) =>
{
  return (<>{
    config.mixture.length === molecules.length ?
    config.mixture.map((mol, i) => {
      return (<div style={{display: 'flex', width: '100%', flexDirection: 'row'}}>
        {molecules[i].formula} ({molecules[i].name})
        <div className="HorizontalSpacer" style={{minWidth: '5%'}}/>
        <input
          type = "number"
          min = {0}
          size = {4}
          disabled = {running}
          value = {mol.ratio}
          onChange={(e) =>
            setConfig((prev) => {return {
              ...prev,
              mixture: prev.mixture.map(
                (m, j) => i === j ? {
                  ...m,
                  ratio: e.target.value ? e.target.valueAsNumber : 0
                } : m),
            }})
          }
        />
      </div>);
    })
    : ""
  }</>);
}

export default MolList;
