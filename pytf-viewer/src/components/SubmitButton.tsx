import React, {useState} from 'react';
import { PytfConfig } from './types';
import '../App.css';

interface ISubmitButton {
  socket: React.MutableRefObject<WebSocket | null>,
  socket_connected: boolean,
  config: PytfConfig,
  running: boolean,
  setRunning: React.Dispatch<React.SetStateAction<boolean>>,
}

const SubmitButton: React.FC<ISubmitButton> =
  ({socket, socket_connected, config, running, setRunning}: ISubmitButton) =>
{
  const [waiting, setWaiting] = useState(false);
  const submitComposition = async () => {
    // Do nothing without web socket connection
    if (!socket.current) return;

    setWaiting(true);
    if (running) {
      // fetch("/cancel", {method: "post"});
      socket.current.send("cancel");
    } else {
      console.log("Sending config: " + JSON.stringify(config))
      // fetch("/submit", {
      //   method: "post",
      //   headers: {
      //     'Content-Type': 'application/json'
      //   },
      //   body: JSON.stringify(config)
      // });
      socket.current.send(JSON.stringify(config));
    }
    setWaiting(false);
    setRunning(!running);
  }
  return (<button disabled={waiting || !socket_connected } onClick={submitComposition}>{running ? "Cancel" : "Submit"}</button>);
}

export default SubmitButton;
