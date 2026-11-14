import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type AnchorHTMLAttributes,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { Glyph, type IconName } from "./icons";

/** Accent hues from the v2 palette. Each maps to a gradient pair in v2.css (`.tone-*`). */
export type Tone =
  | "blue"
  | "indigo"
  | "violet"
  | "purple"
  | "green"
  | "orange"
  | "yellow"
  | "gray"
  | "slate"
  | "teal"
  | "pink"
  | "rose"
  | "red"
  | "periwinkle";

/** Semantic status colours for pills and labels. */
export type Status = "ok" | "warn" | "muted" | "link" | "danger";

function cx(...names: (string | false | null | undefined)[]): string {
  return names.filter(Boolean).join(" ");
}

interface IconTileProps {
  tone: Tone;
  icon?: IconName;
  /** A letter shown instead of an icon (apps and processes). */
  initial?: string;
  size?: number;
  glow?: boolean;
  className?: string;
}

/** A colourful gradient tile with a white icon: the v2 signature element. Decorative. */
export function IconTile({ tone, icon, initial, size = 42, glow = false, className }: IconTileProps) {
  const radius = Math.round(size * 0.27);
  return (
    <span
      className={cx("v2-tile", `tone-${tone}`, glow && "v2-tile-glow", className)}
      style={{ width: size, height: size, borderRadius: radius, fontSize: Math.round(size * 0.43) }}
      aria-hidden="true"
    >
      {icon ? <Glyph name={icon} size={Math.round(size * 0.5)} strokeWidth={size < 30 ? 2.3 : 2.1} /> : initial}
    </span>
  );
}

export function Pill({ status, children, className }: { status: Status; children: ReactNode; className?: string }) {
  return <span className={cx("v2-pill", `v2-pill-${status}`, className)}>{children}</span>;
}

/** A small round dot in a tone, e.g. legends and category chips. */
export function Dot({ tone, size = 10 }: { tone: Tone; size?: number }) {
  return <span className={cx("v2-dot", `tone-${tone}`)} style={{ width: size, height: size }} aria-hidden="true" />;
}

type ButtonVariant = "primary" | "secondary" | "soft" | "danger" | "dangerOutline" | "plain";
type ButtonSize = "sm" | "md" | "lg";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

export function Button({ variant = "secondary", size = "md", className, type = "button", ...rest }: ButtonProps) {
  return <button type={type} className={cx("v2-btn", `v2-btn-${variant}`, `v2-btn-${size}`, className)} {...rest} />;
}

interface LinkButtonProps extends AnchorHTMLAttributes<HTMLAnchorElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

/** Navigation styled as a button. Navigation never starts an action on its own. */
export function LinkButton({ variant = "soft", size = "md", className, ...rest }: LinkButtonProps) {
  return <a className={cx("v2-btn", `v2-btn-${variant}`, `v2-btn-${size}`, className)} {...rest} />;
}

export interface SegmentOption<T extends string> {
  value: T;
  label: ReactNode;
}

interface SegmentedProps<T extends string> {
  label: string;
  options: readonly SegmentOption<T>[];
  value: T;
  onChange: (value: T) => void;
  /**
   * `radio` picks a setting and follows the radio-group keyboard pattern. `buttons` (the
   * default) switches a view or filter: a labelled group of pressed buttons, each in the Tab
   * order, so nothing is announced as tabs without matching tab panels.
   */
  kind?: "buttons" | "radio";
  size?: "sm" | "md";
  className?: string;
}

const NEXT_KEYS = ["ArrowRight", "ArrowDown"];
const PREVIOUS_KEYS = ["ArrowLeft", "ArrowUp"];

/** The option index a key moves to in a radio group, or null if the key isn't handled. */
export function radioKeyTarget(key: string, index: number, count: number): number | null {
  if (count === 0) {
    return null;
  }
  if (NEXT_KEYS.includes(key)) {
    return (index + 1) % count;
  }
  if (PREVIOUS_KEYS.includes(key)) {
    return (index - 1 + count) % count;
  }
  if (key === "Home") {
    return 0;
  }
  return key === "End" ? count - 1 : null;
}

/**
 * Radio-group keyboard pattern for a set of buttons with role="radio": arrow keys move to and
 * select the previous or next option (wrapping), Home and End the first and last. Focus moves
 * with the selection, which pairs with a roving tab index (only the checked option is
 * tabbable).
 */
export function radioGroupKeyDown<T>(event: KeyboardEvent<HTMLElement>, values: readonly T[], current: T, choose: (value: T) => void) {
  const next = radioKeyTarget(event.key, values.indexOf(current), values.length);
  if (next === null) {
    return;
  }
  event.preventDefault();
  choose(values[next]);
  event.currentTarget.querySelectorAll<HTMLElement>('[role="radio"]')[next]?.focus();
}

