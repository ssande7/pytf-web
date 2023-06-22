import React, { useState } from 'react';
import '../App.css';



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
  const [loginFailed, setLoginFailed] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    async function login(credentials: any) {
        return fetch("/login", {
          method: "post",
          headers: {
            'Content-Type': 'application/json'
          },
          body: JSON.stringify(credentials)
        }).then(data => {
          if (data.ok) {
            setLoginFailed(false);
            return data.json();
          }
          setLoginFailed(true);
          return null
        });
    }

    e.preventDefault();
    const token = await login({username, password});
    if (token) {
      setToken(JSON.stringify(token));
    }
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
              <input type="password" onChange={e => setPassword(e.target.value)} />
            </label>
            <div>
              {loginFailed ? "Incorrect username or password!" : ""}
            </div>
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
