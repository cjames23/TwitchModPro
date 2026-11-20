import { useEffect, useState } from "react";
import type { ModChannelOption } from "../types";
import { api } from "../api";

interface Props {
  onAdd: (login: string) => Promise<void>;
  onClose: () => void;
}

/** Lists only the channels Twitch says you moderate. */
export function ChannelPicker({ onAdd, onClose }: Props) {
  const [options, setOptions] = useState<ModChannelOption[] | null>(null);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [adding, setAdding] = useState<string | null>(null);

  async function load(refresh: boolean) {
    setLoading(true);
    setError(null);
    try {
      setOptions(await api.moderatedChannels(refresh));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load(false);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const q = query.trim().toLowerCase().replace(/^#/, "");
  const shown = (options ?? []).filter((o) => !q || o.login.includes(q) || o.name.toLowerCase().includes(q));

  async function add(login: string) {
    setAdding(login);
    setError(null);
    try {
      await onAdd(login);
      setOptions((opts) => opts?.map((o) => (o.login === login ? { ...o, added: true } : o)) ?? null);
    } catch (e) {
      setError(String(e));
    } finally {
      setAdding(null);
    }
  }

  return (
    <div className="picker" role="dialog" aria-label="Add a channel">
      <div className="picker-head">
        <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search your channels…" autoFocus />
        <button className="ghost small" onClick={() => load(true)} disabled={loading} title="Ask Twitch again">
          ↻
        </button>
        <button className="ghost small" onClick={onClose} aria-label="Close">
          ×
        </button>
      </div>
      <div className="picker-list">
        {loading && !options && <div className="muted-text pad">Loading the channels you moderate…</div>}
        {options && shown.length === 0 && (
          <div className="muted-text pad">
            {q ? `You don't moderate a channel matching “${q}”.` : "Twitch didn't return any channels you moderate."}
          </div>
        )}
        {shown.map((o) => (
          <div key={o.login} className="picker-row">
            <span className="channel-name">
              {o.name}
              {o.own && <span className="muted-text"> (you)</span>}
            </span>
            {o.added ? (
              <span className="muted-text">Added</span>
            ) : (
              <button className="small" disabled={adding !== null} onClick={() => add(o.login)}>
                {adding === o.login ? "Adding…" : "Add"}
              </button>
            )}
          </div>
        ))}
      </div>
      {error && <div className="form-error pad">{error}</div>}
      <div className="picker-foot muted-text">Only channels where you're a moderator (or your own) can be monitored.</div>
    </div>
  );
}
