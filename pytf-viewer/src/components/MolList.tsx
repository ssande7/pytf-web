import React, { useState } from 'react';
import { PytfConfig, MixtureComponentDetailed } from './types';
import CloseIcon from '@mui/icons-material/Close';
import AddIcon from '@mui/icons-material/Add';
import RemoveIcon from '@mui/icons-material/Remove';
import '../App.css';
import CollapseIndicator from './CollapseIndicator';

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
  const [show_molecule_2d, setShowMolecule2d] = useState<number|null>(null);

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

  return (<>
      <div className="collapsible" onClick={() => setShowComposition((prev) => !prev)}>
        <b>Composition</b>
        <CollapseIndicator visible={show_composition} />
      </div>
      <div className="collapsible-content"
        style={{
          display: show_composition ? 'flex' : 'none',
          maxHeight: '350pt',
        }}
      >
        <div className="molecule-tile-grid">
          {
            config.mixture.length === molecules.length ?
              composition.map((i) => {
                return (
                  <li
                    className="molecule-tile"
                    style={{
                      background: '#eee',
                      color: 'black',
                      height: '150pt'
                    }}
                  >
                    <button
                      className="icon-button inverted header corner-button hover-red"
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
                        cursor: 'pointer',
                      }}
                      onClick={() => setShowMolecule2d(i)}
                    >
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
                    <div className="number-spin-container">
                      <button className="icon-button inverted hover-blue"
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
                          textAlign: 'center',
                        }}
                        min = {1} size = {4}
                        disabled = {running}
                        value = {config.mixture[i].ratio}
                        onChange={(e) => {
                          const val = e.target.value.replaceAll(RegExp('[^0-9]+', 'g'), '');
                          const ratio = val ? parseInt(val) : 1;
                          update_ratio(i, ratio > 0 ? ratio : 1);
                        }}
                      />
                      <button className="icon-button inverted hover-blue"
                        disabled={running}
                        onClick={() => update_ratio(i, config.mixture[i].ratio+1)}
                      ><AddIcon/></button>
                    </div>
                </li>);
            })
            : null
          }
          {running ? "" :
            <button
              onClick={() => {if (config.mixture.length > 0) setPickMol(true)}}
              className="molecule-tile add-molecule-button"
            >
              <AddIcon fontSize='large'/>
            </button>
          }
        </div>
    </div>
    <div
      className="molecule-picker-surround"
      style={{display: pick_mol ? "flex" : "none"}}
      onClick={() => setPickMol(false)}
    >
      <div className="molecule-picker">
        <div className="collapsible no-hover">
          <b>Add Molecule</b>
          <div className="icon-button header">
            <CloseIcon/>
          </div>
        </div>
        <div className="molecule-tile-grid">
          { config.mixture.length === molecules.length ?
            config.mixture.map((mol, i) => {
              return mol.ratio > 0 ? ""
                : (
                  <button
                    className="molecule-tile molecule-tile-clickable"
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
                      <div style={{
                        height: '100%',
                        display: 'inline-block',
                        verticalAlign: 'middle'
                      }}/>
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
                )
            })
          : null
        }
        </div>
      </div>
    </div>
    <div
      className="molecule-picker-surround"
      style={{
          display: show_molecule_2d === null ? "none" : "flex",
        }}
      onClick={() => setShowMolecule2d(null)}
    >
        {show_molecule_2d !== null ?
          <div>
            <div className="collapsible no-hover">
              <b>{molecules[show_molecule_2d].name} - {format_formula(molecules[show_molecule_2d].formula)}</b>
              <div className="icon-button header" >
                <CloseIcon/>
              </div>
            </div>
            <img style={{
                maxHeight: '90%',
                minWidth: '30vw', background: '#eee',
              }}
              width='auto' height='auto'
              src={"molecules/" + molecules[show_molecule_2d].formula + '.png'}
            />
          </div> : null
        }
    </div>
  </>);
}

export default MolList;
