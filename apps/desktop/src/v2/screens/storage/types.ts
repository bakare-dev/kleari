import type { IconName } from "../../ui/icons";
import type { Status, Tone } from "../../ui/components";

export type FolderTag = "macos" | "yours";

export interface FolderNode {
  name: string;
  gb: number;
  tone: Tone;
  icon: IconName;
  tag?: FolderTag;
  /** Something inside couldn't be read, so the size is a minimum. */
  lowerBound?: boolean;
  children?: readonly FolderNode[];
}

export interface StorageCategoryDetail {
  id: string;
  label: string;
  gb: number;
  /** Something that belongs here couldn't be counted, so `gb` is a minimum. */
  lowerBound?: boolean;
  tone: Tone;
  icon: IconName;
  blurb: string;
  top: readonly { name: string; gb: number }[];
  pill: string;
  pillStatus: Status;
  /** A folder path (child indexes from the root) to open in Largest folders. */
  browse?: readonly number[];
  /** The Cleanup scan that reviews this category, if one does. */
  cleanupMode?: "standard" | "deep" | "dev";
}

export interface LargeFile {
  id: string;
  name: string;
  path: string;
  age: string;
  gb: number;
  tone: Tone;
  icon: IconName;
  /** Files managed by their owner (Docker data, Xcode-managed runtimes, macOS). */
  locked?: { label: string; tip: string };
  /** Adds it to Cleanup's review, where it is listed for you to inspect. */
  reviewId: string;
}

export interface ManagedSpace {
  label: string;
  sub: string;
  /** `null` when macOS didn't report it. */
  gb: number | null;
  tone: Tone;
  icon: IconName;
}

export interface AnalysisStep {
  label: string;
  /** Counted once the step has finished. */
  gb: number | null;
  lowerBound?: boolean;
}

export interface StorageAnalysis {
  running: boolean;
  /** The last analysis was cancelled; the earlier result (if any) is still shown. */
  stopped: boolean;
  /** Why the last analysis failed, if it did. */
  failed: string | null;
  /** 0–100; `null` while running when used space is unknown, so only the step is shown. */
  percent: number | null;
  /** The step running now. */
  stepIndex: number;
  countedGb: number;
  /** Shown when an analysis has finished, such as "281 GB counted in 1 min 10 s". */
  doneNote: string | null;
}

export interface StorageCapacity {
  usedGb: number;
  totalGb: number;
  availableGb: number;
}
