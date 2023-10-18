import React, {useState} from 'react';
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
  const submitComposition = async () => {
    // Do nothing without web socket connection
    if (!socket.current) return;

    setWaiting(true);
    if (running) {
      // console.log("Sending cancel")
      socket.current.send("cancel");
    } else {
      // console.log("Sending config")
      socket.current.send(JSON.stringify(config));
      resetTrajectory();
      setRunning(true);
    }
  }
  return (<button
    className={"submit-button" + (running ? " cancel" : "")}
    disabled={waiting || !socket_connected }
    onClick={submitComposition}
  ><b>{running ? "Cancel" : "Submit"}</b></button>);
}

export default SubmitButton;
