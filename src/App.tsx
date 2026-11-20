import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import "./App.css";
import { api, subscribe } from "./api";
import type {
  ActionTarget,
  AuthState,
  Channel,
  ConnectionInfo,
  Flag,
  FlagStatus,
  LiveMessage,
  ModAction,
  Raid,
  RaidModes,
  Rule,
  Settings,
  Wave,
} from "./types";
import { playTone } from "./util";
import { fmtSecs, indexSanctions } from "./intel";
import { Sidebar } from "./components/Sidebar";
import { FlagQueue } from "./components/FlagQueue";
import { LiveChats } from "./components/LiveChats";
import { RaidBanner, RaidLog } from "./components/Raids";
import { Rules } from "./components/Rules";
import { SettingsPanel } from "./components/SettingsPanel";
import { Tuning } from "./components/Tuning";
import { LoginScreen } from "./components/LoginScreen";
import { UserPanel } from "./components/UserPanel";
import type { ActionHandlers } from "./components/ModActions";
import type { WaveHandlers } from "./components/WaveCard";
import type { InboxHandlers } from "./components/FlagQueue";

type Tab = "flags" | "chat" | "raids" | "rules" | "tuning" | "settings";
const MAX_FLAGS = 500;
const MAX_RAIDS = 200;
const MAX_CHAT_PER_CHANNEL = 200;

interface Toast {
  id: number;
  text: string;
  kind: "info" | "ok" | "error";
}

const SANCTION_ACTIONS = new Set(["warn", "timeout", "ban", "unban"]);

