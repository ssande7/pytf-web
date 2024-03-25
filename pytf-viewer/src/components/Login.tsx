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
      setLoginFailed(false);
      return fetch("/login", {
        method: "post",
        headers: {
          'Content-Type': 'application/json'
        },
        body: JSON.stringify({username: credentials.username, password: credentials.password})
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
              marginBottom: '20px',
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
            <button className="submit-button login" type="submit" color="var(--col-smiles-bg)">Sign in</button>
            <input type="hidden" name="login" value="login"/>
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
