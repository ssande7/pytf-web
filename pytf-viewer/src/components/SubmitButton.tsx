import React, {useState} from 'react';
import { PytfConfig } from './types';
import '../App.css';

interface ISubmitButton {
  config: PytfConfig,
  running: boolean,
  setRunning: React.Dispatch<React.SetStateAction<boolean>>,
}

const SubmitButton: React.FC<ISubmitButton> =
  ({config, running, setRunning}: ISubmitButton) =>
{
  const [waiting, setWaiting] = useState(false);
  const submitComposition = async () => {
    setWaiting(true);
    if (running) {
      fetch("/cancel", {method: "post"});
    } else {
      console.log("Sending config: " + JSON.stringify(config))
      fetch("/submit", {
        method: "post",
        headers: {
          'Content-Type': 'application/json'
        },
        body: JSON.stringify(config)
      });
    }
    setWaiting(false);
    setRunning(!running);
  }
  return (<button disabled={waiting} onClick={submitComposition}>{running ? "Cancel" : "Submit"}</button>);
}

export default SubmitButton;
