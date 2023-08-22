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
            <input placeholder="Username" type="text" onChange={e => setUsername(e.target.value)} />
            <br/>
            <input placeholder="Password" type="password" onChange={e => setPassword(e.target.value)} />
            <br/>
            <button type="submit">Login</button>
          </form>
          <p>
            {loginFailed ? "Incorrect username or password!" : ""}
          </p>
        </div>
      </div>
    </>
  );
}

export default Login;
