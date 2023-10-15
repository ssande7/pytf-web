import { Particles } from "omovi";
import { useState } from "react";
import { atom_types, Lx, Lz } from "./Visualiser";


interface IRoughness {
  particles: Particles | null,
  num_bins: number,
  setNumBins: React.Dispatch<React.SetStateAction<number>>,
  roughness: number | null,
  setRoughness: React.Dispatch<React.SetStateAction<number | null>>,
  mean_height: number | null,
  setMeanHeight: React.Dispatch<React.SetStateAction<number | null>>,
  setHeightMap: React.Dispatch<React.SetStateAction<Float32Array | null>>,
  setShowHeightMap: React.Dispatch<React.SetStateAction<boolean>>,
  setNewRoughness: React.Dispatch<React.SetStateAction<boolean>>,
}

const Roughness: React.FC<IRoughness> = ({
  particles, num_bins, setNumBins, roughness, setRoughness,
  mean_height, setMeanHeight, setHeightMap, setShowHeightMap,
  setNewRoughness
}) => {
  const [min_height, setMinHeight] = useState<number>(0);
  function calcRoughness() {
    if (particles === null) {
      setHeightMap(null);
      setMeanHeight(null);
      setMinHeight(0);
      setRoughness(null);
      return;
    }
    const bins_sq = num_bins * num_bins;
    var height_map = new Float32Array(bins_sq).fill(0);
    var min_ht = Number.MAX_SAFE_INTEGER;
    var i, mean_y = 0;
    for (i = 0; i < particles.count; i++) {
      mean_y += particles.getPosition(i).y;
    }
    mean_y /= particles.count;
    for (i = 0; i < particles.count; i++) {
      const pos = particles.getPosition(i);
      if (pos.y > mean_y*5) { continue } // Skip atoms that are obviously in gas phase
      const rad = atom_types[particles.getType(i)].radius / 10;
      var bx = Math.floor(pos.x * num_bins / Lx);
      var bz = Math.floor(pos.z * num_bins / Lz);
      while (bx < 0) { bx += num_bins }
      while (bx >= num_bins) { bx -= num_bins }
      while (bz < 0) { bz += num_bins }
      while (bz >= num_bins) { bz -= num_bins }
      const idx = bx*num_bins + bz;
      // Use atom position + radius for better heat map tile placement
      const ht = pos.y + rad;
      if (height_map[idx] < ht) { height_map[idx] = ht }
      if (ht < min_ht) { min_ht = ht }
    }
    const mean_height = height_map.reduce((a, b) => a + b) / bins_sq;
    const sqvar = height_map
      .map(h => (h - mean_height)*(h - mean_height))
      .reduce((a, b) => a + b);
    const roughness = Math.sqrt(sqvar / bins_sq);
    setHeightMap(height_map);
    setMeanHeight(mean_height);
    setMinHeight(min_ht);
    setRoughness(roughness);
    setNewRoughness(true);
  }

  return (<div className="MD-param-group">
    <div className="collapsible no-hover">
      <b>Settings</b>
    </div>
    <div className="collapsible-content">
      <div className="flex-row">
        <div style={{marginRight: 'auto'}}>Bins per side:</div>
        <div>{num_bins}</div>
      </div>
      <input type="range" min={1} max={20} defaultValue={10}
        onChange = {
          (e) => setNumBins(e.target.valueAsNumber)
        }
      />
      <br/>
      <div className="flex-row" style={{marginTop: '10pt'}}>
        <div style={{marginRight: 'auto'}}>Show height map:</div>
        <label className="toggle-slider">
          <input type="checkbox" defaultChecked={true}
            onChange = {(e) => setShowHeightMap(e.target.checked)}
          />
          <span className="slider"></span>
        </label>
      </div>
    </div>
    <div className="collapsible no-hover">
      <b>Results</b>
    </div>
    <div className="collapsible-content">
      <div className="flex-row">
        <div style={{marginRight: 'auto'}}>Mean film thickness:</div>
        <div>{mean_height !== null ? (mean_height - min_height).toFixed(3) + ' nm' : '???'}</div>
      </div>
      <div className="flex-row">
        <div style={{marginRight: 'auto'}}>Roughness:</div>
        <div>{roughness !== null ? roughness.toFixed(3) + ' nm' : '???'}</div>
      </div>
    </div>
    <button className="submit-button roughness"
      onClick={calcRoughness}
    >
      <b>Calculate Roughness</b>
    </button>
  </div>);
}

export default Roughness;
