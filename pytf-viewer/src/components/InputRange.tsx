import { useState } from "react";
import { InputConfig } from "./types";

interface IInputRange {
  config: InputConfig,
  value: number,
  setConfigValue: Function,
  disabled: boolean,
}

const InputRange: React.FC<IInputRange> = ({config, setConfigValue, disabled}) => {
  const [value, setValue] = useState(config.default);
  const [text, setText] = useState(config.dec_places === null ? config.default.toString() : config.default.toFixed(config.dec_places));
  const number_box = config.force_number_box || config.min === null || config.max === null || config.dec_places === null;
  var scale = config.dec_places === null ? 1 : Math.pow(10.0, config.dec_places);
  if (config.increment) { scale = 1. / config.increment; }
  const updateValue = (v: number) => {
    setValue(v);
    setConfigValue(v);
  };
  // TODO: dec_places for number box
  return <>
    <div className="flex-row">
      <div style={{marginRight: 'auto'}}>{config.display_name}{number_box && config.display_units ? " (" + config.display_units + ")" : ""}:</div>
      {number_box ?
        <input type={"number"}
          min={config.min === null ? undefined : config.min}
          max={config.max === null ? undefined : config.max}
          step={1./scale}
          defaultValue={config.default}
          disabled={disabled}
          value={text}
          onChange = { (e) => {
            var filtered = e.target.value.replaceAll(RegExp('[^0-9.]+', 'g'), '')
            setText(filtered)
          }}
          onBlur = { (e) => {
            var filtered = e.target.value.replaceAll(RegExp('[^0-9.]+', 'g'), '')
            var v = filtered ? parseFloat(filtered) : config.default;
            if (config.dec_places !== null) { v = Math.round(v * scale) / scale }
            if (config.min !== null && v < config.min) { v = config.min }
            if (config.max !== null && v > config.max) { v = config.max }
            setText(config.dec_places === null ? v.toString() : v.toFixed(config.dec_places))
            updateValue(v);
          }}
        />
        : <div>{config.dec_places === null ? value : value.toFixed(config.dec_places)} {config.display_units}</div>
      }
    </div>
    {number_box ? null :
      <input type="range"
        min={config.min === null ? undefined : config.min * scale} max={config.max === null ? undefined : config.max * scale}
        defaultValue={config.default * scale}
        disabled={disabled}
        onChange = { (e) => updateValue(e.target.valueAsNumber/scale) }
      />
    }
  </>
}

export default InputRange;