export default function App() {
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("flags");
  const [auth, setAuth] = useState<AuthState | null>(null);
  const [channels, setChannels] = useState<Channel[]>([]);
  const [rules, setRules] = useState<Rule[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [flags, setFlags] = useState<Flag[]>([]);
  const [raids, setRaids] = useState<Raid[]>([]);
  const [raidModes, setRaidModes] = useState<RaidModes>({});
  const [connection, setConnection] = useState<ConnectionInfo>({ status: "connecting", joined: [] });
  const [chat, setChat] = useState<Record<string, LiveMessage[]>>({});
  const [selected, setSelected] = useState<string | null>(null);
  const [userPanel, setUserPanel] = useState<{ channel: string; login: string } | null>(null);
  const [actionCount, setActionCount] = useState(0);
  const [waves, setWaves] = useState<Wave[]>([]);
  const [sanctions, setSanctions] = useState<ModAction[]>([]);
  const [ages, setAges] = useState<Record<string, number>>({});
  const [deleted, setDeleted] = useState<Set<string>>(new Set());
  const [inboxConnected, setInboxConnected] = useState<boolean | null>(null);
  const [editRuleId, setEditRuleId] = useState<number | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [now, setNow] = useState(Date.now());

  const settingsRef = useRef<Settings | null>(null);
  settingsRef.current = settings;

  const toast = useCallback((text: string, kind: Toast["kind"] = "info") => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t.slice(-3), { id, text, kind }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), kind === "error" ? 9000 : 5000);
  }, []);

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
        onFlagContext: ({ id, line }) =>
          setFlags((fs) => fs.map((f) => (f.id === id ? { ...f, contextAfter: [...f.contextAfter, line] } : f))),
        onFlagsResolved: ({ ids, resolution }) =>
          setFlags((fs) => fs.map((f) => (ids.includes(f.id) ? { ...f, status: "reviewed", resolution } : f))),
        onRaid: ({ raid, alert }) => {
          setRaids((rs) => (rs.some((r) => r.id === raid.id) ? rs : [raid, ...rs].slice(0, MAX_RAIDS)));
          if (alert && settingsRef.current?.playSounds) playTone("raid");
        },
        onRaidMode: setRaidModes,
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
        onChannels: setChannels,
        onAuth: setAuth,
        onModAction: (a) => {
          setActionCount((n) => n + 1);
          if (a.ok && SANCTION_ACTIONS.has(a.action)) setSanctions((s) => (s.some((x) => x.id === a.id) ? s : [a, ...s]));
        },
        onWave: ({ wave, new: isNew }) => {
          setWaves((ws) => [wave, ...ws.filter((w) => w.id !== wave.id)].sort((a, b) => b.lastSeen - a.lastSeen));
          if (isNew && settingsRef.current?.playSounds) playTone("raid");
        },
        onAccountAges: (a) => setAges((prev) => ({ ...prev, ...a })),
        onMessageDeleted: ({ messageId }) => setDeleted((d) => new Set(d).add(messageId)),
        onInboxStatus: ({ connected }) => setInboxConnected(connected),
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
        setAuth(snap.auth);
        setRaidModes(snap.raidModes);
        setWaves(snap.waves);
        setSanctions(snap.sanctions);
        setLoaded(true);
        // Account ages for open flags; unknown ones arrive later as events.
        const logins = [...new Set(snap.flags.filter((f) => f.status === "open").map((f) => f.userLogin))];
        if (logins.length) api.accountAges(logins).then((a) => setAges((prev) => ({ ...a, ...prev }))).catch(() => {});
      } catch (e) {
        setLoadError(String(e));
      }
    })();
    const tick = setInterval(() => setNow(Date.now()), 1000);
    return () => {
      cancelled = true;
      unlisten?.();
      clearInterval(tick);
    };
  }, [toast]);

  const openFlagCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const f of flags) if (f.status === "open") counts[f.channel] = (counts[f.channel] ?? 0) + 1;
    return counts;
  }, [flags]);
  const openFlagTotal = Object.values(openFlagCounts).reduce((a, b) => a + b, 0);
  const unackedRaids = raids.filter((r) => !r.acknowledged).length;

  async function setFlagStatus(id: number, status: FlagStatus) {
    setFlags((fs) => fs.map((f) => (f.id === id ? { ...f, status, resolution: status === "open" ? null : f.resolution } : f)));
    try {
      await api.setFlagStatus(id, status);
    } catch (e) {
      toast(String(e), "error");
    }
  }

  async function ackRaids() {
    setRaids((rs) => rs.map((r) => ({ ...r, acknowledged: true })));
    await api.ackRaids().catch((e) => toast(String(e), "error"));
  }

  async function updateSettings(s: Settings) {
    setSettings(s);
    try {
      setSettings(await api.saveSettings(s));
    } catch (e) {
      toast(String(e), "error");
    }
  }

  /** Run a mod action and report the result as a toast. Errors are shown, not thrown. */
  async function act(label: string, fn: () => Promise<unknown>) {
    try {
      await fn();
      toast(label, "ok");
    } catch (e) {
      toast(String(e), "error");
    }
  }

  const actions: ActionHandlers = {
    warn: (t, reason) => act(`Warned ${t.userLogin} in #${t.channel}`, () => api.warn(t.channel, t.userLogin, t.userId, reason)),
    banEverywhere: async (t, reason) => {
      try {
        const results = await api.banEverywhere(t.userLogin, t.userId, reason);
        const ok = results.filter((r) => r.ok).length;
        const failed = results.filter((r) => !r.ok);
        toast(
          `Banned ${t.userLogin} in ${ok} of ${results.length} channels` +
            (failed.length ? `. Failed: ${failed.map((f) => `#${f.channel} (${f.error})`).join(", ")}` : ""),
          failed.length ? "error" : "ok",
        );
      } catch (e) {
        toast(String(e), "error");
      }
    },
    deleteMessage: (t: ActionTarget) =>
      act(`Deleted ${t.userLogin}'s message in #${t.channel}`, () => api.deleteMessage(t.channel, t.userLogin, t.messageId!)),
    timeout: (t, secs, reason) =>
      act(`Timed out ${t.userLogin} for ${fmtSecs(secs)} in #${t.channel}`, () =>
        api.timeout(t.channel, t.userLogin, t.userId, secs, reason),
      ),
    ban: (t, reason) => act(`Banned ${t.userLogin} from #${t.channel}`, () => api.ban(t.channel, t.userLogin, t.userId, reason)),
    unban: (t) => act(`Unbanned ${t.userLogin} in #${t.channel}`, () => api.unban(t.channel, t.userLogin, t.userId)),
  };

  const waveHandlers: WaveHandlers = {
    act: async (wave, action, seconds, reason, exclude) => {
      try {
        const r = await api.waveAction(wave.id, action, seconds, reason, exclude);
        if (r.wave) setWaves((ws) => ws.map((w) => (w.id === r.wave!.id ? r.wave! : w)));
        const verb = action === "ban" ? "Banned" : "Timed out";
        toast(
          `${verb} ${r.succeeded} account${r.succeeded === 1 ? "" : "s"}` + (r.failed.length ? `, ${r.failed.length} failed` : ""),
          r.failed.length ? "error" : "ok",
        );
      } catch (e) {
        toast(String(e), "error");
      }
    },
    undo: async (wave) => {
      try {
        const r = await api.waveUndo(wave.id);
        if (r.wave) setWaves((ws) => ws.map((w) => (w.id === r.wave!.id ? r.wave! : w)));
        toast(`Unbanned ${r.succeeded}` + (r.failed.length ? `, ${r.failed.length} failed` : ""), r.failed.length ? "error" : "ok");
      } catch (e) {
        toast(String(e), "error");
      }
    },
    dismiss: async (wave) => {
      try {
        const w = await api.dismissWave(wave.id);
        setWaves((ws) => ws.map((x) => (x.id === w.id ? w : x)));
      } catch (e) {
        toast(String(e), "error");
      }
    },
  };

  const inbox: InboxHandlers = {
    automod: (f, allow) =>
      act(`${allow ? "Approved" : "Denied"} ${f.userLogin}'s message in #${f.channel}`, () => api.automodDecide(f.id, allow)),
    unbanRequest: (f, approve, text) =>
      act(`${approve ? "Unbanned" : "Denied"} ${f.userLogin} in #${f.channel}`, () => api.unbanRequestDecide(f.id, approve, text)),
  };

  const byLogin = useMemo(() => indexSanctions(sanctions), [sanctions]);
  const openWaves = waves.filter((w) => w.status === "open").length;

  const openUser = useCallback((channel: string, login: string) => setUserPanel({ channel, login }), []);

  if (loadError) {
    return <div className="boot-error">Couldn't start: {loadError}</div>;
  }
  if (!loaded || !settings || !auth) {
    return <div className="boot">Loading…</div>;
  }
  if (!auth.account) {
    return <LoginScreen auth={auth} onAuth={setAuth} />;
  }

  const tabs: { id: Tab; label: string; count?: number }[] = [
    { id: "flags", label: "Flags", count: openFlagTotal + openWaves },
    { id: "chat", label: "Live chat" },
    { id: "raids", label: "Raids", count: unackedRaids },
    { id: "rules", label: "Rules" },
    { id: "tuning", label: "Tuning" },
    { id: "settings", label: "Alerts" },
  ];

  return (
    <div className="app">
      <Sidebar
        account={auth.account}
        channels={channels}
        connection={connection}
        openFlagCounts={openFlagCounts}
        raidModes={raidModes}
        raidModeMinutes={settings.raidModeMinutes}
        now={now}
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
        onRaidMode={async (name, minutes) => setRaidModes(await api.setRaidMode(name, minutes))}
        onLogout={async () => setAuth(await api.logout())}
        inboxConnected={inboxConnected}
      />

      <main className="main">
        <RaidBanner raids={raids} onDismiss={ackRaids} />
        <nav className="tabs">
          {tabs.map((t) => (
            <button
              key={t.id}
              className={tab === t.id ? "on" : ""}
              onClick={() => {
                setTab(t.id);
                if (t.id !== "rules") setEditRuleId(null);
              }}
            >
              {t.label}
              {!!t.count && <span className={`tab-count ${t.id}`}>{t.count}</span>}
            </button>
          ))}
        </nav>

        <div className="content">
          {tab === "flags" && (
            <FlagQueue
              flags={flags}
              waves={waves}
              channels={channels}
              channelFilter={selected}
              now={now}
              actions={actions}
              inbox={inbox}
              waveHandlers={waveHandlers}
              intel={{ byLogin, ages, ladder: settings.ladder, lookbackDays: settings.ladderLookbackDays }}
              keyboardEnabled={!userPanel}
              onStatus={setFlagStatus}
              onClearClosed={async () => setFlags(await api.clearClosedFlags())}
              onOpenUser={openUser}
            />
          )}
          {tab === "chat" && (
            <LiveChats
              channels={channels}
              messages={chat}
              channelFilter={selected}
              onOpenUser={openUser}
              sanctionsByLogin={byLogin}
              deleted={deleted}
            />
          )}
          {tab === "raids" && <RaidLog raids={raids} channelFilter={selected} now={now} />}
          {tab === "rules" && <Rules rules={rules} channels={channels} onRulesChanged={setRules} editRuleId={editRuleId} />}
          {tab === "tuning" && (
            <Tuning
              rules={rules}
              onRulesChanged={setRules}
              toast={toast}
              onEditRule={(r) => {
                setEditRuleId(r.id);
                setTab("rules");
              }}
            />
          )}
          {tab === "settings" && <SettingsPanel settings={settings} onChange={updateSettings} />}
        </div>
      </main>

      {userPanel && (
        <UserPanel
          channel={userPanel.channel}
          login={userPanel.login}
          refreshKey={actionCount}
          actions={actions}
          modChannelCount={channels.filter((c) => c.isMod).length}
          onClose={() => setUserPanel(null)}
        />
      )}

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.kind}`}>
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}
