import type { MouseSettings, MouseStatus } from "../../contracts/mouse";

/**
 * A change to the Mouse settings, applied to whatever is saved when its turn comes. `null`
 * means it no longer applies (what it changes is gone), and nothing is written.
 */
export type MouseChange = (settings: MouseSettings) => MouseSettings | null;

export const CHANGED_MEANWHILE = "Not saved: the setting changed while an earlier change was saving. Please try again.";

const apply = (settings: MouseSettings, change: MouseChange) => change(settings) ?? settings;

/**
 * The Mouse page's saves, one at a time. Each change is applied to the last saved settings
 * when it runs, so a change that fails is never carried into a later one. Meanwhile the page
 * shows the saved settings with every change still waiting applied in order; a failed change
 * simply drops out.
 */
export class MouseSaves {
  private confirmed: MouseStatus | null = null;
  private readonly waiting: MouseChange[] = [];
  private tail: Promise<unknown> = Promise.resolve();

  constructor(
    private readonly write: (next: MouseSettings) => Promise<MouseStatus>,
    private readonly describe: (failure: unknown) => string,
    private readonly show: (status: MouseStatus | null) => void,
  ) {}

  /** What the page shows: the saved settings with the waiting changes applied. */
  shown(): MouseStatus | null {
    if (this.confirmed === null) {
      return null;
    }
    return { ...this.confirmed, settings: this.waiting.reduce(apply, this.confirmed.settings) };
  }

  /**
   * Reads the settings (a read, or asking for access) in turn with the saves: after every save
   * queued before it and before any queued after it, so a read never returns from before a
   * later write. Resolves once shown; rejects if the read fails, which doesn't hold up later
   * saves.
   */
  read(load: () => Promise<MouseStatus>): Promise<void> {
    const run = async () => {
      this.confirmed = await load();
      this.show(this.shown());
    };
    const result = this.tail.then(run, run);
    this.tail = result.catch(() => undefined);
    return result;
  }

  /** Queues `change`; resolves with what to tell the user, or `null` once saved. */
  save(change: MouseChange): Promise<string | null> {
    if (this.confirmed === null) {
      return Promise.resolve("The Mouse settings haven’t loaded yet.");
    }
    this.waiting.push(change);
    this.show(this.shown());
    const run = async (): Promise<string | null> => {
      const base = this.confirmed;
      const next = base === null ? null : change(base.settings);
      try {
        if (next === null) {
          return CHANGED_MEANWHILE;
        }
        this.confirmed = await this.write(next);
        return null;
      } catch (failure) {
        return `Not saved: ${this.describe(failure)}`;
      } finally {
        this.waiting.splice(this.waiting.indexOf(change), 1);
        this.show(this.shown());
      }
    };
    const result = this.tail.then(run, run);
    this.tail = result;
    return result;
  }
}
