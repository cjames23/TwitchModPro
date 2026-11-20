import type { Settings, Severity } from "../types";
import { playTone } from "../util";

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