export function Segmented<T extends string>({ label, options, value, onChange, kind = "buttons", size = "sm", className }: SegmentedProps<T>) {
  const radio = kind === "radio";
  return (
    <div
      role={radio ? "radiogroup" : "group"}
      aria-label={label}
      className={cx("v2-seg", `v2-seg-${size}`, className)}
      onKeyDown={radio ? (event) => radioGroupKeyDown(event, options.map((option) => option.value), value, onChange) : undefined}
    >
      {options.map((option) => {
        const on = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            className={cx("v2-seg-option", on && "on")}
            onClick={() => onChange(option.value)}
            {...(radio ? { role: "radio", "aria-checked": on, tabIndex: on ? 0 : -1 } : { "aria-pressed": on })}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  size?: "sm" | "md";
  disabled?: boolean;
}

export function Switch({ checked, onChange, label, size = "md", disabled }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className={cx("v2-switch", `v2-switch-${size}`, checked && "on")}
      onClick={() => onChange(!checked)}
    >
      <span className="v2-switch-knob" aria-hidden="true" />
    </button>
  );
}

interface StepperProps {
  value: string;
  onDecrease: () => void;
  onIncrease: () => void;
  label: string;
  canDecrease: boolean;
  canIncrease: boolean;
}

export function Stepper({ value, onDecrease, onIncrease, label, canDecrease, canIncrease }: StepperProps) {
  return (
    <div className="v2-stepper" role="group" aria-label={label}>
      <button type="button" aria-label={`Decrease ${label}`} onClick={onDecrease} disabled={!canDecrease}>−</button>
      <span aria-live="polite">{value}</span>
      <button type="button" aria-label={`Increase ${label}`} onClick={onIncrease} disabled={!canIncrease}>+</button>
    </div>
  );
}

interface SelectProps<T extends string> {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
  disabled?: boolean;
  id?: string;
}

/** A pop-up menu of fixed choices: the native macOS control, styled like the steppers. */
export function Select<T extends string>({ label, value, options, onChange, disabled = false, id }: SelectProps<T>) {
  return (
    <span className="v2-select">
      <select
        id={id}
        aria-label={label}
        value={value}
        disabled={disabled}
        onChange={(event) => {
          const chosen = options.find((option) => option.value === event.target.value);
          if (chosen) {
            onChange(chosen.value);
          }
        }}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>{option.label}</option>
        ))}
      </select>
      <Glyph name="chevronDown" size={11} strokeWidth={2.4} />
    </span>
  );
}

interface GaugeRingProps {
  /** 0–1. Clamped; a sliver is drawn so zero is visible. `null` draws the track only. */
  fraction: number | null;
  tone: Tone;
  size?: number;
  stroke?: number;
  label: string;
  children?: ReactNode;
}

export function GaugeRing({ fraction, tone, size = 62, stroke = 8, label, children }: GaugeRingProps) {
  const gradientId = useId();
  const radius = 36 - stroke / 2 - 2;
  const circumference = 2 * Math.PI * radius;
  const length = fraction === null ? 0 : Math.max(0.02, Math.min(1, fraction)) * circumference;
  return (
    <span className={cx("v2-gauge", `tone-${tone}`)} style={{ width: size, height: size }}>
      <svg role="img" aria-label={label} width={size} height={size} viewBox="0 0 72 72">
        <defs>
          <linearGradient id={gradientId} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" className="v2-stop-1" />
            <stop offset="1" className="v2-stop-2" />
          </linearGradient>
        </defs>
        <circle cx="36" cy="36" r={radius} fill="none" className="v2-track-stroke" strokeWidth={stroke} />
        {fraction !== null && <circle
          className="v2-gauge-value"
          cx="36"
          cy="36"
          r={radius}
          fill="none"
          stroke={`url(#${gradientId})`}
          strokeWidth={stroke}
          strokeLinecap="round"
          strokeDasharray={`${length.toFixed(1)} ${circumference.toFixed(1)}`}
          transform="rotate(-90 36 36)"
        />}
      </svg>
      {children !== undefined && <span className="v2-gauge-center" aria-hidden="true">{children}</span>}
    </span>
  );
}

interface ProgressBarProps {
  /** 0–100. */
  percent: number;
  label: string;
  valueText?: string;
  /** Visual fill; `scan` is the blue-violet in-progress gradient. */
  fill?: "scan" | "ok" | "warn" | "danger" | "neutral" | Tone;
  running?: boolean;
  height?: number;
  /** Minimum drawn width so a just-started bar is visible. */
  minimum?: number;
}

