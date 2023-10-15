import React, { useState } from 'react';
import AddIcon from '@mui/icons-material/Add';
import RemoveIcon from '@mui/icons-material/Remove';
import '../App.css';

interface ITileSpinner {
  disabled: boolean;
  ratio: number;
  updateRatio: (ratio: number) => void;
}

const TileSpinner: React.FC<ITileSpinner> = ({disabled, ratio, updateRatio}) => {
  const [text, setText] = useState('1');
  return (
    <div className="number-spin-container">
      <button className="icon-button inverted hover-blue"
        disabled={disabled}
        onClick={() => {
          if (ratio > 1) {
            setText((ratio-1).toString());
            updateRatio(ratio-1)
          }
        }}
      ><RemoveIcon/></button>
      <input
        type="text"
        inputMode="numeric"
        style={{
          fontSize: '12pt',
          textAlign: 'center',
        }}
        min={1} size={4}
        disabled={disabled}
        value={text}
        onChange={(e) => {
          const filtered = e.target.value.replaceAll(RegExp('[^0-9]+', 'g'), '');
          setText(filtered)
        }}
        onBlur={(e) => {
          const val = e.target.value.replaceAll(RegExp('[^0-9]+', 'g'), '');
          let r = val ? parseInt(val) : 1;
          if (r < 1) r = 1;
          updateRatio(r);
          setText(r.toString());
        }}
      />
      <button className="icon-button inverted hover-blue"
        disabled={disabled}
        onClick={() => {
          updateRatio(ratio+1);
          setText((ratio+1).toString());
        }}
      ><AddIcon/></button>
    </div>
  );
};

export default TileSpinner;
