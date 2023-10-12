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
    <div className="App">
      <div className="login-bg">
        <div className="login-panel">
          <form className="login-form" onSubmit={handleSubmit}>
            <b style={{
              fontSize: '25pt',
              verticalAlign: 'center',
              margin: '0',
              marginBottom: '20pt',
              textAlign: 'center',
            }}>Vacuum Deposition</b>
            <input placeholder="Username" type="text"
              onChange={e => setUsername(e.target.value)}
              autoFocus={true}
            />
            <input placeholder="Password"
              type="password"
              onChange={e => setPassword(e.target.value)}
            />
            <button className="submit-button roughness" type="submit">Login</button>
          </form>
          <div className="login-fail" style={{display: loginFailed ? 'flex' : 'none'}}>
            Incorrect username or password!
          </div>
        </div>
      </div>
    </div>
  );
}

export default Login;