export function ProgressBar({ percent, label, valueText, fill = "scan", running = false, height = 10, minimum = 0 }: ProgressBarProps) {
  const clamped = Math.max(0, Math.min(100, percent));
  const toneClass = ["scan", "ok", "warn", "danger", "neutral"].includes(fill) ? `v2-fill-${fill}` : `tone-${fill} v2-fill-tone`;
  return (
    <div
      className="v2-progress"
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(clamped)}
      aria-valuetext={valueText}
      style={{ height, borderRadius: height / 2 + 1 }}
    >
      <div className={cx("v2-progress-fill", toneClass)} style={{ width: `${Math.max(clamped, minimum)}%`, borderRadius: height / 2 + 1 }}>
        {running && <span className="v2-sheen" aria-hidden="true" />}
      </div>
    </div>
  );
}

export function Spinner({ size = 16, tone = "accent" }: { size?: number; tone?: "accent" | "danger" }) {
  return <span className={cx("v2-spinner", `v2-spinner-${tone}`)} style={{ width: size, height: size }} aria-hidden="true" />;
}

interface ModalProps {
  open: boolean;
  onClose: () => void;
  labelledBy: string;
  /** `alertdialog` for confirmations that interrupt; `dialog` for sheets and drawers. */
  role?: "dialog" | "alertdialog";
  variant?: "center" | "sheet" | "drawer" | "panel" | "full";
  className?: string;
  children: ReactNode;
}

/**
 * A modal on the native <dialog> element: the browser provides the focus trap, makes the page
 * behind it inert and closes it on Escape (reported through onClose so state stays in sync).
 */
export function Modal({ open, onClose, labelledBy, role = "dialog", variant = "center", className, children }: ModalProps) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (dialog === null) {
      return;
    }
    if (open && !dialog.open) {
      dialog.showModal();
    } else if (!open && dialog.open) {
      dialog.close();
    }
  }, [open]);
  if (!open) {
    return null;
  }
  return (
    <dialog
      ref={ref}
      role={role === "alertdialog" ? "alertdialog" : undefined}
      aria-labelledby={labelledBy}
      aria-modal="true"
      className={cx("v2-modal", `v2-modal-${variant}`, className)}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onClick={(event) => {
        // A click on the backdrop lands on the <dialog> itself, outside its content box.
        if (event.target === event.currentTarget && variant !== "center") {
          onClose();
        }
      }}
    >
      {children}
    </dialog>
  );
}

export interface ToastMessage {
  text: string;
  tone: "ok" | "info" | "warn";
}

/** One transient status message at a time, cleared after a few seconds. */
export function useToast(durationMs = 3500) {
  const [message, setMessage] = useState<ToastMessage | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const show = useCallback(
    (text: string, tone: ToastMessage["tone"] = "ok") => {
      clearTimeout(timer.current);
      setMessage({ text, tone });
      timer.current = setTimeout(() => setMessage(null), durationMs);
    },
    [durationMs],
  );
  return { message, show };
}

export function Toast({ message, bottom = 26, children }: { message: ToastMessage | null; bottom?: number; children?: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  // The region lives in the top layer (popover) so a message is never hidden behind an open
  // dialog or drawer; re-showing it on each message moves it above anything opened since.
  useEffect(() => {
    const region = ref.current;
    if (region === null || typeof region.showPopover !== "function") {
      return;
    }
    if (region.matches(":popover-open")) {
      region.hidePopover();
    }
    region.showPopover();
  }, [message]);
  // The live region stays mounted so screen readers announce each new message.
  return (
    <div ref={ref} popover="manual" className="v2-toast-region" role="status" style={{ bottom }}>
      {message && (
        <div className="v2-toast">
          <span className={`v2-toast-mark v2-mark-${message.tone}`} aria-hidden="true">
            <Glyph name={message.tone === "ok" ? "check" : "info"} size={10} strokeWidth={3.2} />
          </span>
          <span>{message.text}</span>
          {children}
        </div>
      )}
    </div>
  );
}

/** The glass action bar that floats over the bottom of a list. */
export function FloatingBar({ children, attached = false }: { children: ReactNode; attached?: boolean }) {
  return <div className={cx("v2-float", attached && "v2-float-attached")}>{children}</div>;
}

interface ScrollBodyProps {
  label: string;
  className?: string;
  children: ReactNode;
}

/** A page's scrolling body; keyboard-focusable so it can be scrolled. */
export function ScrollBody({ label, className, children }: ScrollBodyProps) {
  return (
    <div className={cx("v2-scroll", className)} tabIndex={0} role="region" aria-label={label}>
      {children}
    </div>
  );
}

interface SearchFieldProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
  width?: number;
  shortcut?: string;
  strong?: boolean;
}

