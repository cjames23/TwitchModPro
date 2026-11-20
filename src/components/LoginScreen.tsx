import { useEffect, useState } from "react";
import type { AuthState } from "../types";
import { api } from "../api";

interface Props {
  auth: AuthState;
  onAuth: (a: AuthState) => void;
}

export function LoginScreen({ auth, onAuth }: Props) {
  const [clientId, setClientId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (!auth.login) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [auth.login]);

  async function run(fn: () => Promise<AuthState>) {
    setBusy(true);
    setError(null);
    try {
      onAuth(await fn());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function login() {
    await run(async () => {
      const a = await api.startLogin();
      if (a.login) api.open(a.login.verificationUri);
      return a;
    });
  }

  const shownError = error ?? auth.error;

  if (auth.checking) {
    return <div className="boot">Checking your Twitch login…</div>;
  }

  return (
    <div className="login-wrap">
      <div className="login-card">
        <div className="brand big">
          <span className="brand-mark">◆</span> Mod Pro
        </div>
        <p className="muted-text">
          Log in with Twitch so the app can see which channels you moderate. You can only monitor those channels, and
          your login is also used to delete messages, time out and ban.
        </p>

        {!auth.clientIdSet ? (
          <>
            <h3>Step 1: connect a Twitch application</h3>
            <ol className="steps">
              <li>
                Open the{" "}
                <a href="#" onClick={(e) => (e.preventDefault(), api.open("https://dev.twitch.tv/console/apps/create"))}>
                  Twitch developer console
                </a>{" "}
                and register an application.
              </li>
              <li>
                Use any name, set the OAuth Redirect URL to <code>http://localhost</code>, pick the category{" "}
                <em>Application Integration</em>, and set the Client Type to <strong>Public</strong>.
              </li>
              <li>Copy the application's Client ID and paste it below.</li>
            </ol>
            <form
              className="row-gap"
              onSubmit={(e) => {
                e.preventDefault();
                run(() => api.setClientId(clientId));
              }}
            >
              <input
                className="grow"
                value={clientId}
                onChange={(e) => setClientId(e.target.value)}
                placeholder="Client ID"
                spellCheck={false}
                autoFocus
              />
              <button className="primary" disabled={busy || !clientId.trim()}>
                Save
              </button>
            </form>
          </>
        ) : auth.login ? (
          <>
            <h3>Approve the login on Twitch</h3>
            <p>A Twitch page opened in your browser. Check that it shows this code, then click Authorize:</p>
            <div className="device-code">{auth.login.userCode}</div>
            <p className="muted-text">
              Waiting for approval… (code expires in {Math.max(0, Math.round((auth.login.expiresAt - now) / 60000))} min)
            </p>
            <div className="row-gap">
              <button onClick={() => api.open(auth.login!.verificationUri)}>Open the Twitch page again</button>
              <button className="ghost" onClick={() => run(api.cancelLogin)}>
                Cancel
              </button>
            </div>
          </>
        ) : (
          <>
            <button className="primary big-btn" onClick={login} disabled={busy}>
              Log in with Twitch
            </button>
            {!auth.clientIdFromBuild && (
              <button className="ghost small" onClick={() => run(() => api.setClientId(""))} disabled={busy}>
                Use a different Client ID
              </button>
            )}
          </>
        )}

        {shownError && <div className="form-error">{shownError}</div>}
      </div>
    </div>
  );
}
