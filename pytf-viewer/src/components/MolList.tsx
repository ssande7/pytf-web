import React, { useEffect, useRef, useState } from 'react';
import { PytfConfig, MixtureComponentWith3D } from './types';
import CloseIcon from '@mui/icons-material/Close';
import AddIcon from '@mui/icons-material/Add';
import Info from '@mui/icons-material/Info';
import '../App.css';
import TileSpinner from './TileSpinner';
import { Particles, Visualizer } from 'omovi';
import { Vector3, Color } from 'three';
import { atom_types } from './Visualiser';
import SmilesImg from './SmilesImg';

const RegSplitNums = RegExp('[0-9]+|[^0-9]+', 'g');
const RegNum = RegExp('[0-9]+', 'g');

const format_formula = (formula: string, sub_sz = '8pt') => {
  return (<>
    {formula.match(RegSplitNums)?.map((str) =>
      RegNum.test(str) ? <sub style={{fontSize: sub_sz}}>{str}</sub> : str
    )}
  </>);
}

interface IMolList {
  running: boolean,
  molecules: Array<MixtureComponentWith3D>,
  config: PytfConfig,
  setConfig: React.Dispatch<React.SetStateAction<PytfConfig>>,
}

const MolList: React.FC<IMolList> =
  ({running, molecules, config, setConfig}: IMolList) =>
{
  const [pick_mol, setPickMol] = useState(false);
  const [composition, setComposition] = useState<Array<number>>([]);
  const [show_molecule_2d, setShowMolecule2d] = useState<MixtureComponentWith3D | null>(null);


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
      <div className="collapsible no-hover">
        <b>Composition</b>
      </div>
      <div className="collapsible-content"
        style={{ maxHeight: '370pt', }}
      >
        <div className="molecule-tile-grid">
          {
            config.mixture.length === molecules.length ?
              composition.map((i) => {
                return (
                  <div className="molecule-tile" style={{background: 'var(--col-smiles-bg)', color: 'var(--col-smiles-fg)', position: 'relative'}}>
                    <button
                      title="Remove"
                      className="icon-button inverted header corner-button hover-red"
                      style={{position: 'absolute', top: '3pt', right: '3pt', zIndex: 2}}
                      disabled={running}
                      onClick={() => {
                        update_ratio(i, 0);
                        setComposition(composition.filter((j) => i !== j));
                      }}
                    >
                      <CloseIcon/>
                    </button>
                    <button
                      className="icon-button inverted header corner-button hover-blue"
                      style={{position: 'absolute', top: '3pt', left: '3pt', zIndex: 2}}
                      onClick={() => {
                        setShowMolecule2d(molecules[i]);
                      }}
                    >
                      <Info/>
                    </button>
                    <div style={{
                        marginLeft: 'auto', marginRight: 'auto',
                        marginTop: 'auto', marginBottom: 'auto',
                        width: '80pt', height: '80pt',
                        cursor: 'pointer',
                      }}
                      onClick={() => {
                        setShowMolecule2d(molecules[i]);
                      }}
                    >
                      <SmilesImg smiles={molecules[i].smiles}
                        options={{width: '100%', height: '100%'}}
                      />
                    </div>
                    <div>
                      {format_formula(molecules[i].formula)}<br/>
                      {molecules[i].name}
                    </div>
                    <TileSpinner
                      disabled={running}
                      ratio={config.mixture[i].ratio}
                      updateRatio={(ratio: number) => {
                        update_ratio(i, ratio)
                      }}
                    />
                </div>);
            })
            : null
          }
          {running ? "" :
            <button
              onClick={() => {if (config.mixture.length > 0) setPickMol(true)}}
              className="molecule-tile-add"
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
        <div className="molecule-tile-grid"
          style={{gridAutoRows: '120pt'}}
        >
          { config.mixture.length === molecules.length ?
            config.mixture.map((mol, i) => {
              return mol.ratio > 0 ? ""
                : (
                  <div
                    className="molecule-tile molecule-tile-clickable"
                    style={{background: 'var(--col-smiles-bg)', color: 'var(--col-smiles-fg)', position: 'relative'}}
                    onClick={() => {
                      setPickMol(false);
                      update_ratio(i, 1);
                      composition.push(i);
                      setComposition(composition);
                    }}
                  >
                    <button
                      className="icon-button inverted header corner-button hover-blue"
                      style={{position: 'absolute', top: '3pt', left: '3pt', zIndex: 2}}
                      onClick={(e) => {
                        setShowMolecule2d(molecules[i]);
                        e.stopPropagation();
                      }}
                    >
                      <Info/>
                    </button>
                    <div style={{
                      marginLeft: 'auto', marginRight: 'auto',
                      marginTop: 'auto', marginBottom: 'auto',
                      width: '70pt', height: '80pt',
                    }}>
                      <SmilesImg smiles={molecules[i].smiles}
                        options={{width: '100%', height: '100%'}}
                      />
                    </div>
                    <div>
                      {format_formula(molecules[i].formula)}<br/>
                      {molecules[i].name}
                    </div>
                  </div>
                )
            })
          : null
        }
        </div>
      </div>
    </div>
    <MoleculeView
      mol={show_molecule_2d}
      close_fn={() => {
        setShowMolecule2d(null)
      }}
    />
  </>);
}


