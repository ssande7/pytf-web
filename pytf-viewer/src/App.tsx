import React, { useEffect, useState } from 'react';
import './App.css';
import Viewer from './components/Viewer';
import Login from './components/Login';

const App: React.FC = () => {
  const [token, setToken] = useState<string | null>(null);

  // Fetch token from server in case we're already logged in
  // TODO: The login page displays briefly while this runs.
  //       Should add a loading page which either jumps to
  //       login if nothing cached, or loads directly to Viewer.
  useEffect(() => {
    async function check_cached_token() {
      try {
        const cached = await fetch("/user-token", {
          method: "post",
        }).then(data => data.json());
        setToken(JSON.stringify(cached));
      } catch (_) {
        // Can add setCheckedIdentity here and make default return the loading screen
      }
    }
    if (!token) {
      check_cached_token();
    }
  }, [])

  if (!token) {
    return (<Login setToken={setToken} />);
  }
  return (<Viewer token={token} setToken={setToken}/>);
}

export default App;
