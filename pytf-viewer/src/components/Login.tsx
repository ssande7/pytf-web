import React, { useState } from 'react';
import '../App.css';



async function login(credentials: any) {
    return fetch("/login", {
      method: "post",
      headers: {
        'Content-Type': 'application/json'
      },
      body: JSON.stringify(credentials)
    }).then(data => data.json());
}

export async function logout(token: any) {
    return fetch("/logout", {
      method: "post",
      headers: {
        'Content-Type': 'application/json'
      },
      body: JSON.stringify(token)
    }).then(data => data.json());
}

interface ILogin {
  setToken: React.Dispatch<React.SetStateAction<string | null>>;
}
const Login: React.FC<ILogin> = ({ setToken }) => {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const token = await login({username, password});
    setToken(JSON.stringify(token));
  }

  return (
    <>
      <div className="App">
        <div className="App-header">
          <h1>Vacuum Deposition</h1>
          <form onSubmit={handleSubmit}>
            <label>
              <p>Username</p>
              <input type="text" onChange={e => setUsername(e.target.value)} />
            </label>
            <label>
              <p>Password</p>
              <input type="text" onChange={e => setPassword(e.target.value)} />
            </label>
            <div>
              <button type="submit">Login</button>
            </div>
          </form>
        </div>
      </div>
    </>
  );
}

export default Login;
