import React, { useState } from 'react';
import { PytfConfig, MixtureComponentDetailed } from './types';
import CloseIcon from '@mui/icons-material/Close';
import AddIcon from '@mui/icons-material/Add';
import RemoveIcon from '@mui/icons-material/Remove';
import ExpandMoreIcon from '@mui/icons-material/ExpandMore';
import ExpandLessIcon from '@mui/icons-material/ExpandLess';
import '../App.css';

const RegSplitNums = RegExp('[0-9]+|[^0-9]+', 'g');
const RegNum = RegExp('[0-9]+', 'g');

const format_formula = (formula: string, sub_sz = '6pt') => {
  return (<>
    {formula.match(RegSplitNums)?.map((str) =>
      RegNum.test(str) ? <sub style={{fontSize: sub_sz}}>{str}</sub> : str
    )}
  </>);
}

interface IMolList {
  running: boolean,
  molecules: Array<MixtureComponentDetailed>,
  config: PytfConfig,
  setConfig: React.Dispatch<React.SetStateAction<PytfConfig>>,
}

const MolList: React.FC<IMolList> =
  ({running, molecules, config, setConfig}: IMolList) =>
{
  const [pick_mol, setPickMol] = useState(false);
  const [show_composition, setShowComposition] = useState(true);
  const [composition, setComposition] = useState<Array<number>>([]);

  const update_ratio = (i: number, val: number) => {
    setConfig((prev) => {return {
      ...prev,
      mixture: prev.mixture.map(
        (m, j) => i === j ? {
          ...m,
          ratio: val
        } : m),
    }})
  }

  return (
    <div style={{ width: '100%' }}>
      <div className="collapsible" onClick={() => setShowComposition((prev) => !prev)}>
        <b>Composition</b>
        <div style={{float: 'right', fontSize: '16pt'}}>
          {show_composition ? <ExpandLessIcon/> : <ExpandMoreIcon/> }
        </div>
      </div>
      <div className="collapsible-content"
        style={{
          display: show_composition ? 'block' : 'none',
          width: '100%'
        }}
      >
        <ul style={{ overflow: 'visible' }}>
          <li><ul style={{maxHeight: '320pt', overflowY: 'scroll'}}>{
            config.mixture.length === molecules.length ?
              composition.map((i) => {
                return (
                  <li
                    className="molecule-tile"
                    style={{background: '#eee', color: 'black', height: '150pt'}}
                  >
                    <button
                      className="App-button"
                      style={{
                        color: 'black',
                        textAlign: 'center',
                        width: '20pt',
                        height: '18pt',
                        float: 'right',
                      }}
                      disabled={running}
                      onClick={() => {
                        update_ratio(i, 0);
                        setComposition(composition.filter((j) => i !== j));
                      }}
                    >
                      <CloseIcon/>
                    </button>
                    <div style={{
                      marginLeft: 'auto', marginRight: 'auto',
                      marginTop: 'auto', marginBottom: 'auto',
                      width: '90pt', height: '90pt',
                    }}>
                      <div style={{height: '100%', display: 'inline-block', verticalAlign: 'middle'}}/>
                      <img
                        style={{
                          maxWidth: '80pt', maxHeight: '100%',
                          verticalAlign: 'middle',
                        }}
                        width="auto" height="auto"
                        src={"molecules/" + molecules[i].formula + ".png"}
                      />
                    </div>
                    <div>
                      {format_formula(molecules[i].formula, '8pt')}<br/>
                      {molecules[i].name}
                    </div>
                    <div style={{
                          marginLeft: 'auto', marginRight: 'auto',
                          marginTop: 'auto', marginBottom: 'auto',
                      }}
                    >
                      <button className="App-button"
                        style={{
                          color: 'black',
                          verticalAlign: 'middle',
                          display: 'inline-block'
                        }}
                        disabled={running}
                        onClick={() => {
                          if (config.mixture[i].ratio > 1)
                            update_ratio(i, config.mixture[i].ratio-1)
                        }}
                      ><RemoveIcon/></button>
                      <input
                        type="text"
                        inputMode="numeric"
                        style={{
                          fontSize: '12pt',
                          verticalAlign: 'middle',
                          textAlign: 'center',
                          display: 'inline-block'
                        }}
                        min = {1} size = {4}
                        width = "80%"
                        disabled = {running}
                        value = {config.mixture[i].ratio}
                        onChange={(e) => {
                          const val = e.target.value.replaceAll(RegExp('[^0-9]+', 'g'), '');
                          const ratio = val ? parseInt(val) : 1;
                          update_ratio(i, ratio > 0 ? ratio : 1);
                        }}
                      />
                      <button className="App-button"
                        style={{
                          color: 'black',
                          verticalAlign: 'middle',
                          display: 'inline-block'
                        }}
                        disabled={running}
                        onClick={() => update_ratio(i, config.mixture[i].ratio+1)}
                      ><AddIcon/></button>
                    </div>
                </li>);
            })
            : ""
          }
          <li className="molecule-tile">
            <button
              onClick={() => setPickMol(true)}
              className="molecule-tile"
              style={{
                color: 'white',
                width: '100%', height: '150pt',
                borderRadius: 0,
                background: 'transparent',
                borderColor: '#eee',
                cursor: 'pointer'
              }}
              disabled={running}
            >
              <AddIcon fontSize='large'/>
            </button>
          </li>
        </ul></li>
      </ul>
    </div>
    <div
      className="molecule-picker"
      style={{display: pick_mol ? "block" : "none"}}
      onClick={() => setPickMol(false)}
    >
      <ul><li>
        <ul>{
          config.mixture.length === molecules.length ?
            config.mixture.map((mol, i) => {
              return mol.ratio > 0 ? ""
                : (<li className="molecule-tile">
                  <button
                    style={{
                      width: '100%', height: '130pt',
                      borderStyle: 'none'
                    }}
                    disabled={running}
                    onClick={() => {
                      setPickMol(false);
                      update_ratio(i, 1);
                      composition.push(i);
                      setComposition(composition);
                    }}
                  >
                    <div style={{
                      marginLeft: 'auto', marginRight: 'auto',
                      marginTop: 'auto', marginBottom: 'auto',
                      width: '80pt', height: '90pt',
                    }}>
                      <div style={{height: '100%', display: 'inline-block', verticalAlign: 'middle'}}/>
                      <img
                        style={{
                          maxWidth: '100%', maxHeight: '100%',
                          verticalAlign: 'middle',
                        }}
                        width="auto" height="auto"
                        src={"molecules/" + molecules[i].formula + ".png"}
                      />
                    </div>
                    <div>
                      {format_formula(molecules[i].formula)}<br/>
                      {molecules[i].name}
                    </div>
                  </button>
                </li>)
            })
          : ""
        }</ul>
      </li></ul>
    </div>
  </div>);
}

export default MolList;
