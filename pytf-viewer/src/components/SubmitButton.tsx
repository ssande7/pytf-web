import React from 'react';
import { PytfConfig } from './types';
import '../App.css';

interface ISubmitButton {
  socket: React.MutableRefObject<WebSocket | null>,
  socket_connected: boolean,
  config: PytfConfig,
  running: boolean,
  setRunning: React.Dispatch<React.SetStateAction<boolean>>,
  waiting: boolean,
  setWaiting: React.Dispatch<React.SetStateAction<boolean>>,
  resetTrajectory: () => void,
}

const SubmitButton: React.FC<ISubmitButton> =
  ({socket, socket_connected, config, running, setRunning, waiting, setWaiting, resetTrajectory}: ISubmitButton) =>
{
  let sum = 0;
  for (let i = 0; i < config.mixture.length; i++) {
    sum += config.mixture[i].ratio;
  }
  const all_zero = sum === 0;

  const submitComposition = async () => {
    // Do nothing without web socket connection
    if (!socket.current) return;

    setWaiting(true);
    if (running) {
      // console.log("Sending cancel")
      socket.current.send("cancel");
    } else {
      // console.log("Sending config")
      socket.current.send(JSON.stringify({mixture: config.mixture, ...Object.fromEntries(config.settings)}));
      resetTrajectory();
      setRunning(true);
    }
  }
  return (<button
    className={"submit-button" + (running ? " cancel" : "")}
    disabled={waiting || all_zero || !socket_connected }
    onClick={submitComposition}
  ><b>{running ? "Cancel" : "Submit"}</b></button>);
}

export default SubmitButton;
