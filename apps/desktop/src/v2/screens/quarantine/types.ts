import type { IconName } from "../../ui/icons";
import type { Tone } from "../../ui/components";

/** Whether a held item can go back now. `pending`: a move or restore not confirmed yet
 * (resolved at the next launch); `unverified`: Kleari couldn't check it. Neither can be
 * restored or deleted until then. `needsFolder`: it came from a folder added outside the
 * home folder, which must be chosen again before it can be restored. */
export type QuarantineItemStatus = "restorable" | "conflict" | "folderMissing" | "missing" | "pending" | "unverified" | "needsFolder";

/** What to ask for. The recorded folder comes from the history, which can be edited, so it
 * is offered as what the record says, never as fact. */
export function chooseFolderHint(item: { name: string; neededFolder?: string }): string {
  return item.neededFolder
    ? `Kleari’s record says it was added from ${item.neededFolder}. Choose the folder you added it from.`
    : `Choose the folder you added ${item.name} from.`;
}

/** Items that can be selected for restore or permanent delete. */
export function actionable(item: { status: QuarantineItemStatus }): boolean {
  return item.status !== "pending" && item.status !== "unverified";
}

export interface QuarantineItem {
  id: string;
  name: string;
  from: string;
  gb: number;
  tone: Tone;
  icon: IconName;
  status: QuarantineItemStatus;
  /** A file now exists at the original place, so a restore is blocked rather than overwriting it. */
  conflict?: boolean;
  /** Choosing its folder would allow a restore (`needsFolder`) or settle an interrupted one (`pending`). */
  chooseFolder?: boolean;
  /** What the history records as the folder it was added from: a hint, not proof. */
  neededFolder?: string;
}

export interface QuarantineBatch {
  id: string;
  title: string;
  sub: string;
  isNew?: boolean;
  items: readonly QuarantineItem[];
}

export interface DeleteJob {
  items: readonly QuarantineItem[];
  /** How many items are finished (deleted or failed). */
  done: number;
  /** Items that couldn't be deleted; they stay in quarantine. */
  failed: ReadonlySet<string>;
  stopRequested: boolean;
  finished: boolean;
}

/** Why a restore didn't happen, as the backend reported it. */
export type RestoreReason = "conflict" | "folderMissing" | "missing" | "needsAccess" | "notAllowed" | "needsFolder" | "failed";

export type RestoreOutcome =
  | { kind: "restored"; count: number }
  /** `reasons`: the backend's outcome for each blocked item, by ID. */
  | { kind: "blocked"; restored: number; blocked: readonly QuarantineItem[]; reasons: Readonly<Record<string, RestoreReason>> }
  /** Some restores couldn't be confirmed either way; Kleari tries again at each launch. */
  | { kind: "unconfirmed"; restored: number; unconfirmed: readonly QuarantineItem[] }
  | { kind: "failed"; message: string };
