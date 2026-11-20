import { useEffect, useMemo, useRef, useState } from "react";
import "./App.css";
import { api, subscribe } from "./api";
import type { Channel, ConnectionInfo, Flag, FlagStatus, LiveMessage, Raid, Rule, Settings } from "./types";
import { playTone } from "./util";
import { Sidebar } from "./components/Sidebar";
import { FlagQueue } from "./components/FlagQueue";
import { LiveChats } from "./components/LiveChats";
import { RaidBanner, RaidLog } from "./components/Raids";
import { Rules } from "./components/Rules";
import { SettingsPanel } from "./components/SettingsPanel";

type Tab = "flags" | "chat" | "raids" | "rules" | "settings";
const MAX_FLAGS = 500;
const MAX_RAIDS = 200;
const MAX_CHAT_PER_CHANNEL = 200;

interface Toast {
  id: number;
  text: string;
}

export default function App() {
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("flags");
  const [channels, setChannels] = useState<Channel[]>([]);
  const [rules, setRules] = useState<Rule[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [flags, setFlags] = useState<Flag[]>([]);
  const [raids, setRaids] = useState<Raid[]>([]);
  const [connection, setConnection] = useState<ConnectionInfo>({ status: "connecting", joined: [] });
  const [chat, setChat] = useState<Record<string, LiveMessage[]>>({});
  const [selected, setSelected] = useState<string | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [now, setNow] = useState(Date.now());

  const settingsRef = useRef<Settings | null>(null);
  settingsRef.current = settings;

  function toast(text: string) {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t.slice(-3), { id, text }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 6000);
  }

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      // Subscribe first so nothing that happens during the snapshot is missed.
      const off = await subscribe({
        onFlag: ({ flag, alert }) => {
          setFlags((fs) => (fs.some((f) => f.id === flag.id) ? fs : [flag, ...fs].slice(0, MAX_FLAGS)));
          if (alert && settingsRef.current?.playSounds) playTone("flag");
        },
        onRaid: ({ raid, alert }) => {
          setRaids((rs) => (rs.some((r) => r.id === raid.id) ? rs : [raid, ...rs].slice(0, MAX_RAIDS)));
          if (alert && settingsRef.current?.playSounds) playTone("raid");
        },
        onChat: (batch) =>
          setChat((prev) => {
            const next = { ...prev };
            for (const m of batch) {
              const list = next[m.channel] === prev[m.channel] ? [...(prev[m.channel] ?? [])] : next[m.channel];
              list.push(m);
              next[m.channel] = list.length > MAX_CHAT_PER_CHANNEL ? list.slice(-MAX_CHAT_PER_CHANNEL) : list;
            }
            return next;
          }),
        onConnection: setConnection,
        onNotice: ({ channel, text }) => toast(channel ? `#${channel}: ${text}` : text),
      });
      // If the effect was torn down while we were subscribing, don't leak the listeners.
      if (cancelled) {
        off();
        return;
      }
      unlisten = off;
      try {
        const snap = await api.snapshot();
        if (cancelled) return;
        setChannels(snap.channels);
        setRules(snap.rules);
        setSettings(snap.settings);
        setFlags(snap.flags);
        setRaids(snap.raids);
        setConnection(snap.connection);
        setLoaded(true);
      } catch (e) {
        setLoadError(String(e));
      }
    })();
    const tick = setInterval(() => setNow(Date.now()), 15000);
    return () => {
      cancelled = true;
      unlisten?.();
      clearInterval(tick);
    };
  }, []);

  const openFlagCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const f of flags) if (f.status === "open") counts[f.channel] = (counts[f.channel] ?? 0) + 1;
    return counts;
  }, [flags]);
  const openFlagTotal = Object.values(openFlagCounts).reduce((a, b) => a + b, 0);
  const unackedRaids = raids.filter((r) => !r.acknowledged).length;

  async function setFlagStatus(id: number, status: FlagStatus) {
    setFlags((fs) => fs.map((f) => (f.id === id ? { ...f, status } : f)));
    try {
      await api.setFlagStatus(id, status);
    } catch (e) {
      toast(String(e));
    }
  }

  async function ackRaids() {
    setRaids((rs) => rs.map((r) => ({ ...r, acknowledged: true })));
    await api.ackRaids().catch((e) => toast(String(e)));
  }

  async function updateSettings(s: Settings) {
    setSettings(s);
    try {
      setSettings(await api.saveSettings(s));
    } catch (e) {
      toast(String(e));
    }
  }

  if (loadError) {
    return <div className="boot-error">Couldn't start: {loadError}</div>;
  }
  if (!loaded || !settings) {
    return <div className="boot">Loading…</div>;
  }

  const tabs: { id: Tab; label: string; count?: number }[] = [
    { id: "flags", label: "Flags", count: openFlagTotal },
    { id: "chat", label: "Live chat" },
    { id: "raids", label: "Raids", count: unackedRaids },
    { id: "rules", label: "Rules" },
    { id: "settings", label: "Alerts" },
  ];

  return (
    <div className="app">
      <Sidebar
        channels={channels}
        connection={connection}
        openFlagCounts={openFlagCounts}
        selected={selected}
        onSelect={setSelected}
        onAdd={async (name) => setChannels(await api.addChannel(name))}
        onRemove={async (name) => {
          setChannels(await api.removeChannel(name));
          if (selected === name) setSelected(null);
          setChat((c) => {
            const { [name]: _removed, ...rest } = c;
            return rest;
          });
        }}
        onToggleMute={async (name, muted) => setChannels(await api.setChannelMuted(name, muted))}
      />

      <main className="main">
        <RaidBanner raids={raids} onDismiss={ackRaids} />
        <nav className="tabs">
          {tabs.map((t) => (
            <button key={t.id} className={tab === t.id ? "on" : ""} onClick={() => setTab(t.id)}>
              {t.label}
              {!!t.count && <span className={`tab-count ${t.id}`}>{t.count}</span>}
            </button>
          ))}
        </nav>

        <div className="content">
          {tab === "flags" && (
            <FlagQueue
              flags={flags}
              channelFilter={selected}
              now={now}
              onStatus={setFlagStatus}
              onClearClosed={async () => setFlags(await api.clearClosedFlags())}
            />
          )}
          {tab === "chat" && <LiveChats channels={channels} messages={chat} channelFilter={selected} />}
          {tab === "raids" && <RaidLog raids={raids} channelFilter={selected} now={now} />}
          {tab === "rules" && <Rules rules={rules} channels={channels} onRulesChanged={setRules} />}
          {tab === "settings" && <SettingsPanel settings={settings} onChange={updateSettings} />}
        </div>
      </main>

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className="toast">
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}
