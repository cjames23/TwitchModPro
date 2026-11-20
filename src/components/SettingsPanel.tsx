import type { LadderStep, Settings, Severity } from "../types";
import { playTone } from "../util";
import { ordinal } from "../intel";

const TIMEOUT_CHOICES = [60, 600, 3600, 21600, 86400, 604800];
const choiceLabel = (s: number) =>
  s % 86400 === 0 ? `${s / 86400}d` : s % 3600 === 0 ? `${s / 3600}h` : `${s / 60}m`;

export function SettingsPanel({ settings, onChange }: { settings: Settings; onChange: (s: Settings) => void }) {
  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => onChange({ ...settings, [k]: v });

  return (
    <section className="panel">
      <header className="panel-header">
        <h2>Alerts</h2>
      </header>
      <div className="settings">
        <div className="setting-group">
          <h3>Flagged messages</h3>
          <label className="check">
            <input type="checkbox" checked={settings.notifyFlags} onChange={(e) => set("notifyFlags", e.target.checked)} />
            Desktop notification when a message is flagged
          </label>
          <label className="field inline">
            <span>Only alert for severity</span>
            <select
              value={settings.notifyMinSeverity}
              onChange={(e) => set("notifyMinSeverity", e.target.value as Severity)}
              disabled={!settings.notifyFlags}
            >
              <option value="low">Low and above</option>
              <option value="medium">Medium and above</option>
              <option value="high">High only</option>
            </select>
          </label>
          <p className="muted-text">Every flag still lands in the queue. This only controls pop-ups and sounds.</p>
        </div>

        <div className="setting-group">
          <h3>Raids</h3>
          <label className="check">
            <input type="checkbox" checked={settings.notifyRaids} onChange={(e) => set("notifyRaids", e.target.checked)} />
            Desktop notification when a watched channel is raided
          </label>
          <label className="field inline">
            <span>Minimum raid size</span>
            <input
              type="number"
              min={0}
              value={settings.raidMinViewers}
              onChange={(e) => set("raidMinViewers", Math.max(0, Number(e.target.value) || 0))}
              disabled={!settings.notifyRaids}
            />
            <span className="hint">viewers</span>
          </label>
          <label className="field inline">
            <span>Raid mode lasts</span>
            <input
              type="number"
              min={0}
              max={120}
              value={settings.raidModeMinutes}
              onChange={(e) => set("raidModeMinutes", Math.min(120, Math.max(0, Number(e.target.value) || 0)))}
            />
            <span className="hint">minutes after a raid (0 = off)</span>
          </label>
          <p className="muted-text">
            During raid mode, rules marked “Only during raid mode” are switched on for that channel. You can also start
            it by hand with the ⚡ button next to a channel.
          </p>
        </div>

        <div className="setting-group">
          <h3>Escalation ladder</h3>
          <p className="muted-text">
            The suggested action for a user's 1st, 2nd, 3rd… offense in a channel. Offenses are warnings, timeouts and bans
            (yours or other mods') in the last {settings.ladderLookbackDays} days.
          </p>
          {settings.ladder.map((step, i) => (
            <div key={i} className="ladder-row">
              <span className="ladder-n">{ordinal(i + 1)}</span>
              <select
                value={step.action}
                onChange={(e) => {
                  const action = e.target.value as LadderStep["action"];
                  const next = [...settings.ladder];
                  next[i] = { action, durationSecs: action === "timeout" ? (step.durationSecs ?? 600) : null };
                  set("ladder", next);
                }}
              >
                <option value="warn">Warn</option>
                <option value="timeout">Timeout</option>
                <option value="ban">Ban</option>
              </select>
              {step.action === "timeout" && (
                <select
                  value={step.durationSecs ?? 600}
                  onChange={(e) => {
                    const next = [...settings.ladder];
                    next[i] = { ...step, durationSecs: Number(e.target.value) };
                    set("ladder", next);
                  }}
                >
                  {TIMEOUT_CHOICES.map((c) => (
                    <option key={c} value={c}>
                      {choiceLabel(c)}
                    </option>
                  ))}
                </select>
              )}
              {settings.ladder.length > 1 && (
                <button className="ghost small" onClick={() => set("ladder", settings.ladder.filter((_, j) => j !== i))}>
                  Remove
                </button>
              )}
            </div>
          ))}
          <div className="row-gap">
            {settings.ladder.length < 8 && (
              <button onClick={() => set("ladder", [...settings.ladder, { action: "ban", durationSecs: null }])}>+ Add step</button>
            )}
            <label className="field inline">
              <span>Count offenses from the last</span>
              <input
                type="number"
                min={1}
                max={365}
                value={settings.ladderLookbackDays}
                onChange={(e) => set("ladderLookbackDays", Math.min(365, Math.max(1, Number(e.target.value) || 1)))}
              />
              <span className="hint">days</span>
            </label>
          </div>
        </div>

        <div className="setting-group">
          <h3>Spam waves</h3>
          <label className="field inline">
            <span>Flag when at least</span>
            <input
              type="number"
              min={2}
              max={100}
              value={settings.waveMinAccounts}
              onChange={(e) => set("waveMinAccounts", Math.min(100, Math.max(2, Number(e.target.value) || 2)))}
            />
            <span className="hint">accounts post the same message or have look-alike names within</span>
            <input
              type="number"
              min={10}
              max={600}
              value={settings.waveWindowSecs}
              onChange={(e) => set("waveWindowSecs", Math.min(600, Math.max(10, Number(e.target.value) || 10)))}
            />
            <span className="hint">seconds</span>
          </label>
          <p className="muted-text">A burst of first-time chatters needs twice as many accounts. Mods and VIPs are ignored.</p>
        </div>

        <div className="setting-group">
          <h3>Sound</h3>
          <label className="check">
            <input type="checkbox" checked={settings.playSounds} onChange={(e) => set("playSounds", e.target.checked)} />
            Play a sound with alerts
          </label>
          <div className="row-gap">
            <button onClick={() => playTone("flag")}>Preview flag sound</button>
            <button onClick={() => playTone("raid")}>Preview raid sound</button>
          </div>
        </div>

        <p className="muted-text">Mute a single channel from the bell icon in the sidebar.</p>
      </div>
    </section>
  );
}