export function SearchField({ label, value, onChange, width = 200, shortcut, strong = false }: SearchFieldProps) {
  return (
    <label className={cx("v2-search", strong && "v2-search-strong")} style={{ width }}>
      <Glyph name="search" size={14} />
      <input type="search" aria-label={label} placeholder={label} value={value} onChange={(event) => onChange(event.target.value)} />
      {shortcut && <kbd>{shortcut}</kbd>}
    </label>
  );
}

interface StatCardProps {
  tone: Tone;
  icon: IconName;
  label: string;
  value: ReactNode;
  sub: ReactNode;
  /** `lg` is the 42 px tile used by Quarantine; `md` the 38 px tile used elsewhere. */
  size?: "md" | "lg";
}

export function StatCard({ tone, icon, label, value, sub, size = "md" }: StatCardProps) {
  return (
    <div className={cx("v2-card", "v2-stat", `v2-stat-${size}`)}>
      <IconTile tone={tone} icon={icon} size={size === "lg" ? 42 : 38} glow />
      <span className="v2-stat-text">
        <span className="v2-stat-label">{label}</span>
        <span className="v2-stat-value">{value}</span>
        <span className="v2-stat-sub">{sub}</span>
      </span>
    </div>
  );
}

interface PageHeaderProps {
  title: string;
  headingId: string;
  subtitle: ReactNode;
  actions?: ReactNode;
  /** Content above the title, such as the Cleanup step tabs. */
  above?: ReactNode;
}

export function PageHeader({ title, headingId, subtitle, actions, above }: PageHeaderProps) {
  return (
    <header className={cx("v2-page-header", above !== undefined && "v2-page-header-stacked")}>
      {above}
      <div className="v2-page-header-row">
        <div className="v2-page-title">
          {/* tabIndex -1: receives focus after navigation without joining the Tab order. */}
          <h1 id={headingId} tabIndex={-1}>{title}</h1>
          <span className="v2-page-subtitle">{subtitle}</span>
        </div>
        {actions !== undefined && <div className="v2-page-actions">{actions}</div>}
      </div>
    </header>
  );
}

export type LiveState = "live" | "paused" | "waiting" | "stale" | "unavailable";

const liveLabels: Record<LiveState, string> = {
  live: "Live",
  paused: "Paused",
  waiting: "Waiting",
  stale: "Stale",
  unavailable: "Unavailable",
};

/** Says whether readings are actually arriving. Only "live" pulses green. */
export function LiveBadge({ state }: { state: LiveState }) {
  return (
    <span className="v2-live" role="status">
      <span className={cx("v2-live-dot", state === "live" && "pulse", state === "stale" && "stale")} aria-hidden="true" />
      {liveLabels[state]}
    </span>
  );
}

/** SVG polyline points for a series scaled into a w×h box (0 at the bottom). */
export function seriesPoints(values: readonly number[], width: number, height: number, max: number, padTop = 4, padBottom = 4): string {
  const span = height - padTop - padBottom;
  const step = values.length > 1 ? width / (values.length - 1) : 0;
  return values
    .map((value, index) => `${(index * step).toFixed(1)},${(height - padBottom - Math.min(value / max, 1) * span).toFixed(1)}`)
    .join(" ");
}

export interface ChartSegment {
  points: string;
  firstX: string;
  lastX: string;
}

/**
 * Runs of consecutive readings, right-aligned on a fixed axis of `slots` so the newest sample
 * is at the right edge. A `null` breaks the line, so a missing reading shows as a gap, never
 * as a join. Single isolated readings are skipped: a line needs two points. The default
 * geometry is the System page's 480×150 chart.
 */
export function chartSegments(
  values: readonly (number | null)[],
  slots: number,
  max: number,
  { width = 480, bottom = 146, span = 136 }: { width?: number; bottom?: number; span?: number } = {},
): ChartSegment[] {
  const offset = slots - values.length;
  const step = width / Math.max(1, slots - 1);
  const segments: ChartSegment[] = [];
  let run: string[] = [];
  let runX: string[] = [];
  const flush = () => {
    if (run.length >= 2) {
      segments.push({ points: run.join(" "), firstX: runX[0], lastX: runX[runX.length - 1] });
    }
    run = [];
    runX = [];
  };
  values.forEach((entry, index) => {
    if (entry === null) {
      flush();
      return;
    }
    const x = ((offset + index) * step).toFixed(1);
    run.push(`${x},${(bottom - Math.min(entry / max, 1) * span).toFixed(1)}`);
    runX.push(x);
  });
  flush();
  return segments;
}