interface IMoleculeView {
  mol: MixtureComponentWith3D | null,
  close_fn: () => void,
}

const MoleculeView: React.FC<IMoleculeView> = ({mol, close_fn}) => {
  const molecule_3d_element = useRef<HTMLDivElement | null>(null);
  const [mol3d, setMol3D] = useState<Visualizer | null>(null);
  const [loading_mol3d, setLoadingMol3D] = useState(false);
  const [atoms_3d, setAtoms3D] = useState<Particles | null>();
  const prevAtomsRef = useRef<Particles | null>();
  const cameraInitTarget = new Vector3(0, 0, 0);
  const cameraInitPosition = new Vector3(0.7, 0.7, 0.7);

  useEffect(() => {
    setAtoms3D(mol?.particles)
  }, [mol])

  useEffect(() => {
    if (molecule_3d_element.current && !loading_mol3d && !mol3d) {
      setLoadingMol3D(true);
      const new_mol3d = new Visualizer({
        domElement: molecule_3d_element.current,
        initialColors: atom_types.map((atom) => atom.color),
      })
      atom_types.map((atom, idx) => new_mol3d.setRadius(idx, atom.radius/10.));
      new_mol3d.materials.particles.shininess = 50;
      new_mol3d.ambientLight.intensity = 0.5;
      new_mol3d.pointLight.intensity = 0.7;
      new_mol3d.scene.background = new Color(0x606160);
      setMol3D(new_mol3d);
      setLoadingMol3D(false);
    }
    return () => { if (mol3d) mol3d.dispose() }
  }, [molecule_3d_element]);

  useEffect(() => {
    prevAtomsRef.current = atoms_3d;
  }, [atoms_3d])

  const prevAtoms = prevAtomsRef.current;
  useEffect(() => {
    if (!mol3d) { return }
    if (prevAtoms && prevAtoms !== atoms_3d) {
      mol3d.remove(prevAtoms)
    }
    if (atoms_3d) {
      mol3d.add(atoms_3d)
      mol3d.setCameraTarget(cameraInitTarget);
      mol3d.setCameraPosition(cameraInitPosition);
    }
  }, [atoms_3d, mol3d])

  return (
    <div
      className="molecule-picker-surround"
      style={{display: mol === null ? "none" : "flex",}}
      onMouseDown={close_fn}
    >
      <div onMouseDown={(e) => {e.stopPropagation()}} style={{zIndex: 110}}>
        <div className="collapsible no-hover"
            onClick={close_fn}
        >
          <Info style={{marginRight: '5pt'}}/><b>{mol?.name} - {mol ? format_formula(mol.formula, '10pt') : null}</b>
          <div className="icon-button header">
            <CloseIcon/>
          </div>
        </div>
        <div className="molecule-picker-content">
          <div className="molecule-picker-imgfill">
              <SmilesImg smiles={mol ? mol.smiles : ""}
                options={{width: '100%', height: '100%'}}
              />
          </div>
          <div
            className="molecule-picker-3d"
            ref={molecule_3d_element}
            onMouseDown={(e) => {e.stopPropagation()}}
          >
          </div>
        </div>
      </div>
    </div>
  );
};

export default MolList;
