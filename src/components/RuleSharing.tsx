import { useMemo, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { ImportChoice, ImportPreview, Rule } from "../types";
import { api } from "../api";

const STATUS_LABEL = {
  new: "New",
  identical: "You already have this",
  conflict: "Same name as one of yours",
  invalid: "Can't be imported",
} as const;

function fileName(name: string) {
  return (name.trim() || "rules").replace(/[^\w\- ]+/g, "").replace(/\s+/g, "-").toLowerCase() + ".json";
}

/** Export rules to a file or the clipboard, and import packs other mods shared. */
export function RuleSharing({
  rules,
  toast,
  onImported,
}: {
  rules: Rule[];
  toast: (text: string, kind?: "info" | "error") => void;
  onImported: (rules: Rule[]) => void;
}) {
  const [mode, setMode] = useState<"export" | "import">("export");
  return (
    <div className="rule-editor sharing">
      <div className="seg">
        <button className={mode === "export" ? "on" : ""} onClick={() => setMode("export")}>
          Export
        </button>
        <button className={mode === "import" ? "on" : ""} onClick={() => setMode("import")}>
          Import
        </button>
      </div>
      {mode === "export" ? <Export rules={rules} toast={toast} /> : <Import toast={toast} onImported={onImported} />}
    </div>
  );
}

function Export({ rules, toast }: { rules: Rule[]; toast: (text: string, kind?: "info" | "error") => void }) {
  const groups = useMemo(() => {
    const m = new Map<string, Rule[]>();
    for (const r of rules) {
      const g = r.pack ?? "Your rules";
      m.set(g, [...(m.get(g) ?? []), r]);
    }
    return [...m.entries()];
  }, [rules]);
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [busy, setBusy] = useState(false);

  const toggle = (ids: number[], on: boolean) =>
    setPicked((p) => {
      const n = new Set(p);
      ids.forEach((id) => (on ? n.add(id) : n.delete(id)));
      return n;
    });

  async function run(how: "file" | "copy") {
    setBusy(true);
    try {
      const text = await api.exportRules([...picked], name, description);
      if (how === "copy") {
        await navigator.clipboard.writeText(text);
        toast(`Copied ${picked.size} rule${picked.size === 1 ? "" : "s"}. Paste it in Discord or a DM, or save it as a .json file.`);
      } else {
        const path = await save({ defaultPath: fileName(name), filters: [{ name: "Rule pack", extensions: ["json"] }] });
        if (!path) return;
        await api.saveRuleFile(path, text);
        toast(`Saved ${picked.size} rule${picked.size === 1 ? "" : "s"} to ${path}`);
      }
    } catch (e) {
      toast(String(e), "error");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <p className="muted-text">
        Pick the rules to share. Which channels a rule is limited to and your per-channel alert settings aren't included;
        the person importing chooses their own.
      </p>
      <div className="share-list">
        {groups.map(([group, rs]) => {
          const ids = rs.map((r) => r.id);
          const all = ids.every((id) => picked.has(id));
          return (
            <div key={group} className="share-group">
              <label className="check share-group-title">
                <input type="checkbox" checked={all} onChange={(e) => toggle(ids, e.target.checked)} />
                <strong>{group}</strong> <span className="muted-text">({rs.length})</span>
              </label>
              {rs.map((r) => (
                <label key={r.id} className="check share-rule">
                  <input type="checkbox" checked={picked.has(r.id)} onChange={(e) => toggle([r.id], e.target.checked)} />
                  {r.name}
                  {!r.enabled && <span className="muted-text"> (off)</span>}
                </label>
              ))}
            </div>
          );
        })}
      </div>
      <label className="field">
        <span>Pack name</span>
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Kitdy's mod rules" />
      </label>
      <label className="field">
        <span>Description (optional)</span>
        <input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="What these rules are for" />
      </label>
      <div className="editor-actions">
        <span className="muted-text">{picked.size} selected</span>
        <span className="spacer" />
        <button disabled={busy || !picked.size || !name.trim()} onClick={() => run("copy")}>
          Copy as text
        </button>
        <button className="primary" disabled={busy || !picked.size || !name.trim()} onClick={() => run("file")}>
          Save file…
        </button>
      </div>
    </>
  );
}

function Import({ toast, onImported }: { toast: (text: string, kind?: "info" | "error") => void; onImported: (rules: Rule[]) => void }) {
  const [text, setText] = useState("");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [choices, setChoices] = useState<Record<number, ImportChoice>>({});
  const [group, setGroup] = useState("");
  const [testMode, setTestMode] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function check(source: string) {
    setError(null);
    setPreview(null);
    try {
      const p = await api.previewRuleImport(source);
      setText(source);
      setPreview(p);
      setGroup(p.name || "Imported");
      const initial: Record<number, ImportChoice> = {};
      for (const it of p.items) initial[it.index] = it.status === "new" ? "add" : "skip";
      setChoices(initial);
    } catch (e) {
      setError(String(e));
    }
  }

  async function openFile() {
    const path = await open({ multiple: false, directory: false, filters: [{ name: "Rule pack", extensions: ["json"] }] });
    if (typeof path !== "string") return;
    try {
      await check(await api.readRuleFile(path));
    } catch (e) {
      setError(String(e));
    }
  }

  async function doImport() {
    if (!preview) return;
    setBusy(true);
    try {
      const picked = Object.entries(choices)
        .filter(([, c]) => c !== "skip")
        .map(([i, c]) => [Number(i), c] as [number, ImportChoice]);
      const rules = await api.importRules(text, picked, group, testMode);
      toast(`Imported ${picked.length} rule${picked.length === 1 ? "" : "s"}${testMode ? " in test mode. Check the Tuning tab after a stream." : "."}`);
      onImported(rules);
    } catch (e) {
      toast(String(e), "error");
    } finally {
      setBusy(false);
    }
  }

  const count = Object.values(choices).filter((c) => c !== "skip").length;

  if (!preview) {
    return (
      <>
        <p className="muted-text">Open a rule pack file another mod sent you, or paste the text they copied.</p>
        <div className="row-gap">
          <button className="primary" onClick={openFile}>
            Open file…
          </button>
        </div>
        <label className="field">
          <span>Or paste it here</span>
          <textarea rows={8} value={text} onChange={(e) => setText(e.target.value)} placeholder='{"format": "twitch-mod-pro/rules", …}' spellCheck={false} />
        </label>
        {error && <div className="form-error">{error}</div>}
        <div className="editor-actions">
          <span className="spacer" />
          <button className="primary" disabled={!text.trim()} onClick={() => check(text)}>
            Check pasted text
          </button>
        </div>
      </>
    );
  }

  return (
    <>
      <div className="import-head">
        <strong>{preview.name || "Rule pack"}</strong>
        {preview.exportedAt > 0 && <span className="muted-text"> · shared {new Date(preview.exportedAt).toLocaleDateString()}</span>}
        {preview.description && <p className="muted-text">{preview.description}</p>}
      </div>
      <div className="share-list">
        {preview.items.map((it) => {
          const choice = choices[it.index] ?? "skip";
          return (
            <div key={it.index} className={`import-item ${it.status}`}>
              <div className="import-title">
                <strong>{it.rule.name}</strong> <span className={`import-status ${it.status}`}>{STATUS_LABEL[it.status]}</span>
              </div>
              {it.error && <div className="form-error">{it.error}</div>}
              {it.status !== "invalid" && (
                <div className="seg small">
                  <button className={choice === "skip" ? "on" : ""} onClick={() => setChoices((c) => ({ ...c, [it.index]: "skip" }))}>
                    Skip
                  </button>
                  <button className={choice === "add" ? "on" : ""} onClick={() => setChoices((c) => ({ ...c, [it.index]: "add" }))}>
                    {it.status === "new" ? "Add" : "Add a copy"}
                  </button>
                  {it.status === "conflict" && (
                    <button className={choice === "replace" ? "on" : ""} onClick={() => setChoices((c) => ({ ...c, [it.index]: "replace" }))}>
                      Replace mine
                    </button>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
      <label className="field">
        <span>Group them as</span>
        <input value={group} onChange={(e) => setGroup(e.target.value)} />
      </label>
      <label className="check">
        <input type="checkbox" checked={testMode} onChange={(e) => setTestMode(e.target.checked)} />
        Import in test mode (they record what they would flag, without filling your queue)
      </label>
      <div className="editor-actions">
        <button className="ghost" onClick={() => setPreview(null)}>
          Back
        </button>
        <span className="spacer" />
        <button className="primary" disabled={busy || count === 0} onClick={doImport}>
          {busy ? "Importing…" : `Import ${count} rule${count === 1 ? "" : "s"}`}
        </button>
      </div>
    </>
  );
}
