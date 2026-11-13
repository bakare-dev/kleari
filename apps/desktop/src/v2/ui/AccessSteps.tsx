import { Button } from "./components";

/** macOS permissions Kleari can ask for, each with where to turn it on in System Settings. */
export type AccessKind = "fullDiskAccess" | "accessibility" | "appManagement" | "automation" | "filesAndFolders" | "notifications" | "loginItems";

const guides: Record<AccessKind, { title: string; why: string; steps: readonly string[] }> = {
  fullDiskAccess: {
    title: "Turn on Full Disk Access",
    why: "Lets Kleari measure folders macOS protects (Mail, Safari, the Trash, other apps’ data). Kleari still only reads them.",
    steps: [
      "Click Open System Settings below. It opens Privacy & Security › Full Disk Access.",
      "Find Kleari in the list. If it isn’t there, click the + button under the list, open Applications, choose Kleari and click Open.",
      "Turn the switch next to Kleari on. macOS may ask for your Mac’s password or Touch ID.",
      "If macOS says Kleari must quit and reopen, choose Quit & Reopen (or quit Kleari and open it again).",
    ],
  },
  accessibility: {
    title: "Turn on Accessibility for Kleari",
    why: "Lets the Mouse module change how your mouse wheel and buttons work. It never sees what you type.",
    steps: [
      "Click Open System Settings below. It opens Privacy & Security › Accessibility.",
      "Find Kleari in the list. If it isn’t there, click +, open Applications, choose Kleari and click Open.",
      "Turn the switch next to Kleari on, and enter your password or use Touch ID if asked.",
      "Come back to Kleari: it notices by itself within a few seconds.",
    ],
  },
  appManagement: {
    title: "Allow Kleari to remove apps",
    why: "macOS asks before one app moves another app to the Trash.",
    steps: [
      "Click Open System Settings below. It opens Privacy & Security › App Management.",
      "Find Kleari and turn its switch on. If it isn’t listed, click +, open Applications and choose Kleari.",
      "Enter your password or use Touch ID if asked, then come back and choose Remove again.",
      "An app an administrator installed may still need removing in Finder: drag it to the Trash there.",
    ],
  },
  automation: {
    title: "Allow Kleari to control this app",
    why: "Kleari asks a browser for its tabs, or Finder to empty the Trash, only when you choose it.",
    steps: [
      "Click Open System Settings below. It opens Privacy & Security › Automation.",
      "Find Kleari in the list and click the arrow or the name to show the apps under it.",
      "Turn on the switch for the app (Safari, Google Chrome, Finder, …).",
      "Come back to Kleari and try again. If Kleari isn’t listed yet, try once first: macOS then asks, and you can choose OK.",
    ],
  },
  filesAndFolders: {
    title: "Allow Kleari to read a folder",
    why: "macOS asks before an app reads Desktop, Documents or Downloads.",
    steps: [
      "Click Open System Settings below. It opens Privacy & Security › Files and Folders.",
      "Find Kleari and click it to show the folders under it.",
      "Turn on the folders you want Kleari to read (for example Downloads).",
      "Come back to Kleari and scan again.",
    ],
  },
  notifications: {
    title: "Allow Kleari’s notifications",
    why: "Kleari only notifies what you turn on in Settings › Notifications.",
    steps: [
      "Click Open System Settings below. It opens Notifications.",
      "Scroll to Kleari in the list of apps and click it.",
      "Turn on Allow notifications, and choose Banners or Alerts.",
    ],
  },
  loginItems: {
    title: "Manage what opens at login",
    why: "macOS keeps the list of apps that open when you log in, and items that run in the background.",
    steps: [
      "Click Open System Settings below. It opens General › Login Items & Extensions.",
      "Under Open at Login, add or remove apps with + and −.",
      "Under Allow in the Background (or App Background Activity), turn off items you don’t want running.",
    ],
  },
};

interface AccessStepsProps {
  kind: AccessKind;
  /** Opens the matching pane of System Settings. */
  onOpen?: () => void;
  /** Hides the heading when the surrounding card already has one. */
  compact?: boolean;
}

/** Plain steps for turning a permission on in System Settings, for anyone, not only experts. */
export function AccessSteps({ kind, onOpen, compact = false }: AccessStepsProps) {
  const guide = guides[kind];
  return (
    <div className="v2-access-steps">
      {!compact && <strong className="v2-row-title">{guide.title}</strong>}
      <span className="v2-card-note">{guide.why}</span>
      <ol>
        {guide.steps.map((step) => (
          <li key={step}>{step}</li>
        ))}
      </ol>
      {onOpen && (
        <Button variant="soft" size="sm" onClick={onOpen}>
          Open System Settings
        </Button>
      )}
    </div>
  );
}
