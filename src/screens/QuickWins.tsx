import { useEffect, useMemo, useState } from "react";
import { Gauge, Wrench } from "lucide-react";
import { IssueCard } from "../components/IssueCard";
import { useFixes } from "../fixes";
import { IMPACT_RANK } from "../components/ui";
import type { Finding, ScanResult } from "../types";

type Filter = "issues" | "manual" | "ok" | "all";

const GROUPS: { title: string; hint?: string; test: (f: Finding) => boolean; cls: string }[] = [
  { title: "High impact", test: (f) => f.action !== "choose" && f.impact === "high", cls: "c-high" },
  { title: "Medium impact", test: (f) => f.action !== "choose" && f.impact === "medium", cls: "c-medium" },
  { title: "Low impact", test: (f) => f.action !== "choose" && (f.impact === "low" || f.impact === "low_medium"), cls: "c-low" },
  {
    title: "Your call",
    hint: "Trade-offs between FPS and security. Never applied automatically and not counted in your score.",
    test: (f) => f.action === "choose",
    cls: "c-accent",
  },
];

interface Props {
  scan: ScanResult | null;
  focusCheck: string | null;
  onScan: () => void;
  onRecheck: (checkId: string) => Promise<void>;
}

function matches(f: Finding, filter: Filter) {
  switch (filter) {
    case "issues":
      return f.status === "issue";
    case "manual":
      return f.status === "unknown";
    case "ok":
      return f.status === "ok" || f.status === "not_applicable";
    case "all":
      return true;
  }
}

export function QuickWins({ scan, focusCheck, onScan, onRecheck }: Props) {
  const [filter, setFilter] = useState<Filter>("issues");
  const [open, setOpen] = useState<Set<string>>(() => new Set(focusCheck ? [focusCheck] : []));
  const fixes = useFixes();

  useEffect(() => {
    if (!focusCheck) return;
    const f = scan?.findings.find((x) => x.check_id === focusCheck);
    if (f && !matches(f, filter)) setFilter("all");
    setOpen((o) => new Set(o).add(focusCheck));
    requestAnimationFrame(() =>
      document.querySelector(`[data-check="${focusCheck}"]`)?.scrollIntoView({ block: "start", behavior: "smooth" }),
    );
    // Only when a new check is focused from elsewhere.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusCheck]);

  const counts = useMemo(() => {
    const f = scan?.findings ?? [];
    return {
      issues: f.filter((x) => matches(x, "issues")).length,
      manual: f.filter((x) => matches(x, "manual")).length,
      ok: f.filter((x) => matches(x, "ok")).length,
      all: f.length,
    };
  }, [scan]);

  if (!scan) {
    return (
      <div className="page">
        <div className="card empty">
          <Gauge size={28} />
          <strong>No scan yet</strong>
          <p className="muted">A scan checks 16 settings and hardware issues in a few seconds, without changing anything.</p>
          <button className="btn btn-primary" onClick={onScan}>
            Scan my PC
          </button>
        </div>
      </div>
    );
  }

  const toggle = (id: string) =>
    setOpen((o) => {
      const n = new Set(o);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  const visible = scan.findings.filter((f) => matches(f, filter));
  const safeCount = scan.findings.filter((f) => f.status === "issue" && f.fix?.safe).length;
  const card = (f: Finding) => (
    <IssueCard key={f.check_id} finding={f} open={open.has(f.check_id)} onToggle={() => toggle(f.check_id)} onRecheck={onRecheck} />
  );

  return (
    <div className="page">
      <div className="section-head">
        <div className="tabs" role="group" aria-label="Filter checks">
          {(
            [
              ["issues", "Needs attention"],
              ["manual", "Check manually"],
              ["ok", "Good"],
              ["all", "All checks"],
            ] as [Filter, string][]
          ).map(([key, label]) => (
            <button key={key} aria-pressed={filter === key} onClick={() => setFilter(key)}>
              {label} <span className="num">({counts[key]})</span>
            </button>
          ))}
        </div>
        <div className="head-actions">
          <p className="hint">Impact labels are typical estimates. Benchmark mode will measure the real effect on your PC.</p>
          {safeCount > 0 && (
            <button className="btn btn-sm" onClick={() => fixes.fixAllSafe(scan.findings)} disabled={fixes.busy}>
              <Wrench size={14} /> Fix all safe ({safeCount})
            </button>
          )}
        </div>
      </div>

      {visible.length === 0 && (
        <div className="card empty">
          <strong>Nothing here</strong>
          <p className="muted">
            {filter === "issues" ? "Your PC is already in good shape: no issues found." : "No checks in this group."}
          </p>
        </div>
      )}

      {filter === "issues"
        ? GROUPS.map((g) => {
            const items = visible.filter(g.test);
            if (items.length === 0) return null;
            return (
              <section key={g.title} className="issue-list" aria-label={g.title}>
                <div className="group-title">
                  <span className={`badge ${g.cls}`}>{items.length}</span>
                  <h2>{g.title}</h2>
                  {g.hint && <span className="hint">{g.hint}</span>}
                </div>
                {items.map(card)}
              </section>
            );
          })
        : [...visible].sort((a, b) => IMPACT_RANK[a.impact] - IMPACT_RANK[b.impact]).map(card)}
    </div>
  );
}
