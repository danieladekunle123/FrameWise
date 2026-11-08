import { fireEvent, render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { IssueCard } from "./components/IssueCard";
import { FixProvider } from "./fixes";
import { statusMeta } from "./components/ui";
import { topIssues } from "./screens/Home";
import { Cleanup } from "./screens/Cleanup";
import { MonitorScreen } from "./screens/Monitor";
import { Profiles } from "./screens/Profiles";
import { Myths } from "./screens/Other";
import { ErrorBoundary } from "./components/Crash";
import { readableOn } from "./theme";
import type { Finding, ScanResult } from "./types";
import mockScan from "./mock-scan.json";

const scan = mockScan as ScanResult;
const byId = (id: string) => scan.findings.find((f) => f.check_id === id)!;

describe("scan helpers", () => {
  it("lists the top issues by impact and leaves out security choices", () => {
    const top = topIssues(scan);
    expect(top).toHaveLength(3);
    expect(top[0].impact).toBe("high");
    expect(top.every((f) => f.action !== "choose")).toBe(true);
  });

  it("gives every status a word, never colour alone", () => {
    for (const f of scan.findings) {
      expect(statusMeta(f).label.length).toBeGreaterThan(0);
    }
    expect(statusMeta(byId("memory_integrity")).label).toBe("Your call");
  });
});

describe("accent text contrast", () => {
  it("picks dark text on the dark-theme violet and white on the light-theme violet", () => {
    expect(readableOn("#8B73FF")).toBe("#0E0F14");
    expect(readableOn("#6D4AFF")).toBe("#FFFFFF");
    expect(readableOn("#FFD700")).toBe("#0E0F14");
  });
});

function withFixes(ui: ReactNode, onFindings = () => {}) {
  return (
    <FixProvider restorePoint onFindings={onFindings} onOpenCleanup={() => {}}>
      {ui}
    </FixProvider>
  );
}

describe("IssueCard", () => {
  const ram: Finding = byId("ram_speed");

  it("shows the summary collapsed and the guide when opened", () => {
    const { rerender } = render(withFixes(<IssueCard finding={ram} open={false} onToggle={() => {}} onRecheck={async () => {}} />));
    expect(screen.getByText(ram.summary)).toBeTruthy();
    expect(screen.queryByText("Steps")).toBeNull();

    rerender(withFixes(<IssueCard finding={ram} open onToggle={() => {}} onRecheck={async () => {}} />));
    expect(screen.getByText("Steps")).toBeTruthy();
    expect(screen.getByText(ram.guide.steps[0])).toBeTruthy();
    expect(screen.getByText("3600 MT/s")).toBeTruthy();
  });

  it("calls recheck with the check id", async () => {
    const calls: string[] = [];
    render(withFixes(<IssueCard finding={ram} open onToggle={() => {}} onRecheck={async (id) => void calls.push(id)} />));
    fireEvent.click(screen.getByText("Recheck"));
    expect(calls).toEqual(["ram_speed"]);
  });

  it("offers no Fix for guides", () => {
    render(withFixes(<IssueCard finding={ram} open onToggle={() => {}} onRecheck={async () => {}} />));
    expect(ram.fix).toBeNull();
    expect(screen.queryByRole("button", { name: /Fix|Turn/ })).toBeNull();
  });

  it("applies a fix, then asks to keep a new refresh rate and offers Undo", async () => {
    const hz = byId("refresh_rate");
    render(withFixes(<IssueCard finding={hz} open onToggle={() => {}} onRecheck={async () => {}} />));
    fireEvent.click(screen.getByRole("button", { name: "Set highest refresh rate" }));
    const keep = await screen.findByRole("dialog", { name: "Keep this refresh rate?" }, { timeout: 6000 });
    expect(within(keep).getByText(/puts the previous rate back/)).toBeTruthy();
    fireEvent.click(within(keep).getByRole("button", { name: "Keep" }));
    expect(await screen.findByText("1 change made", {}, { timeout: 3000 })).toBeTruthy();
    expect(screen.getByText(/restore point was created first/)).toBeTruthy();
    // Undo in the toast and on the card's "Changed by FrameWise" row.
    expect(await screen.findByText("Changed by FrameWise")).toBeTruthy();
    expect(screen.getAllByRole("button", { name: /Undo/ }).length).toBe(2);
  }, 15000);

  it("asks before turning off a security setting", () => {
    const mi = byId("memory_integrity");
    render(withFixes(<IssueCard finding={mi} open onToggle={() => {}} onRecheck={async () => {}} />));
    fireEvent.click(screen.getByRole("button", { name: "Turn it off" }));
    const dlg = screen.getByRole("dialog");
    expect(within(dlg).getByText(/removes that protection/)).toBeTruthy();
    // Cancel is focused first, so Enter never turns protection off by accident.
    expect(document.activeElement?.textContent).toBe("Cancel");
  });

  it("lets you pick which startup apps to disable", async () => {
    const st = byId("startup_bloat");
    render(withFixes(<IssueCard finding={st} open onToggle={() => {}} onRecheck={async () => {}} />));
    fireEvent.click(screen.getByRole("button", { name: "Choose apps to disable" }));
    const dlg = await screen.findByRole("dialog");
    const disable = within(dlg).getByRole("button", { name: /Disable/ }) as HTMLButtonElement;
    expect(disable.disabled).toBe(true);
    fireEvent.click(await within(dlg).findByLabelText(/Discord/));
    expect(disable.disabled).toBe(false);
    expect(within(dlg).getByText(/already disabled/)).toBeTruthy();
  });

  it("won't call a benchmark difference real without two runs per side", async () => {
    render(withFixes(<MonitorScreen />));
    const rows = await screen.findAllByRole("group", { name: /Compare/ }, { timeout: 3000 });
    fireEvent.click(within(rows[0]).getByRole("button", { name: "After" }));
    fireEvent.click(within(rows[1]).getByRole("button", { name: "Before" }));
    expect(await screen.findByText("Need more runs", {}, { timeout: 3000 })).toBeTruthy();
    expect(screen.getByText(/Record at least two runs on each side/)).toBeTruthy();
  });

  it("opens the editor instead of turning on an empty game profile", async () => {
    render(<Profiles />);
    const sw = await screen.findByRole("switch", { name: "Profile for Fortnite" }, { timeout: 3000 });
    fireEvent.click(sw);
    const dlg = await screen.findByRole("dialog", { name: "Profile for Fortnite" });
    expect(sw.getAttribute("aria-checked")).toBe("false");
    // Pick "Pause" for Discord and save: the row now describes the profile.
    const discord = await within(dlg).findByRole("group", { name: /discord\.exe/ }, { timeout: 3000 });
    fireEvent.click(within(discord).getByRole("button", { name: "Pause" }));
    fireEvent.click(within(dlg).getByRole("button", { name: "Save and turn on" }));
    expect(await screen.findByText(/Epic Games · pauses discord/, {}, { timeout: 3000 })).toBeTruthy();
    expect((await screen.findByRole("switch", { name: "Profile for Fortnite" })).getAttribute("aria-checked")).toBe("true");
  });

  it("shows a calm error screen instead of a blank window when the interface fails", () => {
    const Broken = () => {
      throw new Error("boom");
    };
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <ErrorBoundary>
        <Broken />
      </ErrorBoundary>,
    );
    expect(screen.getByText("Something went wrong in this window")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Reload FrameWise" })).toBeTruthy();
    spy.mockRestore();
  });

  it("says when clearing the standby list isn't expected to help, and reports what it cleared", async () => {
    render(withFixes(<Myths />));
    expect(await screen.findByText(/isn't expected to help here/, {}, { timeout: 3000 })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Clear the standby list now/ }));
    expect(await screen.findByText(/went from 9\.2 GB to 307 MB/, {}, { timeout: 5000 })).toBeTruthy();
  });

  it("never ticks a Cleanup category it couldn't measure", async () => {
    render(withFixes(<Cleanup />));
    const dumps = (await screen.findByLabelText(/Windows crash dumps/, {}, { timeout: 3000 })) as HTMLInputElement;
    expect(dumps.disabled).toBe(true);
    expect(dumps.checked).toBe(false);
    // Measured, default-selected categories are ticked.
    expect((screen.getByLabelText(/Your temporary files/) as HTMLInputElement).checked).toBe(true);
  });
});
