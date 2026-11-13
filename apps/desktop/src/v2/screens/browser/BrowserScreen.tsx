import { useEffect, useState } from "react";
import type { Browser, TabList } from "../../../contracts/browser";
import type { ProcessRow } from "../../../contracts/processes";
import type { ProcessMonitor } from "../../../features/processes/useProcessesPage";
import { closeBrowserTabs, listBrowserTabs } from "../../../lib/browserIpc";
import { openSystemSettings } from "../../../lib/systemPageIpc";
import { StorageError } from "../../../lib/storageIpc";
import { storageErrorText } from "../../../features/storage/format";
import { AccessSteps } from "../../ui/AccessSteps";
import { Button, IconTile, Modal, PageHeader, Pill, Spinner, Toast, useToast, type Tone } from "../../ui/components";
import { formatGb, plural } from "../../ui/format";
import { headingIdFor, hrefFor } from "../../shell/routes";
import "./browser.css";

const meta: Record<Browser, { name: string; tone: Tone; process: string }> = {
  safari: { name: "Safari", tone: "blue", process: "Safari" },
  chrome: { name: "Google Chrome", tone: "green", process: "Google Chrome" },
  brave: { name: "Brave", tone: "orange", process: "Brave Browser" },
  edge: { name: "Microsoft Edge", tone: "teal", process: "Microsoft Edge" },
};

function usage(rows: readonly ProcessRow[], browser: Browser): ProcessRow | null {
  return rows.find((row) => row.name === meta[browser].process) ?? null;
}

