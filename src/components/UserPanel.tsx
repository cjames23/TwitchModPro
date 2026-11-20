import { useEffect, useState } from "react";
import type { UserHistory } from "../types";
import { api } from "../api";
import { channelColor, clockTime, timeAgo } from "../util";
import { accountAge, actionLabel, isNewAccount } from "../intel";
import { ModActions, type ActionHandlers } from "./ModActions";

interface Props {
  channel: string;
  login: string;
  /** Changes whenever a mod action happens, so the panel reloads. */
  refreshKey: number;
  actions: ActionHandlers;
  modChannelCount: number;
  onClose: () => void;
}

export function UserPanel({ channel, login, refreshKey, actions, modChannelCount, onClose }: Props) {
  const [data, setData] = useState<UserHistory | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .userHistory(channel, login)
      .then((d) => live && setData(d))
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [channel, login, refreshKey]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const target = { channel, userLogin: login, userId: data?.userId ?? null };

  return (
    <>
      <div className="drawer-backdrop" onClick={onClose} />
      <aside className="drawer" role="dialog" aria-label={`History for ${login}`}>
        <header className="drawer-head">
          <div>
            <h2>{data?.displayName ?? login}</h2>
            <div className="muted-text">
              in <span style={{ color: channelColor(channel) }}>#{channel}</span>
              {" · "}
              <a href="#" onClick={(e) => (e.preventDefault(), api.openChannel(login))}>
                Twitch profile ↗
              </a>
            </div>
            {data?.accountCreatedAt && (
              <div className={`account-age ${isNewAccount(data.accountCreatedAt, Date.now()) ? "warn" : ""}`}>
                Account {accountAge(data.accountCreatedAt, Date.now())} (created{" "}
                {new Date(data.accountCreatedAt).toLocaleDateString()})
              </div>
            )}
          </div>
          <button className="ghost" onClick={onClose} aria-label="Close">
            ×
          </button>
        </header>

        <ModActions target={target} actions={actions} showUnban modChannelCount={modChannelCount} />

        {error && <div className="form-error">{error}</div>}
        {data && (
          <div className="drawer-body">
            <section>
              <h3>Recent messages</h3>
              {data.messages.length === 0 && (
                <p className="muted-text">No messages seen from this user since the app started.</p>
              )}
              {data.messages.map((m) => (
                <div key={m.id} className={`history-line ${m.flagSeverity ? `flagged sev-${m.flagSeverity}` : ""}`}>
                  <span className="ctx-time">{clockTime(m.timestamp)}</span>
                  <span className="msg-text">{m.text}</span>
                  <button
                    className="ghost small"
                    title="Delete this message"
                    onClick={() => actions.deleteMessage({ ...target, messageId: m.id })}
                  >
                    Delete
                  </button>
                </div>
              ))}
            </section>

            <section>
              <h3>Flags ({data.flags.length})</h3>
              {data.flags.length === 0 && <p className="muted-text">No flags for this user.</p>}
              {data.flags.map((f) => (
                <div key={f.id} className="history-line">
                  <span className="ctx-time">{timeAgo(f.timestamp)}</span>
                  <span className={`sev-badge ${f.severity}`}>{f.severity}</span>
                  <span className="msg-text">{f.text}</span>
                  <span className="muted-text">{f.resolution ?? f.status}</span>
                </div>
              ))}
            </section>

            <section>
              <h3>Mod actions in #{channel}</h3>
              {data.actions.length === 0 && <p className="muted-text">None yet.</p>}
              {data.actions.map((a) => (
                <div key={a.id} className={`history-line ${a.ok ? "" : "failed"}`}>
                  <span className="ctx-time">{timeAgo(a.timestamp)}</span>
                  <span>
                    {actionLabel(a)}
                    {a.reason && <span className="muted-text"> ({a.reason})</span>}
                    {!a.byApp && <span className="muted-text"> by a mod</span>}
                  </span>
                  {!a.ok && <span className="form-error">Failed: {a.error}</span>}
                </div>
              ))}
            </section>

            <section>
              <h3>In your other channels</h3>
              {data.otherChannels.length === 0 && <p className="muted-text">No warnings, timeouts or bans elsewhere.</p>}
              {data.otherChannels.map((a) => (
                <div key={a.id} className="history-line">
                  <span className="ctx-time">{timeAgo(a.timestamp)}</span>
                  <span>
                    {actionLabel(a)} in <span style={{ color: channelColor(a.channel) }}>#{a.channel}</span>
                    {!a.byApp && <span className="muted-text"> by a mod</span>}
                  </span>
                </div>
              ))}
            </section>
          </div>
        )}
      </aside>
    </>
  );
}
