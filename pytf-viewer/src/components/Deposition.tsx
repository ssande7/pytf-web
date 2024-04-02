import React, { useEffect, useState, useRef } from 'react';
import { Particles } from 'omovi'
import { logout } from './Login'
import '../App.css';
import Composition from './Composition';
import Visualiser from './Visualiser';
import { Help } from './Help';
import Analysis from './Analysis';
import LightModeIcon from '@mui/icons-material/LightMode';
import DarkModeIcon from '@mui/icons-material/DarkMode';

export function toggleDarkMode() {
  const style = getComputedStyle(document.body);
  ['--col-bg', '--col-frame', '--col-frame-hover',
    '--col-frame-content', '--col-tab-disabled',
    '--col-icon-hover', '--col-fg', '--col-add-button',
  ].forEach((col) => {
    const c = parseInt(style.getPropertyValue(col).slice(1), 16);
    document.documentElement.style.setProperty(col, '#' + (0xfff - c).toString(16));
  })
}

interface IDeposition {
  token: string;
  setToken: React.Dispatch<React.SetStateAction<string | null>>;
  dark_mode: boolean;
  setDarkMode:  React.Dispatch<React.SetStateAction<boolean>>;
}

const Deposition: React.FC<IDeposition> = ({ token, setToken, dark_mode, setDarkMode }) => {
  const [running, setRunning] = useState(false);
  const [failed, setFailed] = useState(false);
  const socket = useRef<WebSocket | null>(null);
  const [socket_connected, setSocketConnected] = useState(false);
  const [last_message, setLastMessage] = useState<MessageEvent<any> | null>(null);

  const [last_frame, setLastFrame] = useState(0);
  const [next_segment, setNextSegment] = useState(1);
  const [particles, setParticles] = useState<Array<Particles>>([]);
  const [wait_for_segment, setWaitForSegment] = useState<boolean>(false);
  const [latest_segment, setLatestSegment] = useState<number>(0);
  const [num_segments, setNumSegments] = useState<number>(0);
  const [sim_done, setSimDone] = useState<boolean>(false);
  const [submit_waiting, setSubmitWaiting] = useState(false);

  const [particles_roughness, setParticlesRoughness] = useState<Particles | null>(null);
  const [roughness_ready, setRoughnessReady] = useState<boolean>(false);
  const [num_bins, setNumBins] = useState<number>(10); // bin size in nm
  const [roughness, setRoughness] = useState<number | null>(null);
  const [mean_height, setMeanHeight] = useState<number | null>(null);
  const [height_map, setHeightMap] = useState<Float32Array | null>(null);
  const [show_height_map, setShowHeightMap] = useState(true);
  const [new_roughness, setNewRoughness] = useState(false);
  const [try_reconnect, setTryReconnect] = useState(false); // State doesn't matter, just flipped every disconnect
  const RECONNECT_DELAY = 5000; // ms == 5 s - delay before trying to re-open websocket
  const RETRY_DELAY = 10000; // ms == 10 s - delay between retrying trajectory segments

  const [current_tab, setCurrentTab] = useState(0);

  useEffect(() => {
    const timer = setInterval(() => {
      if (socket.current?.readyState === WebSocket.CLOSED) {
        console.log("Queing reconnect attempt.");
        setTryReconnect((v) => !v);
      }
    }, RECONNECT_DELAY);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    if (socket.current && socket.current.readyState !== WebSocket.CLOSED) { return }
    let ws_url = window.location.href.replace(new RegExp("^http"), "ws");
    if (!ws_url.endsWith("/")) {
      ws_url += "/"
    }

    socket.current = new WebSocket(ws_url + "socket");
    socket.current.onopen = () => setSocketConnected(true);
    socket.current.onclose = () => {
      setSocketConnected(false)
      setRunning(false);
    };
    socket.current.onmessage = (e) => {setLastMessage(e); return false;}
    const current = socket.current;
    return () => {
      current.close();
    }
  }, [try_reconnect, setTryReconnect]);

  // Process web socket messages
  useEffect(() => {
    if (last_message === null) return;
    setLastMessage(null);
    if (!running) {
      console.log("Unexpected message while not running");
      return;
    }
    if (last_message.data instanceof Blob) {
      if (!wait_for_segment) {
        console.log("Received segment while not waiting for one.");
        return;
      }
      last_message.data.arrayBuffer().then((buf) => {
        const buffer = new DataView(buf);
        const segment_id = buffer.getUint32(0, true);
        if (segment_id !== next_segment) {
          console.log("Expecting segment ", next_segment, ", but received ", segment_id);
          return
        }
        const num_frames    = buffer.getUint32(4, true);
        const num_particles = buffer.getUint32(8, true);
        // console.log("Got new segment:\n\tid: ", segment_id, "\n\tframes: ", num_frames, "\n\tparticles: ", num_particles);
        const types = new Uint8Array(buffer.buffer, 12, num_particles);
        var offset = 12 + num_particles;
        for (let i = 0; i < num_frames; i++) {
          const frame = new Particles(num_particles)
          for (let j = 0; j < num_particles; j += 1) {
            // 12 bytes per particle position
            // Rotate x,y,z -> y,z,x since THREE wants y to be up by default
            frame.add(
              buffer.getFloat32(offset + j*12 + 4, true),
              buffer.getFloat32(offset + j*12 + 8, true),
              buffer.getFloat32(offset + j*12, true),
              types[j],
              types[j]
              )
          }
          offset += 12*num_particles;
          particles.push(frame);
        }
        // console.log("Particles now contains ", particles.length, " frames.");
        setLastFrame((last_frame) => last_frame + num_frames);
        setParticles(particles);
        setWaitForSegment(false);
        setNextSegment(segment_id + 1);
        // Updating next_segment will trigger a check for new segments to download
      }).catch(console.error);

    } else if (last_message.data.startsWith("new_frames")) {
      const packet = JSON.parse(last_message.data.slice(10));
      const latest_segment = packet.l;
      if (num_segments !== packet.f) { setNumSegments(packet.f); }
      if (latest_segment === packet.f) { setSimDone(true); }
      setLatestSegment((prev) => latest_segment > prev ? latest_segment : prev);
      // Updating latest_segment will trigger a check for new segments to download

    } else if (last_message.data.startsWith("no_seg")) {
      const seg = Number.parseInt(last_message.data.slice(6));
      console.log("Segment not available yet: ", seg);
      setWaitForSegment((waiting) => seg === next_segment ? false : waiting);

    } else if (last_message.data === "cancel") {
      setRunning(false);

    } else if (last_message.data === "failed") {
      // console.log("Job failed!");
      setRunning(false);
      setFailed(true);
      // setWaitForSegment(false);
    } else if (last_message.data !== "queued") {
      // queued sent when job has been queued.
      // No need to handle apart from unsetting submit_waiting below.
      console.log("Got unknown message: ", last_message.data);
    }
    setSubmitWaiting(false);
  }, [last_message, setLastMessage,
      running, setRunning,
      wait_for_segment, setWaitForSegment,
      latest_segment, setLatestSegment,
      setLastFrame, setParticles,
      next_segment, setNextSegment,
      num_segments, setNumSegments,
      particles, sim_done,
    ]);

  // up to a new segment, or a new segment is available, so request it.
  useEffect(() => {
    if (!running || !socket.current) { return }
    if (!wait_for_segment && next_segment <= latest_segment) {
      // console.log("Queueing request for next segment: ", next_segment);
      // Wait 0.25s before requesting more frames to avoid laggy rendering from
      // constant refreshes of `particles` when downloading quickly
      setWaitForSegment(true);
      setTimeout(() => {
        socket.current?.send(next_segment.toString());
        console.log("Requested segment ", next_segment);
      }, 250);
    } else if (next_segment > latest_segment) {
      setWaitForSegment(false);
      if (sim_done) {
        setRunning(false);
        setParticlesRoughness(particles[particles.length-1]);
        setRoughnessReady(true);
        setCurrentTab(1);
      }
    }
  }, [next_segment, latest_segment, sim_done, socket,
      running, wait_for_segment, setWaitForSegment,
      setRoughnessReady, setRunning,
      setParticlesRoughness, setCurrentTab, particles])

  // Retry request for next segment in case something de-synced.
  // Probably a better way to handle this...
  useEffect(() => {
    if (!running || !socket.current || !wait_for_segment) { return }
    const retry = setInterval(() => {
      socket.current?.send(next_segment.toString());
      console.log("Retrying segment ", next_segment);
    }, RETRY_DELAY)
    return () => clearInterval(retry);
  }, [socket, next_segment, running, wait_for_segment])

  const [status_text, setStatusText] = useState("Idle");
  useEffect(() => {
    if (!socket_connected) {
      setStatusText("Disconnected! Try refreshing the page.");
    } else if (submit_waiting) {
      setStatusText("Submitting");
    } else if (failed) {
      setStatusText("Simulation failed! Try a different configuration.");
    } else if (running) {
      if (num_segments > 0) {
        if (latest_segment < num_segments) {
          setStatusText("Running step " + (latest_segment + 1) + " of " + num_segments);
        } else {
          setStatusText("Complete (downloading step " + next_segment + " of " + num_segments + ")");
        }
      } else {
        setStatusText("In Queue");
      }
    } else if (roughness_ready) {
      setStatusText("Complete");
    } else {
      setStatusText("Idle");
    }
  }, [submit_waiting, failed, running, next_segment, latest_segment, num_segments, roughness_ready, socket_connected]);

  const tabs = [
    {
      name: "Simulation",
      enable: true,
      content:
        <Composition
          socket={socket} socket_connected={socket_connected}
          running={running} setRunning={setRunning}
          submit_waiting={submit_waiting} setSubmitWaiting={setSubmitWaiting}
          resetTrajectory={() => {
            // console.log("Resetting trajectory");
            particles.map((p) => p.dispose());
            particles.length = 0;
            setSimDone(false);
            setFailed(false);
            setNextSegment(1);
            setLatestSegment(0);
            setNumSegments(0);
            setLastFrame(0);
            setWaitForSegment(false);
            setParticles(particles);
            setRoughness(null);
            setMeanHeight(null);
            setHeightMap(null);
            setRoughnessReady(false);
          }}
        />
      },
      {
        name: "Analysis",
        enable: roughness_ready && particles_roughness,
        content:
          <Analysis
            particles={particles_roughness}
            num_bins={num_bins} setNumBins={setNumBins}
            roughness={roughness} setRoughness={setRoughness}
            mean_height={mean_height} setMeanHeight={setMeanHeight}
            setHeightMap={setHeightMap} setShowHeightMap={setShowHeightMap}
            setNewRoughness={setNewRoughness}
          />
    },
    {
      name: "Help",
      enable: true,
      content: <Help/>
    },
  ];

  return (
    <>
      <div className="App">
        <div className="App-header">
          <div className="header-text">
            <b>Vacuum Deposition</b>
          </div>
          <div className="header-button-container">
            <div className="header-button"
                onClick={() => {
                  toggleDarkMode();
                  setDarkMode((d) => !d);
                }}
                title="Toggle dark/light mode"
            >
              {dark_mode ? <LightModeIcon/> : <DarkModeIcon/>}
            </div>
            <div className="header-button"
                onClick={() => {
                  logout({ token });
                  setToken(null);
                }}
            >
              Sign Out ({JSON.parse(token).token})
            </div>
          </div>
        </div>
        <div className="view-container">
          <div id="resize-container" className="resize-container">
          <div className="tab-container">
            <div className="tab-buttons">
              { tabs.map((tab, i) => { return (
                <button className={"tab-button" +
                  (i === current_tab ? " tab-button-selected" : "")}
                  onClick={() => setCurrentTab(i)}
                  disabled={!tab.enable}
                >
                  <b>{tab.name}</b>
                </button>)
              })}
            </div>
            { tabs.length === 0 ? null : tabs.map((tab, i) => {
              return <div className="MD-params"
                style={{display: i === current_tab ? 'flex' : 'none' }}
              >
                {tab.content}
              </div>
              })
            }
          </div>
          <div className="MD-vis-resize"
            onMouseDown={(e) => {
              const params = document.getElementById("resize-container");
              if (!params) { return };
              const drag_save = {e: e, old_width: params.offsetWidth};
              document.onmousemove = (e) => {
                const delta = e.clientX - drag_save.e.clientX;
                params.style.width = Math.min(
                  Math.max(drag_save.old_width + delta, 0),
                  document.documentElement.offsetWidth
                ).toString() + "px";
              }
              document.onmouseup = () => {
                document.onmousemove = document.onmouseup = null;
              }
            }}
          >
          </div>
          </div>
          <div className="vis-container">
            <Visualiser
              particles={particles} num_frames={last_frame}
              height_map={height_map} show_height_map={show_height_map}
              num_bins={num_bins} mean_height={mean_height}
              roughness={roughness} new_roughness={new_roughness}
              setNewRoughness={setNewRoughness}
              status_text={status_text}
            />
          </div>
        </div>
      </div>
    </>
  );
}

export default Deposition;