/** Browser care: how hard each browser works, its tabs, and closing the ones you choose. */
export function BrowserScreen({ monitor }: { monitor: ProcessMonitor }) {
  const [lists, setLists] = useState<Partial<Record<Browser, TabList>>>({});
  const [loading, setLoading] = useState<Browser | null>(null);
  const [selection, setSelection] = useState<ReadonlySet<string>>(new Set());
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const toast = useToast(6000);
  const { enter, leave } = monitor;

  // Readings come every 2 seconds while this page is on screen, as on Processes.
  useEffect(() => {
    enter();
    return leave;
  }, [enter, leave]);

  const rows = monitor.list.status === "ready" ? monitor.list.value.rows : monitor.list.status === "error" ? (monitor.list.last?.rows ?? []) : [];
  const running = (Object.keys(meta) as Browser[]).filter((browser) => usage(rows, browser) !== null);
  const picked = Object.values(lists).flatMap((list) => list?.tabs.filter((tab) => selection.has(tab.id)) ?? []);

  const show = (browser: Browser) => {
    setLoading(browser);
    listBrowserTabs(browser).then(
      (list) => {
        setLoading(null);
        setLists((current) => ({ ...current, [browser]: list }));
      },
      (error: unknown) => {
        setLoading(null);
        toast.show(storageErrorText[error instanceof StorageError ? error.kind : "unknown"], "warn");
      },
    );
  };

  const close = () => {
    setBusy(true);
    closeBrowserTabs(picked.map((tab) => tab.id)).then(
      ({ closed, notAllowed }) => {
        setBusy(false);
        setConfirm(false);
        setSelection(new Set());
        toast.show(
          notAllowed
            ? "macOS didn’t let Kleari close tabs. Allow it under Automation, then try again."
            : closed === picked.length
              ? `Closed ${plural(closed, "tab")}. Reopen one from the browser’s History menu.`
              : `Closed ${closed} of ${picked.length}; the others changed since they were listed.`,
          notAllowed ? "warn" : undefined,
        );
        for (const browser of new Set(Object.values(lists).filter((list) => list?.tabs.some((tab) => picked.includes(tab))).map((list) => list?.browser))) {
          if (browser) {
            show(browser);
          }
        }
      },
      (error: unknown) => {
        setBusy(false);
        setConfirm(false);
        toast.show(storageErrorText[error instanceof StorageError ? error.kind : "unknown"], "warn");
      },
    );
  };

  const toggle = (id: string) => {
    const next = new Set(selection);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    setSelection(next);
  };

  return (
    <>
      <PageHeader
        title="Browser care"
        headingId={headingIdFor("browser")}
        subtitle="How hard your browsers work, and the tabs they hold. Close the tabs you no longer need to give memory and battery back."
        actions={picked.length > 0 ? <Button variant="danger" size="sm" onClick={() => setConfirm(true)}>Close {plural(picked.length, "tab")}…</Button> : undefined}
      />
      <div className="br-body">
        {running.length === 0 && (
          <section className="v2-card br-empty" aria-label="No browser open">
            <IconTile tone="blue" icon="laptop" size={40} glow />
            <span className="v2-row-title">{monitor.list.status === "loading" ? "Reading…" : "No supported browser is open"}</span>
            <span className="v2-card-note">Safari, Google Chrome, Brave and Microsoft Edge show here while they’re open.</span>
          </section>
        )}
        {running.map((browser) => {
          const row = usage(rows, browser);
          const list = lists[browser];
          const windows = list ? [...new Set(list.tabs.map((tab) => tab.window))] : [];
          return (
            <section key={browser} aria-label={meta[browser].name} className="v2-card br-card">
              <div className="br-head">
                <IconTile tone={meta[browser].tone} icon="laptop" size={38} glow />
                <span className="v2-row-text">
                  <span className="v2-row-title">{meta[browser].name}</span>
                  <span className="v2-row-sub v2-num">
                    {row?.cpuPercent === null || row?.cpuPercent === undefined ? "CPU —" : `CPU ${Math.round(row.cpuPercent)}%`} · {formatGb((row?.memoryBytes ?? 0) / 1e9)} memory · {plural(row?.processCount ?? 0, "process", "processes")}
                    {browser === "safari" ? " (Safari’s page processes belong to macOS and aren’t counted)" : ""}
                  </span>
                </span>
                {(row?.cpuPercent ?? 0) > 50 && <Pill status="warn">Busy</Pill>}
                <Button variant="soft" size="sm" disabled={loading !== null} onClick={() => show(browser)}>
                  {loading === browser ? <Spinner size={10} /> : null} {list ? "List again" : "Show tabs"}
                </Button>
              </div>
              {list?.status === "notAllowed" && <AccessSteps kind="automation" onOpen={() => void openSystemSettings("automation")} />}
              {list?.status === "failed" && <span className="v2-card-note">{meta[browser].name} didn’t answer. Try again in a moment.</span>}
              {list?.status === "notRunning" && <span className="v2-card-note">{meta[browser].name} isn’t open any more.</span>}
              {list?.status === "listed" && (
                <>
                  <span className="v2-card-note">{plural(list.tabs.length, "tab")} in {plural(windows.length, "window")}. Pages you don’t look at still use memory, and some keep running scripts.</span>
                  {windows.map((window) => (
                    <div key={window} className="br-window">
                      <span className="v2-row-sub">Window {window}</span>
                      <ul className="br-tabs">
                        {list.tabs.filter((tab) => tab.window === window).map((tab) => (
                          <li key={tab.id}>
                            <input type="checkbox" aria-label={`Choose ${tab.title}`} checked={selection.has(tab.id)} onChange={() => toggle(tab.id)} />
                            <span className="v2-row-text">
                              <span className="v2-row-title v2-ellipsis">{tab.title}</span>
                              <span className="v2-row-sub v2-ellipsis">{tab.site}</span>
                            </span>
                          </li>
                        ))}
                      </ul>
                    </div>
                  ))}
                </>
              )}
            </section>
          );
        })}
        <section aria-labelledby="br-more" className="v2-card br-more">
          <h2 id="br-more" className="v2-card-title-sm">More ways to lighten a browser</h2>
          <ul className="v2-info-list">
            <li><span className="v2-row-text"><span className="v2-row-title">Clear its cache</span><span className="v2-row-sub">A Standard scan in <a className="v2-link" href={hrefFor("cleanup")}>Cleanup</a> finds browser caches when the browser is closed.</span></span></li>
            <li><span className="v2-row-text"><span className="v2-row-title">Quit and reopen it</span><span className="v2-row-sub">Frees memory a long-running browser holds. <a className="v2-link" href={hrefFor("processes")}>Processes</a> can ask it to quit.</span></span></li>
            <li><span className="v2-row-text"><span className="v2-row-title">Turn on its memory saver</span><span className="v2-row-sub">Chrome, Brave and Edge can put tabs to sleep: Settings › Performance.</span></span></li>
          </ul>
        </section>
      </div>

      <Modal open={confirm} onClose={() => setConfirm(false)} labelledBy="br-close-title" role="alertdialog">
        <div className="v2-dialog-body">
          <h2 id="br-close-title" className="v2-dialog-title">Close {plural(picked.length, "tab")}?</h2>
          <p className="v2-dialog-text">Unsaved work in these pages (a form, a draft) is lost. You can reopen closed tabs from the browser’s History menu.</p>
          <div className="v2-dialog-actions">
            <Button variant="secondary" onClick={() => setConfirm(false)}>Cancel</Button>
            <Button variant="danger" disabled={busy} onClick={close}>{busy ? <Spinner size={10} tone="danger" /> : null} Close tabs</Button>
          </div>
        </div>
      </Modal>
      <Toast message={toast.message} />
    </>
  );
}
